#!/usr/bin/env python3
import json
import os
import subprocess

args = json.loads(os.environ.get("PLUGIN_ARGS", "{}"))
filter_name = args.get("filter", "")

result = subprocess.run(["ps", "aux", "--no-headers"], capture_output=True, text=True)
processes = []
for line in result.stdout.strip().split("\n"):
    parts = line.split(None, 10)
    if len(parts) >= 11:
        proc = {
            "user": parts[0],
            "pid": int(parts[1]),
            "cpu": float(parts[2]),
            "mem": float(parts[3]),
            "command": parts[10],
        }
        if not filter_name or filter_name.lower() in parts[10].lower():
            processes.append(proc)

processes.sort(key=lambda p: p["cpu"], reverse=True)

print(json.dumps({"processes": processes[:50], "total_matching": len(processes)}, indent=2))
