#!/usr/bin/env python3
"""Offline contract tests: local fake ComfyUI only; no models or GPU execution."""
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
import unittest
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parents[1]
PLUGIN = ROOT / "plugins" / "comfyui"
GRAPH = {
    "6": {"class_type": "CLIPTextEncode", "inputs": {"text": "old", "clip": ["4", 1]}},
    "7": {"class_type": "CLIPTextEncode", "inputs": {"text": "negative", "clip": ["4", 1]}},
    "10": {"class_type": "LoadImage", "inputs": {"image": "old.png"}},
    "3": {"class_type": "KSampler", "inputs": {"seed": 123, "positive": ["6", 0]}},
    "4": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "model.safetensors"}},
    "9": {"class_type": "SaveImage", "inputs": {"images": ["10", 0]}},
}
SCHEMAS = {
    "CLIPTextEncode": {"input": {"required": {"text": ["STRING"], "clip": ["CLIP"]}}, "output": ["CONDITIONING"]},
    "LoadImage": {"input": {"required": {"image": [["old.png"]]}}, "output": ["IMAGE"]},
    "KSampler": {"input": {"required": {"seed": ["INT", {"min": 0}], "positive": ["CONDITIONING"]}}, "output": ["LATENT"]},
    "CheckpointLoaderSimple": {"input": {"required": {"ckpt_name": [["model.safetensors"]]}}, "output": ["MODEL", "CLIP", "VAE"]},
    "SaveImage": {"input": {"required": {"images": ["IMAGE"]}}, "output": [], "output_node": True},
}
PNG = b"\x89PNG\r\n\x1a\n" + b"offline fixture"


class Server(ThreadingHTTPServer):
    daemon_threads = True


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, data, status=200, raw=False):
        body = data if raw else json.dumps(data).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream" if raw else "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_GET(self):
        parsed = urllib.parse.urlsplit(self.path)
        path = urllib.parse.unquote(parsed.path)
        self.server.requests.append(("GET", self.path, None, dict(self.headers)))
        if self.server.redirect:
            self.send_response(307)
            self.send_header("Location", "/do-not-follow")
            self.end_headers()
            return
        if path.startswith("/object_info"):
            nodes = self.server.schemas
            if path != "/object_info":
                name = path.removeprefix("/object_info/")
                nodes = {name: nodes[name]} if name in nodes else {}
            return self.reply(nodes)
        if path == "/userdata":
            return self.reply(list(self.server.workflows))
        if path.startswith("/userdata/"):
            name = path.removeprefix("/userdata/workflows/")
            if name not in self.server.workflows:
                return self.reply({}, 404)
            return self.reply(self.server.workflows[name], raw=True)
        if path == "/history/job-1":
            return self.reply(self.server.history)
        if path == "/view":
            query = urllib.parse.parse_qs(parsed.query)
            self.server.views.append(query)
            if query.get("filename") == [self.server.bad_media]:
                return self.reply({"error": "DO-NOT-ECHO-SECRET"}, 500)
            if self.server.trickle:
                self.send_response(200)
                self.end_headers()
                for byte in self.server.media:
                    try:
                        self.wfile.write(bytes([byte]))
                        self.wfile.flush()
                    except (BrokenPipeError, ConnectionResetError):
                        break
                    time.sleep(0.05)
                return
            return self.reply(self.server.media, raw=True)
        self.reply({}, 404)

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        self.server.requests.append(("POST", self.path, body, dict(self.headers)))
        parsed = urllib.parse.urlsplit(self.path)
        path = urllib.parse.unquote(parsed.path)
        if path.startswith("/userdata/workflows/"):
            name = path.removeprefix("/userdata/workflows/")
            overwrite = urllib.parse.parse_qs(parsed.query).get("overwrite", ["true"])[0]
            if name in self.server.workflows and overwrite == "false":
                return self.reply({}, 409)
            self.server.workflows[name] = body
            return self.reply("workflows/" + name)
        if path == "/upload/image":
            return self.reply(self.server.upload)
        if path == "/prompt":
            self.server.prompts.append(json.loads(body))
            if self.server.delay:
                time.sleep(self.server.delay)
            if self.server.prompt_status != 200:
                return self.reply({"error": {"message": "DO-NOT-ECHO-SECRET"}, "node_errors": {"6": {"errors": [{"type": "required_input_missing", "message": "DO-NOT-ECHO-SECRET"}]}}}, self.server.prompt_status)
            return self.reply({"prompt_id": "job-1", "node_errors": {}})
        self.reply({}, 404)


class ComfyPluginTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = Server(("127.0.0.1", 0), Handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.data = Path(self.tmp.name)
        self.server.requests = []
        self.server.schemas = copy.deepcopy(SCHEMAS)
        self.server.workflows = {"edit.api.json": json.dumps(GRAPH).encode()}
        self.server.prompts = []
        self.server.views = []
        self.server.upload = {"name": "renamed.png", "subfolder": "praxis", "type": "input"}
        self.server.history = {}
        self.server.media = PNG
        self.server.prompt_status = 200
        self.server.delay = 0
        self.server.redirect = False
        self.server.trickle = False
        self.server.bad_media = None

    def call(self, script, args, context=None, secrets=None, env=None, success=True):
        environment = {key: value for key, value in os.environ.items() if not key.startswith(("COMFYUI_", "PLUGIN_", "VM_"))}
        environment.update({"COMFYUI_BASE_URL": "http://127.0.0.1:" + str(self.server.server_port),
                            "DATA_DIR": str(self.data), "PLUGIN_ARGS": json.dumps(args),
                            "PLUGIN_CONTEXT": json.dumps(context or {}), "PLUGIN_SECRETS": json.dumps(secrets or {}),
                            "PYTHONDONTWRITEBYTECODE": "1"})
        environment.update(env or {})
        process = subprocess.run(["python3", str(PLUGIN / (script + ".py"))], env=environment, capture_output=True, text=True, timeout=8)
        self.assertEqual(process.returncode, 0 if success else 1, process.stdout + process.stderr)
        self.assertEqual(process.stderr, "")
        return json.loads(process.stdout)

    def completed(self, outputs=None, status=None):
        self.server.history = {"job-1": {"status": status or {"status_str": "success", "completed": True, "messages": []},
                                        "outputs": outputs if outputs is not None else {"9": {"images": [{"filename": "image.png", "subfolder": "nested", "type": "output"}]}}}}

    def test_discovery_is_bounded_and_can_get_real_node_inputs(self):
        result = self.call("nodes", {"query": "loadimage", "limit": 1})
        self.assertEqual([node["class_type"] for node in result["nodes"]], ["LoadImage"])
        result = self.call("nodes", {"node_class": "CLIPTextEncode"})
        self.assertIn("input", result["nodes"]["CLIPTextEncode"])
        self.assertFalse(self.server.prompts)

    def test_create_get_edit_download_and_no_accidental_overwrite(self):
        created = self.call("workflow", {"action": "save", "name": "folder/new.api.json", "workflow": GRAPH})
        self.assertEqual(created["format"], "api")
        self.assertIn("folder/new.api.json", self.server.workflows)
        conflict = self.call("workflow", {"action": "save", "name": "folder/new.api.json", "workflow": GRAPH}, success=False)
        self.assertEqual(conflict["http_status"], 409)
        fetched = self.call("workflow", {"action": "get", "name": "folder/new.api.json"})
        self.assertEqual(json.loads(Path(fetched["file"]["path"]).read_text()), GRAPH)
        self.assertTrue(fetched["file"]["download_url"].startswith("/api/files/"))
        patch = [{"op": "replace", "path": "/6/inputs/text", "value": "new"},
                 {"op": "add", "path": "/11", "value": {"class_type": "SaveImage", "inputs": {"images": ["10", 0]}}},
                 {"op": "remove", "path": "/7"}]
        self.call("workflow", {"action": "edit", "name": "folder/new.api.json", "expected_sha256": fetched["sha256"], "patch": patch})
        changed = json.loads(self.server.workflows["folder/new.api.json"])
        self.assertEqual(changed["6"]["inputs"]["text"], "new")
        self.assertEqual(changed["6"]["inputs"]["clip"], ["4", 1])
        self.assertIn("11", changed)
        self.assertNotIn("7", changed)
        error = self.call("workflow", {"action": "edit", "name": "folder/new.api.json", "expected_sha256": fetched["sha256"], "patch": patch}, success=False)
        self.assertEqual(error["code"], "workflow_changed")
        listed = self.call("workflow", {"action": "list", "limit": 1})
        self.assertEqual(len(listed["workflows"]), 1)
        self.assertIsNotNone(listed["next_offset"])

    def test_edit_rejects_invalid_patch_before_saving_and_supports_ui_json(self):
        original = self.server.workflows["edit.api.json"]
        args = {"action": "edit", "name": "edit.api.json", "expected_sha256": hashlib.sha256(original).hexdigest(),
                "patch": [{"op": "replace", "path": "/6/inputs/missing", "value": "x"}]}
        self.call("workflow", args, success=False)
        self.assertEqual(self.server.workflows["edit.api.json"], original)
        ui = {"version": 0.4, "nodes": [{"id": 1, "widgets_values": ["hello"]}], "links": []}
        self.call("workflow", {"action": "save", "name": "visual.json", "workflow": ui})
        response = self.call("run", {"workflow_name": "visual.json"}, success=False)
        self.assertEqual(response["code"], "api_format_required")
        self.assertFalse(self.server.prompts)

    def test_run_reuses_old_attachment_and_only_binds_explicit_inputs(self):
        old = self.data / "downloads" / "old image.png"
        old.parent.mkdir()
        old.write_bytes(PNG)
        result = self.call("run", {"workflow_name": "edit.api.json", "prompt": 'put this on the moon "☾"',
                                   "prompt_targets": [{"node_id": "6", "input": "text"}],
                                   "files": [{"path": str(old), "node_id": "10", "input": "image"}],
                                   "overrides": [{"node_id": "3", "input": "seed", "value": 42}]},
                           secrets={"comfyui_api_key": "test-token"}, context={"comfyui_workflow_user": "profile-1"})
        self.assertEqual(result["prompt_id"], "job-1")
        self.assertEqual(result["status"], "queued")
        self.assertEqual(len(self.server.prompts), 1)
        graph = self.server.prompts[0]["prompt"]
        self.assertEqual(graph["6"]["inputs"]["text"], 'put this on the moon "☾"')
        self.assertEqual(graph["7"]["inputs"]["text"], "negative")
        self.assertEqual(graph["10"]["inputs"]["image"], "praxis/renamed.png")
        self.assertEqual(graph["3"]["inputs"]["seed"], 42)
        self.assertIsInstance(graph["3"]["inputs"]["seed"], int)
        self.assertEqual(json.loads(self.server.workflows["edit.api.json"]), GRAPH)
        upload = next(item for item in self.server.requests if item[1] == "/upload/image")
        self.assertIn(PNG, upload[2])
        self.assertIn(b'name="image"; filename="praxis_', upload[2])
        for _, _, _, headers in self.server.requests:
            headers = {k.lower(): v for k, v in headers.items()}
            self.assertEqual(headers["authorization"], "Bearer test-token")
            self.assertEqual(headers["comfy-user"], "profile-1")

    def test_vm_and_dashboard_paths_map_to_existing_host_files(self):
        for alias, local in [("/mnt/shared/downloads/prior.wav", self.data / "shared/downloads/prior.wav"),
                             ("/api/files/prior.mp4", self.data / "uploads/prior.mp4")]:
            local.parent.mkdir(parents=True, exist_ok=True)
            local.write_bytes(b"opaque file passed unchanged to the configured loader")
            self.call("run", {"workflow": GRAPH, "files": [{"path": alias, "node_id": "10", "input": "image"}]})
        self.assertEqual(len(self.server.prompts), 2)

    def test_validation_and_file_preflight_happen_before_upload_or_submission(self):
        cases = [
            {"prompt": "no target"},
            {"prompt": "wrong target", "prompt_targets": [{"node_id": "999", "input": "text"}]},
            {"overrides": [{"node_id": "6", "input": "typo", "value": "x"}]},
            {"files": [{"path": "/does/not/exist", "node_id": "10", "input": "image"}]},
            {"files": [{"path": "https://cdn.discordapp.com/a.png", "node_id": "10", "input": "image"}]},
            {"files": [{"path": "/mnt/shared/../../etc/passwd", "node_id": "10", "input": "image"}]},
            {"prompt": "x", "prompt_targets": [{"node_id": "6", "input": "text"}], "overrides": [{"node_id": "6", "input": "text", "value": "y"}]},
            {"wait_seconds": True},
        ]
        for args in cases:
            with self.subTest(args=args):
                self.call("run", {"workflow": GRAPH, **args}, success=False)
        self.assertFalse(any(method == "POST" for method, *_ in self.server.requests))

    def test_result_downloads_all_descriptor_kinds_to_unique_safe_local_files(self):
        self.completed({"9": {"images": [{"filename": "image.png", "subfolder": "nested", "type": "output"}],
                              "audio": [{"filename": "speech.wav", "subfolder": "", "type": "output"}],
                              "gifs": [{"filename": "movie.mp4", "subfolder": "", "type": "temp"}], "text": ["done"]}})
        result = self.call("result", {"prompt_id": "job-1"})
        self.assertEqual(result["status"], "completed")
        self.assertEqual(len(result["files"]), 3)
        for file in result["files"]:
            self.assertEqual(Path(file["path"]).parent, self.data / "uploads")
            self.assertEqual(Path(file["path"]).read_bytes(), PNG)
            self.assertNotEqual(Path(file["path"]).name, file["remote"]["filename"])
        self.assertEqual(len(self.server.views), 3)
        self.assertFalse(self.server.prompts)

    def test_pending_and_error_history_never_resubmit(self):
        pending = self.call("result", {"prompt_id": "job-1", "wait_seconds": 0.05})
        self.assertEqual(pending["status"], "pending")
        self.completed(status={"status_str": "error", "completed": False, "messages": [["execution_error", {"exception_message": "DO-NOT-ECHO-SECRET"}]]})
        error = self.call("result", {"prompt_id": "job-1"}, success=False)
        self.assertEqual(error["code"], "execution_error")
        self.assertEqual(error["prompt_id"], "job-1")
        self.assertNotIn("DO-NOT-ECHO-SECRET", json.dumps(error))
        self.assertFalse(self.server.prompts)

    def test_unsafe_output_descriptor_is_rejected_before_any_download(self):
        for name, folder in [("../secret", ""), ("safe.png", "../../secret"), ("safe.png [input]", ""), ("C:\\secret", ""), ("%2e%2e.png", "")]:
            self.completed({"9": {"images": [{"filename": "good.png", "subfolder": "", "type": "output"},
                                               {"filename": name, "subfolder": folder, "type": "output"}]}})
            self.call("result", {"prompt_id": "job-1"}, success=False)
        self.assertFalse(self.server.views)
        self.assertFalse(list((self.data / "uploads").glob("comfyui_*")))

    def test_validation_error_and_lost_ack_are_not_retried(self):
        self.server.prompt_status = 400
        error = self.call("run", {"workflow": GRAPH}, success=False)
        self.assertEqual(error["http_status"], 400)
        self.assertIn("required_input_missing", json.dumps(error))
        self.assertNotIn("DO-NOT-ECHO-SECRET", json.dumps(error))
        self.assertEqual(len(self.server.prompts), 1)
        self.server.prompt_status = 200
        self.server.delay = 0.3
        error = self.call("run", {"workflow": GRAPH}, env={"COMFYUI_WORKFLOW_TIMEOUT_SECONDS": "0.1"}, success=False)
        self.assertEqual(error["code"], "timeout")
        self.assertTrue(error["submission_attempted"])
        self.assertFalse(error["retry_safe"])
        self.assertEqual(len(self.server.prompts), 2)

    def test_invalid_configuration_and_workflow_names_fail_without_requests(self):
        for url in ["ftp://localhost:8188", "http://user:password@localhost:8188", "http://localhost:8188?token=secret", "http://8.8.8.8:8188"]:
            self.call("nodes", {}, env={"COMFYUI_BASE_URL": url}, success=False)
        for name in ["../secret.json", "/root.json", "%2e%2e/secret.json", "x\\y.json", "name.txt"]:
            self.call("workflow", {"action": "get", "name": name}, success=False)
        self.assertFalse(self.server.requests)

    def test_installed_node_preflight_rejects_incomplete_graphs_without_execution(self):
        valid = self.call("workflow", {"action": "validate", "workflow": GRAPH})
        self.assertTrue(valid["valid"])
        self.assertFalse(valid["execution_verified"])
        changes = [
            ("4", "ckpt_name", "nonexistent-model.safetensors"),
            ("6", "clip", ["missing", 0]),
            ("6", "clip", ["4", 99]),
            ("6", "clip", ["10", 0]),
            ("3", "seed", -1),
            ("3", "seed", "not an integer"),
        ]
        for node, key, value in changes:
            graph = copy.deepcopy(GRAPH)
            graph[node]["inputs"][key] = value
            error = self.call("run", {"workflow": graph}, success=False)
            self.assertEqual(error["code"], "workflow_validation")
        for change in ["missing_input", "no_output", "uninstalled", "cycle"]:
            graph = copy.deepcopy(GRAPH)
            if change == "missing_input":
                del graph["6"]["inputs"]["clip"]
            elif change == "no_output":
                del graph["9"]
            elif change == "uninstalled":
                graph["10"]["class_type"] = "NotInstalled"
            else:
                graph["6"]["inputs"]["clip"] = ["6", 0]
            error = self.call("workflow", {"action": "validate", "workflow": graph}, success=False)
            self.assertEqual(error["code"], "workflow_validation")
        self.assertFalse(any(method == "POST" for method, *_ in self.server.requests))

    def test_complete_example_graphs_have_all_connections_and_run_with_bindings(self):
        schemas = self.server.schemas
        schemas["KSampler"]["input"]["required"].update({
            "model": ["MODEL"], "negative": ["CONDITIONING"], "latent_image": ["LATENT"],
            "steps": ["INT", {"min": 1}], "cfg": ["FLOAT"], "denoise": ["FLOAT", {"min": 0, "max": 1}],
            "sampler_name": [["euler"]], "scheduler": [["normal"]]})
        schemas["SaveImage"]["input"]["required"]["filename_prefix"] = ["STRING"]
        schemas["EmptyLatentImage"] = {"input": {"required": {"width": ["INT"], "height": ["INT"], "batch_size": ["INT"]}}, "output": ["LATENT"]}
        schemas["VAEDecode"] = {"input": {"required": {"samples": ["LATENT"], "vae": ["VAE"]}}, "output": ["IMAGE"]}
        schemas["VAEEncode"] = {"input": {"required": {"pixels": ["IMAGE"], "vae": ["VAE"]}}, "output": ["LATENT"]}
        reference = self.data / "reference.png"
        reference.write_bytes(PNG)
        self.completed()
        for example in ["txt2img", "img2img"]:
            graph = json.loads((PLUGIN / "examples" / (example + ".api.json")).read_text())
            graph["4"]["inputs"]["ckpt_name"] = "model.safetensors"
            if example == "img2img":
                graph["10"]["inputs"]["image"] = "old.png"
            self.call("workflow", {"action": "save", "name": example + ".json", "workflow": graph})
            self.call("workflow", {"action": "validate", "workflow_name": example + ".json"})
            result = self.call("run", {"workflow_name": example + ".json", "prompt": "a new\\nmultiline prompt",
                                       "prompt_targets": [{"node_id": "6", "input": "text"}], "wait_seconds": 1,
                                       "files": [{"path": str(reference), "node_id": "10", "input": "image"}] if example == "img2img" else []})
            self.assertTrue(result["execution_verified"])
            self.assertEqual(len(result["files"]), 1)
        self.assertEqual(len(self.server.prompts), 2)

    def test_custom_node_preflight_can_defer_to_comfyui_authoritative_validation(self):
        graph = copy.deepcopy(GRAPH)
        graph["10"]["class_type"] = "CustomDynamicNode"
        self.call("run", {"workflow": graph, "preflight": False})
        self.assertEqual(len(self.server.prompts), 1)
        self.assertFalse(any(path.startswith("/object_info") for _, path, *_ in self.server.requests))

    def test_download_batch_failure_cleans_published_files_and_keeps_job_id(self):
        self.completed({"9": {"images": [{"filename": "good.png", "subfolder": "", "type": "output"},
                                         {"filename": "broken.png", "subfolder": "", "type": "output"}]}})
        self.server.bad_media = "broken.png"
        error = self.call("result", {"prompt_id": "job-1"}, success=False)
        self.assertEqual(error["prompt_id"], "job-1")
        self.assertEqual(error["http_status"], 500)
        self.assertEqual(list((self.data / "uploads").iterdir()), [])
        self.assertEqual(len(self.server.views), 2)

    def test_response_limits_and_trickle_download_timeout_leave_no_files(self):
        self.completed()
        error = self.call("result", {"prompt_id": "job-1"}, env={"COMFYUI_MAX_FILE_BYTES": "4"}, success=False)
        self.assertEqual(error["code"], "response_too_large")
        self.server.trickle = True
        started = time.monotonic()
        error = self.call("result", {"prompt_id": "job-1"}, env={"COMFYUI_WORKFLOW_TIMEOUT_SECONDS": "0.1"}, success=False)
        self.assertEqual(error["code"], "timeout")
        self.assertEqual(error["prompt_id"], "job-1")
        self.assertLess(time.monotonic() - started, 2)
        self.assertEqual(list((self.data / "uploads").iterdir()), [])

    def test_no_redirects_or_environment_proxy_leaks(self):
        self.call("nodes", {}, env={"HTTP_PROXY": "http://127.0.0.1:1", "http_proxy": "http://127.0.0.1:1", "NO_PROXY": "", "no_proxy": ""})
        self.server.redirect = True
        error = self.call("nodes", {}, success=False)
        self.assertEqual(error["http_status"], 307)
        self.assertFalse(any(path == "/do-not-follow" for _, path, *_ in self.server.requests))

    def test_local_workflow_input_and_vm_only_file_boundaries(self):
        workflow = self.data / "source.json"
        workflow.write_text(json.dumps(GRAPH))
        self.call("workflow", {"action": "save", "name": "import.api.json", "workflow_path": str(workflow)})
        self.call("run", {"workflow_path": str(workflow)})
        error = self.call("run", {"workflow_path": str(workflow)}, env={"VM_ENABLED": "true", "VM_MODE": "vm"}, success=False)
        self.assertEqual(error["code"], "unsafe_path")
        shared = self.data / "shared"
        shared.mkdir()
        (shared / "escape").symlink_to(workflow)
        error = self.call("run", {"workflow_path": "/mnt/shared/escape"}, success=False)
        self.assertEqual(error["code"], "unsafe_path")

    def test_file_limits_and_duplicate_upload_deduplication(self):
        reference = self.data / "input.anything"
        reference.write_bytes(b"arbitrary file contents")
        graph = copy.deepcopy(GRAPH)
        graph["12"] = copy.deepcopy(graph["10"])
        files = [{"path": str(reference), "node_id": node, "input": "image"} for node in ["10", "12"]]
        error = self.call("run", {"workflow": graph, "files": files}, env={"COMFYUI_MAX_FILE_BYTES": "2"}, success=False)
        self.assertEqual(error["code"], "file_too_large")
        self.assertFalse(self.server.requests)
        self.call("run", {"workflow": graph, "files": files})
        self.assertEqual(sum(path == "/upload/image" for _, path, *_ in self.server.requests), 1)

    def test_json_pointer_array_edits_are_transactional(self):
        graph = {"nodes": [{"id": 1, "widgets_values": ["old", 10]}], "links": [], "a/b": {"~key": False}}
        raw = json.dumps(graph).encode()
        self.server.workflows["editor.json"] = raw
        changes = [{"op": "test", "path": "/nodes/0/id", "value": 1},
                   {"op": "replace", "path": "/nodes/0/widgets_values/0", "value": "new"},
                   {"op": "add", "path": "/nodes/0/widgets_values/-", "value": True},
                   {"op": "replace", "path": "/a~1b/~0key", "value": True}]
        self.call("workflow", {"action": "edit", "name": "editor.json", "expected_sha256": hashlib.sha256(raw).hexdigest(), "patch": changes})
        changed = json.loads(self.server.workflows["editor.json"])
        self.assertEqual(changed["nodes"][0]["widgets_values"], ["new", 10, True])
        self.assertTrue(changed["a/b"]["~key"])
        raw = self.server.workflows["editor.json"]
        self.call("workflow", {"action": "edit", "name": "editor.json", "expected_sha256": hashlib.sha256(raw).hexdigest(),
                               "patch": [{"op": "replace", "path": "/nodes/0/id", "value": 99}, {"op": "test", "path": "/nodes/0/id", "value": 1}]}, success=False)
        self.assertEqual(self.server.workflows["editor.json"], raw)

    def test_previously_uploaded_subfolder_file_is_not_rejected_by_root_only_widget_choices(self):
        self.server.schemas["LoadImage"]["input"]["required"]["image"] = [["old.png"], {"image_upload": True}]
        self.call("run", {"workflow": GRAPH, "overrides": [{"node_id": "10", "input": "image", "value": "praxis/already-uploaded.png"}]})
        self.assertFalse(any(path == "/upload/image" for _, path, *_ in self.server.requests))
        self.assertEqual(self.server.prompts[0]["prompt"]["10"]["inputs"]["image"], "praxis/already-uploaded.png")

    def test_arbitrary_empty_files_and_large_structured_outputs_are_preserved(self):
        self.server.media = b""
        self.completed({"9": {"custom_data": [{"filename": "empty.txt", "type": "input", "subfolder": ""}], "text": ["x" * 40000]}})
        result = self.call("result", {"prompt_id": "job-1"})
        self.assertEqual(result["files"][0]["bytes"], 0)
        self.assertTrue(result["outputs_omitted"])
        self.assertEqual(json.loads(Path(result["outputs_file"]["path"]).read_text()), self.server.history["job-1"]["outputs"])

    def test_patch_test_boolean_is_not_equal_to_a_number(self):
        raw = self.server.workflows["edit.api.json"]
        self.call("workflow", {"action": "edit", "name": "edit.api.json", "expected_sha256": hashlib.sha256(raw).hexdigest(),
                               "patch": [{"op": "add", "path": "/3/inputs/test_flag", "value": True},
                                         {"op": "test", "path": "/3/inputs/test_flag", "value": 1}]}, success=False)
        self.assertEqual(raw, self.server.workflows["edit.api.json"])

    def test_plugin_folder_runs_standalone_after_install_copy(self):
        installed = self.data / "separate installation" / "comfyui"
        shutil.copytree(PLUGIN, installed, ignore=shutil.ignore_patterns("__pycache__"))
        result = self.call(str(installed / "nodes"), {"node_class": "LoadImage"})
        self.assertIn("LoadImage", result["nodes"])
        result = self.call(str(installed / "run"), {"workflow": GRAPH})
        self.assertEqual(result["prompt_id"], "job-1")

    def test_manifest_uses_existing_plugin_handlers(self):
        manifest = json.loads((PLUGIN / "plugin.json").read_text())
        self.assertEqual(manifest["name"], "comfyui")
        self.assertEqual({t["name"] for t in manifest["tools"]}, {"comfyui_nodes", "comfyui_workflow", "comfyui_run", "comfyui_result"})
        for tool in manifest["tools"]:
            self.assertEqual(tool["handler"]["type"], "script")
            self.assertTrue((PLUGIN / tool["handler"]["path"]).is_file())


if __name__ == "__main__":
    unittest.main()
