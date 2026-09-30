#!/usr/bin/env python3
"""Create, inspect, edit and export real ComfyUI userdata workflows."""
import copy
import hashlib
import re
import sys
import urllib.parse
from common import (ComfyError, OutputBatch, WORKFLOW_LIMIT, boolean, decode, encode,
                    fields, local_path, main, number, read_file, safe_relative, text)


def workflow_name(value):
    value = safe_relative(value, "workflow name")
    if not value.endswith(".json") or len(value) > 240:
        raise ComfyError("invalid_arguments", "Workflow name must end in .json and contain at most 240 characters")
    return value


def route(name):
    # Entire path encoded as one aiohttp {file} parameter, like ComfyUI frontend.
    return "/userdata/" + urllib.parse.quote("workflows/" + workflow_name(name), safe="")


def workflow_format(graph):
    if len(encode(graph)) > WORKFLOW_LIMIT:
        raise ComfyError("workflow_too_large", "Workflow exceeds the 4 MiB limit")
    if not isinstance(graph, dict) or not graph:
        raise ComfyError("invalid_workflow", "Workflow must be a nonempty JSON object")
    if isinstance(graph.get("nodes"), list):
        if not isinstance(graph.get("links", []), list):
            raise ComfyError("invalid_workflow", "Editor workflow links must be an array")
        return "editor"
    if len(graph) > 2000:
        raise ComfyError("invalid_workflow", "Workflow exceeds 2000 nodes")
    for node_id, node in graph.items():
        text(node_id, "node ID", 128)
        if not isinstance(node, dict) or not isinstance(node.get("inputs"), dict):
            raise ComfyError("invalid_workflow", "Expected API graph nodes with class_type and inputs (not a POST /prompt wrapper)")
        text(node.get("class_type"), "class_type", 256)
    return "api"


def api_graph(graph):
    if workflow_format(graph) != "api":
        raise ComfyError("api_format_required", "This is editor JSON. Use ComfyUI File > Export (API), or construct an API graph from comfyui_nodes schemas. There is no generic server-side editor-to-API converter.")
    return graph


def fetch(client, name):
    raw = client.request("GET", route(name), limit=WORKFLOW_LIMIT)
    graph = decode(raw)
    workflow_format(graph)
    return graph, hashlib.sha256(raw).hexdigest()


def load(args, client, name_key="workflow_name"):
    sources = [key for key in (name_key, "workflow_path", "workflow") if key in args]
    if len(sources) != 1:
        raise ComfyError("invalid_arguments", f"Supply exactly one of {name_key}, workflow_path, or workflow")
    if sources[0] == name_key:
        return fetch(client, args[name_key])[0]
    if sources[0] == "workflow_path":
        return decode(read_file(local_path(args["workflow_path"]), WORKFLOW_LIMIT))
    return copy.deepcopy(args["workflow"])


