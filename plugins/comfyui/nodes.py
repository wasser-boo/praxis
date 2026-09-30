#!/usr/bin/env python3
"""Discover the actual node classes, connections, widgets and models on this server."""
import sys
import urllib.parse
from common import ComfyError, fields, main, number, text


def execute(args, client):
    fields(args, {"node_class", "query", "limit", "offset"})
    if "node_class" in args:
        if len(args) != 1:
            raise ComfyError("invalid_arguments", "node_class cannot be combined with catalog filters")
        node = text(args["node_class"], "node_class", 256)
        nodes = client.json("GET", "/object_info/" + urllib.parse.quote(node, safe=""))
        if not isinstance(nodes, dict) or node not in nodes:
            raise ComfyError("node_not_found", "This node class is not installed on the configured ComfyUI server")
        return {"nodes": nodes}
    query = args.get("query", "")
    if not isinstance(query, str) or len(query) > 256:
        raise ComfyError("invalid_arguments", "query must be a string up to 256 characters")
    limit = number(args.get("limit", 20), "limit", 1, 100, integer=True)
    offset = number(args.get("offset", 0), "offset", 0, 1000000, integer=True)
    nodes = client.json("GET", "/object_info")
    if not isinstance(nodes, dict):
        raise ComfyError("invalid_response", "Malformed node catalog")
    matches = []
    for name, node in sorted(nodes.items()):
        if not isinstance(node, dict):
            continue
        if query.lower() not in (name + " " + str(node.get("display_name", "")) + " " + str(node.get("category", ""))).lower():
            continue
        matches.append({"class_type": name, "display_name": node.get("display_name", name),
                        "category": node.get("category", ""), "output_node": node.get("output_node", False)})
    return {"nodes": matches[offset:offset + limit], "next_offset": offset + limit if offset + limit < len(matches) else None,
            "note": "Use node_class for full required/optional inputs, output slots and current model/file choices. Never invent unavailable nodes or models."}


if __name__ == "__main__":
    sys.exit(main(execute))
