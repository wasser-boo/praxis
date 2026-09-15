#!/usr/bin/env python3
"""Resume a known prompt ID, wait optionally, and download its completed outputs."""
import re
import sys
import time
from common import ComfyError, MAX_FILES, OutputBatch, descriptor, encode, fields, main, number, text


def prompt_id(value):
    value = text(value, "prompt_id", 128)
    if not re.fullmatch(r"[A-Za-z0-9_-]+", value):
        raise ComfyError("invalid_arguments", "Invalid prompt_id")
    return value


def output_files(outputs, selected=None):
    """Support arbitrary output keys (images, audio, gifs, video, custom files).

    Only standard filename/subfolder/type descriptors are downloadable; a remote
    URL or absolute path from a custom node is never followed as a substitute.
    """
    files = []
    seen = set()
    if selected is not None and any(node not in outputs for node in selected):
        raise ComfyError("missing_output", "A requested output node is absent from completed history")
    for node_id, values in outputs.items():
        if selected is not None and node_id not in selected:
            continue
        stack = [values]
        while stack:
            item = stack.pop()
            if isinstance(item, dict):
                if "filename" in item:
                    remote = descriptor(item)
                    key = (remote["filename"], remote["subfolder"], remote["type"])
                    if key not in seen:
                        seen.add(key)
                        files.append((node_id, remote))
                        if len(files) > MAX_FILES:
                            raise ComfyError("too_many_files", f"More than {MAX_FILES} outputs; select output_nodes to narrow the download")
                else:
                    stack.extend(reversed(list(item.values())))
            elif isinstance(item, list):
                stack.extend(reversed(item))
    return files


def collect(client, job_id, wait_seconds=0, selected=None):
    deadline = time.monotonic() + wait_seconds
    while True:
        history = client.json("GET", "/history/" + job_id)
        if not isinstance(history, dict):
            raise ComfyError("invalid_response", "Malformed ComfyUI history")
        entry = history.get(job_id)
        if entry is not None:
            if not isinstance(entry, dict) or not isinstance(entry.get("status"), dict):
                raise ComfyError("invalid_response", "Missing history status")
            status = entry["status"]
            messages = status.get("messages", [])
            if not isinstance(messages, list):
                raise ComfyError("invalid_response", "Malformed history events")
            events = {message[0] for message in messages if isinstance(message, list) and message and isinstance(message[0], str)}
            if status.get("status_str") == "error" or events & {"execution_error", "execution_interrupted"}:
                raise ComfyError("execution_error", "ComfyUI execution failed or was interrupted; inspect the server's history. No resubmission was made.")
            if status.get("status_str") != "success" or type(status.get("completed")) is not bool:
                raise ComfyError("invalid_response", "Unknown/malformed history status")
            if status["completed"]:
                outputs = entry.get("outputs")
                if not isinstance(outputs, dict):
                    raise ComfyError("invalid_response", "Completed history is missing outputs")
                # Validate the entire descriptor batch before downloading/publishing.
                descriptors = output_files(outputs, selected)
                with OutputBatch() as batch:
                    files = []
                    total = 0
                    for node_id, remote in descriptors:
                        limit = min(client.file_limit, client.file_limit * 4 - total)
                        if limit <= 0:
                            raise ComfyError("response_too_large", "Total output exceeds four times the configured per-file byte limit")
                        data = client.request("GET", "/view", query=remote, limit=limit)
                        total += len(data)
                        files.append({**batch.save(data, remote["filename"]), "node_id": node_id, "remote": remote})
                    chosen = {node: value for node, value in outputs.items() if selected is None or node in selected}
                    small = len(encode(chosen)) <= 32768
                    return {"prompt_id": job_id, "status": "completed", "execution_verified": True,
                            "files": files, "outputs": chosen if small else None, "outputs_omitted": not small,
                            "outputs_file": None if small else batch.save(encode(chosen), "outputs.json")}
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return {"prompt_id": job_id, "status": "pending", "files": [],
                    "note": "Queued/running or not in retained history. Call comfyui_result with this same prompt_id; do not submit again."}
        time.sleep(min(1, remaining))


def selection(args):
    selected = args.get("output_nodes")
    if selected is not None:
        if not isinstance(selected, list) or not 1 <= len(selected) <= 64:
            raise ComfyError("invalid_arguments", "output_nodes must contain 1..64 node IDs")
        for value in selected:
            text(value, "output node ID", 128)
    return selected


def execute(args, client):
    fields(args, {"prompt_id", "wait_seconds", "output_nodes"}, {"prompt_id"})
    job_id = prompt_id(args["prompt_id"])
    wait = number(args.get("wait_seconds", 0), "wait_seconds", 0, 900)
    selected = selection(args)
    try:
        return collect(client, job_id, wait, selected)
    except ComfyError as error:
        error.result["prompt_id"] = job_id
        raise
    except OSError:
        raise ComfyError("storage", "Cannot save output files; the existing job can be fetched again with comfyui_result", prompt_id=job_id) from None
    except Exception:
        raise ComfyError("invalid_response", "Malformed ComfyUI history/output; inspect the existing job", prompt_id=job_id) from None


if __name__ == "__main__":
    sys.exit(main(execute))