def validate(graph, client, skip_values=()):
    """Static preflight against installed nodes. /prompt remains authoritative.

    Custom VALIDATE_INPUTS, installed weights, VRAM, cycles resolved by execution,
    and runtime loaders cannot be proved by metadata. Never claim a test run here.
    """
    api_graph(graph)
    schemas = client.json("GET", "/object_info")
    if not isinstance(schemas, dict):
        raise ComfyError("invalid_response", "Malformed ComfyUI node schemas")
    issues = []
    edges = {node_id: set() for node_id in graph}

    def issue(node_id, key, reason):
        if len(issues) < 64:
            issues.append({"node_id": node_id, "input": key, "reason": reason})

    has_output = False
    for node_id, node in graph.items():
        schema = schemas.get(node["class_type"])
        if not isinstance(schema, dict):
            issue(node_id, None, "Node class is not installed; discover available classes with comfyui_nodes")
            continue
        has_output = has_output or schema.get("output_node") is True
        inputs = schema.get("input", {})
        required = inputs.get("required", {})
        optional = inputs.get("optional", {})
        if not isinstance(required, dict) or not isinstance(optional, dict):
            raise ComfyError("invalid_response", "Malformed node input schema")
        declared = {**required, **optional}
        for key in required:
            if key not in node["inputs"]:
                issue(node_id, key, "Required input is missing")
        for key, value in node["inputs"].items():
            spec = declared.get(key)
            if spec is None:
                if not schema.get("accept_all_inputs", False):
                    issue(node_id, key, "Input is not declared by the installed node")
                continue
            if not isinstance(spec, list) or not spec:
                raise ComfyError("invalid_response", "Malformed node input declaration")
            kind = spec[0]
            options = spec[1] if len(spec) > 1 and isinstance(spec[1], dict) else {}
            if isinstance(value, list) and len(value) == 2 and isinstance(value[0], str) and type(value[1]) is int:
                source, slot = value
                if source not in graph:
                    issue(node_id, key, "Connection references a missing node")
                    continue
                edges[node_id].add(source)
                upstream = schemas.get(graph[source]["class_type"], {})
                outputs = upstream.get("output", [])
                if not 0 <= slot < len(outputs):
                    issue(node_id, key, "Connection references a missing output slot")
                elif isinstance(kind, str) and isinstance(outputs[slot], str):
                    compatible = set(kind.split(",")) & set(outputs[slot].split(","))
                    if not compatible and "*" not in (kind, outputs[slot]):
                        issue(node_id, key, "Connected output type does not match this input type")
                continue
            if (node_id, key) in skip_values:
                continue  # Uploaded filename is not in the server's combo until upload.
            if isinstance(kind, list):
                # Upload widgets often list only the input root, while their
                # VALIDATE_INPUTS accepts subfolders (including our praxis/ files).
                # Do not falsely reject a valid previously uploaded filename.
                upload_widget = any(name.endswith("_upload") and enabled is True for name, enabled in options.items())
                if not upload_widget and value not in kind:
                    issue(node_id, key, "Value/model is not in the installed node's choices; inspect comfyui_nodes")
            elif kind == "INT" and type(value) is not int:
                issue(node_id, key, "Expected an integer")
            elif kind == "FLOAT" and type(value) not in (int, float):
                issue(node_id, key, "Expected a number")
            elif kind == "BOOLEAN" and type(value) is not bool:
                issue(node_id, key, "Expected a boolean")
            elif kind == "STRING" and not isinstance(value, str):
                issue(node_id, key, "Expected a string")
            elif isinstance(kind, str) and kind not in ("INT", "FLOAT", "BOOLEAN", "STRING", "*"):
                # Custom types may have custom widgets; defer their literal validation.
                if options.get("forceInput"):
                    issue(node_id, key, "This input requires a node connection")
            if type(value) in (int, float):
                if "min" in options and value < options["min"] or "max" in options and value > options["max"]:
                    issue(node_id, key, "Numeric input is outside the node's range")
    if not has_output:
        issue("", None, "Workflow needs an installed output node (for example SaveImage)")
    # Iterative topological elimination avoids recursion depth limits on large graphs.
    pending = {node: set(sources) for node, sources in edges.items()}
    while pending:
        ready = {node for node, sources in pending.items() if not sources}
        if not ready:
            issue("", None, "Workflow contains a connection cycle")
            break
        pending = {node: sources - ready for node, sources in pending.items() if node not in ready}
    if issues:
        raise ComfyError("workflow_validation", "Workflow failed installed-node preflight; nothing was queued", issues=issues)
    return {"valid": True, "validation": "installed_node_preflight", "nodes": len(graph),
            "execution_verified": False, "note": "Only a successful comfyui_run + comfyui_result verifies runtime execution. Custom validation, weights and GPU availability are checked by ComfyUI."}


def pointer(path):
    if path == "":
        return []
    if not isinstance(path, str) or not path.startswith("/") or re.search(r"~(?![01])", path):
        raise ComfyError("invalid_patch", "Patch paths must be JSON pointers")
    return [part.replace("~1", "/").replace("~0", "~") for part in path[1:].split("/")]


def json_equal(left, right):
    # JSON booleans are not numbers (Python's True == 1 must not pass a test).
    if type(left) in (int, float) and type(right) in (int, float):
        return left == right
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(json_equal(value, right[key]) for key, value in left.items())
    if isinstance(left, list):
        return len(left) == len(right) and all(json_equal(a, b) for a, b in zip(left, right))
    return left == right


