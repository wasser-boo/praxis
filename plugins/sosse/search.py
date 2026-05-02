#!/usr/bin/env python3
import json
import os
import sys
import urllib.request
import urllib.error
import base64

def get_config():
    args = json.loads(os.environ.get("PLUGIN_ARGS", "{}"))

    ctx = {}
    ctx_raw = os.environ.get("PLUGIN_CONTEXT", "")
    if ctx_raw:
        try:
            ctx = json.loads(ctx_raw)
        except json.JSONDecodeError:
            pass

    sec = {}
    sec_raw = os.environ.get("PLUGIN_SECRETS", "")
    if sec_raw:
        try:
            sec = json.loads(sec_raw)
        except json.JSONDecodeError:
            pass

    sosse_url = ctx.get("sosse_url", "").rstrip("/")
    username = sec.get("sosse_username", "")
    password = sec.get("sosse_password", "")
    api_key = sec.get("sosse_api_key", "")
    if api_key == "CHANGE_ME":
        api_key = ""

    if not sosse_url:
        print(json.dumps({"error": "sosse_url not set. Use set_context to configure it."}))
        sys.exit(1)

    return args, sosse_url, username, password, api_key

def make_request(url, username, password, api_key, method="GET", data=None):
    headers = {"Content-Type": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    elif username and password:
        creds = base64.b64encode(f"{username}:{password}".encode()).decode()
        headers["Authorization"] = f"Basic {creds}"

    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        body = e.read().decode() if e.fp else ""
        return {"error": f"HTTP {e.code}: {body}"}
    except Exception as e:
        return {"error": str(e)}

def main():
    args, sosse_url, username, password, api_key = get_config()

    query = args.get("query", "")
    collection = args.get("collection")
    limit = args.get("limit", 20)

    if not query:
        print(json.dumps({"error": "query parameter is required"}))
        sys.exit(1)

    body = {"query": query}
    if collection is not None:
        body["collection"] = collection

    url = f"{sosse_url}/api/search/?limit={limit}"
    result = make_request(url, username, password, api_key, method="POST", data=json.dumps(body).encode())

    if "error" in result:
        print(json.dumps(result))
        sys.exit(1)

    results = result.get("results", [])
    output = {
        "total_count": result.get("count", 0),
        "results": []
    }

    for doc in results:
        entry = {
            "id": doc.get("id"),
            "url": doc.get("url", ""),
            "title": doc.get("title", ""),
        }
        content = doc.get("content", "")
        if content:
            entry["snippet"] = content[:500]
        if doc.get("mimetype"):
            entry["mimetype"] = doc["mimetype"]
        if doc.get("crawl_last"):
            entry["last_crawled"] = doc["crawl_last"]
        output["results"].append(entry)

    print(json.dumps(output, indent=2))

if __name__ == "__main__":
    main()
