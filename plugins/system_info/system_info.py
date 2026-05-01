#!/usr/bin/env python3
import json
import os
import platform
import shutil

args = json.loads(os.environ.get("PLUGIN_ARGS", "{}"))

info = {
    "hostname": platform.node(),
    "os": platform.system(),
    "os_version": platform.version(),
    "architecture": platform.machine(),
    "python_version": platform.python_version(),
}

total, used, free = shutil.disk_usage("/")
info["disk"] = {
    "total_gb": round(total / (1024**3), 1),
    "used_gb": round(used / (1024**3), 1),
    "free_gb": round(free / (1024**3), 1),
}

try:
    with open("/proc/meminfo") as f:
        mem = {}
        for line in f:
            parts = line.split(":")
            if len(parts) == 2:
                key = parts[0].strip()
                val = parts[1].strip().split()[0]
                mem[key] = int(val)
        if "MemTotal" in mem and "MemAvailable" in mem:
            info["memory"] = {
                "total_mb": round(mem["MemTotal"] / 1024, 1),
                "available_mb": round(mem["MemAvailable"] / 1024, 1),
                "used_mb": round((mem["MemTotal"] - mem["MemAvailable"]) / 1024, 1),
            }
except FileNotFoundError:
    pass

try:
    with open("/proc/uptime") as f:
        uptime_secs = float(f.read().split()[0])
        info["uptime_hours"] = round(uptime_secs / 3600, 1)
except FileNotFoundError:
    pass

print(json.dumps(info, indent=2))
