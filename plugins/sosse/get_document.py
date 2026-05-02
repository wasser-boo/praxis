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

def make_request(url, username, password, api_key, method="GET"):
    headers = {"Content-Type": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    elif username and password:
        creds = base64.b64encode(f"{username}:{password}".encode()).decode()
        headers["Authorization"] = f"Basic {creds}"

    req = urllib.request.Request(url, headers=headers, method=method)
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

    doc_id = args.get("document_id")
    if doc_id is None:
        print(json.dumps({"error": "document_id parameter is required"}))
        sys.exit(1)

    url = f"{sosse_url}/api/document/{doc_id}/"
    doc = make_request(url, username, password, api_key)

    if "error" in doc:
        print(json.dumps(doc))
        sys.exit(1)

    output = {
        "id": doc.get("id"),
        "url": doc.get("url", ""),
        "title": doc.get("title", ""),
        "mimetype": doc.get("mimetype"),
        "crawl_first": doc.get("crawl_first"),
        "crawl_last": doc.get("crawl_last"),
        "has_html_snapshot": doc.get("has_html_snapshot", False),
        "has_thumbnail": doc.get("has_thumbnail", False),
        "screenshot_count": doc.get("screenshot_count", 0),
        "tags": doc.get("tags_str", ""),
        "hidden": doc.get("hidden", False),
        "error": doc.get("error", ""),
    }

    content = doc.get("content", "")
    if content:
        if len(content) > 5000:
            output["content"] = content[:5000] + f"\n\n[... truncated, {len(content)} chars total]"
        else:
            output["content"] = content

    print(json.dumps(output, indent=2))

if __name__ == "__main__":
    main()