def patch_workflow(original, changes):
    if not isinstance(changes, list) or not 1 <= len(changes) <= 100:
        raise ComfyError("invalid_patch", "patch must contain 1..100 add/replace/remove/test operations")
    graph = copy.deepcopy(original)
    try:
        for change in changes:
            fields(change, {"op", "path", "value"}, {"op", "path"})
            op = change["op"]
            if op not in ("add", "replace", "remove", "test") or (op != "remove" and "value" not in change):
                raise ComfyError("invalid_patch", "Unsupported or incomplete patch operation")
            parts = pointer(change["path"])
            if not parts:
                if op in ("add", "replace"):
                    graph = copy.deepcopy(change["value"])
                elif op == "test" and json_equal(graph, change["value"]):
                    pass
                else:
                    raise ComfyError("invalid_patch", "Root removal or failed test")
                continue
            target = graph
            for part in parts[:-1]:
                target = target[index(part, len(target))] if isinstance(target, list) else target[part]
            key = parts[-1]
            if isinstance(target, list):
                position = len(target) if key == "-" and op == "add" else index(key, len(target), op == "add")
                if op == "add":
                    target.insert(position, copy.deepcopy(change["value"]))
                elif op == "remove":
                    target.pop(position)
                elif op == "replace":
                    target[position] = copy.deepcopy(change["value"])
                elif not json_equal(target[position], change["value"]):
                    raise ComfyError("invalid_patch", "Patch test failed")
            elif isinstance(target, dict):
                if op != "add" and key not in target:
                    raise KeyError(key)
                if op in ("add", "replace"):
                    target[key] = copy.deepcopy(change["value"])
                elif op == "remove":
                    del target[key]
                elif not json_equal(target[key], change["value"]):
                    raise ComfyError("invalid_patch", "Patch test failed")
            else:
                raise TypeError()
    except (KeyError, IndexError, TypeError, ValueError):
        raise ComfyError("invalid_patch", "Patch references a missing/invalid location; nothing was saved") from None
    workflow_format(graph)
    return graph


def index(part, length, adding=False):
    if not re.fullmatch(r"0|[1-9][0-9]*", part) or len(part) > 8:
        raise ValueError()
    value = int(part)
    if value >= length + int(adding):
        raise IndexError()
    return value


def execute(args, client):
    action = args.get("action")
    allowed = {
        "list": {"action", "limit", "offset"},
        "get": {"action", "name", "include_workflow"},
        "save": {"action", "name", "workflow", "workflow_path", "overwrite"},
        "edit": {"action", "name", "expected_sha256", "patch"},
        "validate": {"action", "workflow_name", "workflow_path", "workflow"},
    }
    if action not in allowed:
        raise ComfyError("invalid_arguments", "action must be list, get, save, edit or validate")
    fields(args, allowed[action])
    if action == "validate":
        return validate(load(args, client), client)
    if action == "list":
        limit = number(args.get("limit", 20), "limit", 1, 100, integer=True)
        offset = number(args.get("offset", 0), "offset", 0, 1000000, integer=True)
        try:
            listing = client.json("GET", "/userdata", query={"dir": "workflows", "recurse": "true", "split": "false"})
        except ComfyError as error:
            if error.result.get("http_status") != 404:
                raise
            listing = []  # New ComfyUI profile may not have a workflows directory yet.
        if not isinstance(listing, list) or any(not isinstance(name, str) for name in listing):
            raise ComfyError("invalid_response", "Malformed workflow listing")
        names = sorted(name for name in listing if name.endswith(".json"))
        return {"workflows": names[offset:offset + limit], "next_offset": offset + limit if offset + limit < len(names) else None}
    name = workflow_name(args.get("name"))
    if action == "save":
        overwrite = boolean(args.get("overwrite", False), "overwrite")
        graph = load({key: value for key, value in args.items() if key in ("workflow", "workflow_path")}, client)
    else:
        graph, digest = fetch(client, name)
        if action == "edit":
            expected = text(args.get("expected_sha256"), "expected_sha256", 64)
            if expected != digest:
                raise ComfyError("workflow_changed", "Workflow changed since it was read. Get it again and reapply the intended edits.")
            graph = patch_workflow(graph, args.get("patch"))
            overwrite = True
    kind = workflow_format(graph)
    raw = encode(graph)
    with OutputBatch() as output:
        file = output.save(raw, name)  # Preflight storage before any server mutation.
        if action in ("save", "edit"):
            client.request("POST", route(name), raw, query={"overwrite": str(overwrite).lower()})
            digest = hashlib.sha256(raw).hexdigest()
        result = {"name": name, "format": kind, "sha256": digest, "file": file, "execution_verified": False}
        if action == "get" and boolean(args.get("include_workflow", True), "include_workflow"):
            result["workflow"] = graph
        return result


if __name__ == "__main__":
    sys.exit(main(execute))
