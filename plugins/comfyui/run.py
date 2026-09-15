#!/usr/bin/env python3
"""Run any API workflow, with explicit typed node inputs and local file uploads."""
import sys
import uuid
from common import (ComfyError, MAX_FILES, OutputBatch, boolean, encode, fields, local_path,
                    main, node_errors, number, text)
from result import collect, prompt_id, selection
from workflow import api_graph, load, validate


def bindings(args, graph):
    updates = []
    uploads = []
    used = set()

    def target(item, allowed, required):
        fields(item, allowed, required)
        node_id = text(item.get("node_id"), "node_id", 128)
        key = text(item.get("input"), "input", 256)
        if node_id not in graph or key not in graph[node_id]["inputs"]:
            raise ComfyError("invalid_arguments", "Binding must name an existing node input; get/edit the workflow first")
        if (node_id, key) in used:
            raise ComfyError("invalid_arguments", "An input has more than one binding; remove conflicting prompt/file/override targets")
        used.add((node_id, key))
        return node_id, key

    def array(key, maximum):
        value = args.get(key, [])
        if not isinstance(value, list) or len(value) > maximum:
            raise ComfyError("invalid_arguments", f"{key} must be an array with at most {maximum} items")
        return value

    prompt_targets = array("prompt_targets", 64)
    if "prompt" in args:
        prompt = args["prompt"]
        if not isinstance(prompt, str) or not prompt.strip() or len(prompt) > 32000 or "\x00" in prompt:
            raise ComfyError("invalid_arguments", "prompt must contain 1..32000 characters")
        if not prompt_targets:
            raise ComfyError("invalid_arguments", "prompt requires explicit prompt_targets; positive/negative nodes are never guessed")
        for item in prompt_targets:
            node_id, key = target(item, {"node_id", "input"}, {"node_id", "input"})
            updates.append((node_id, key, prompt))
    elif prompt_targets:
        raise ComfyError("invalid_arguments", "prompt_targets requires prompt")
    for item in array("overrides", 256):
        node_id, key = target(item, {"node_id", "input", "value"}, {"node_id", "input", "value"})
        updates.append((node_id, key, item["value"]))
    for item in array("files", MAX_FILES):
        node_id, key = target(item, {"node_id", "input", "path"}, {"node_id", "input", "path"})
        path = local_path(item["path"])
        uploads.append((node_id, key, path))
    for node_id, key, value in updates:
        graph[node_id]["inputs"][key] = value
    return uploads


def execute(args, client):
    fields(args, {"workflow_name", "workflow_path", "workflow", "prompt", "prompt_targets", "files", "overrides", "wait_seconds", "output_nodes", "preflight"})
    wait = number(args.get("wait_seconds", 0), "wait_seconds", 0, 900)
    preflight = boolean(args.get("preflight", True), "preflight")
    selected = selection(args)
    graph = api_graph(load(args, client))
    if selected is not None and any(node not in graph for node in selected):
        raise ComfyError("invalid_arguments", "output_nodes contains a node absent from the workflow")
    uploads = bindings(args, graph)
    api_graph(graph)  # Enforce finite JSON/size limits even when preflight is disabled.
    paths = set(path for _, _, path in uploads)
    sizes = [path.stat().st_size for path in paths]
    if any(size > client.file_limit for size in sizes) or sum(sizes) > 4 * client.file_limit:
        raise ComfyError("file_too_large", "Input files exceed the configured per-file/total byte limit")
    # All binding/path/schema checks precede uploads and GPU submission. File
    # choice validation is deferred until /prompt, after uploads create choices.
    if preflight:
        validate(graph, client, {(node, key) for node, key, _ in uploads})
    attempted = False
    job_id = None
    uploaded = []
    try:
        with OutputBatch() as output:
            cache = {}
            for node_id, key, path in uploads:
                if path not in cache:
                    remote, value = client.upload(path)
                    cache[path] = value
                    uploaded.append({"path": str(path), "remote": remote})
                graph[node_id]["inputs"][key] = cache[path]
            snapshot = output.save(encode(api_graph(graph)), "submitted.api.json")
            attempted = True  # From here a lost acknowledgement has unknown outcome.
            queued = client.json("POST", "/prompt", {"prompt": graph, "client_id": str(uuid.uuid4())})
            if not isinstance(queued, dict):
                raise ComfyError("invalid_response", "Malformed ComfyUI /prompt response")
            if queued.get("prompt_id") is not None:
                job_id = prompt_id(queued["prompt_id"])
            if queued.get("error") is not None or queued.get("node_errors"):
                raise ComfyError("workflow_validation", "ComfyUI reported node validation errors; inspect the workflow before another run", node_errors=node_errors(queued))
            if job_id is None:
                raise ComfyError("invalid_response", "ComfyUI did not return a prompt_id")
            result = collect(client, job_id, wait, selected) if wait else {"prompt_id": job_id, "status": "queued", "files": [],
                "note": "Use comfyui_result with this prompt_id to wait/download; do not queue again."}
            return {**result, "submitted_workflow": snapshot, "uploaded_files": uploaded}
    except ComfyError as error:
        error.result.update({"prompt_id": job_id, "submission_attempted": attempted, "uploaded_files": uploaded})
        if attempted:
            error.result["remote_job_note"] = "The job may have executed or still be running. Check history; no resubmission, queue deletion or interrupt was sent."
        raise
    except OSError:
        raise ComfyError("storage", "Cannot read/save workflow files; check storage. Do not resubmit an existing prompt_id.",
                         prompt_id=job_id, submission_attempted=attempted, uploaded_files=uploaded) from None
    except Exception:
        raise ComfyError("invalid_response", "Malformed ComfyUI response; inspect the existing job rather than submitting again.",
                         prompt_id=job_id, submission_attempted=attempted, uploaded_files=uploaded) from None


if __name__ == "__main__":
    sys.exit(main(execute))
