#!/usr/bin/env python3
import json
import os
import sys
import urllib.request
import urllib.error
import base64

SPOTIFY_TOKEN_URL = "https://accounts.spotify.com/api/token"
SPOTIFY_API_BASE = "https://api.spotify.com/v1"

def get_config():
    args = json.loads(os.environ.get("PLUGIN_ARGS", "{}"))

    sec = {}
    sec_raw = os.environ.get("PLUGIN_SECRETS", "")
    if sec_raw:
        try:
            sec = json.loads(sec_raw)
        except json.JSONDecodeError:
            pass

    client_id = sec.get("spotify_client_id", "")
    client_secret = sec.get("spotify_client_secret", "")

    if not client_id or not client_secret:
        print(json.dumps({"error": "spotify_client_id and spotify_client_secret must be set in secrets"}))
        sys.exit(1)

    return args, client_id, client_secret

def get_access_token(client_id, client_secret):
    creds = base64.b64encode(f"{client_id}:{client_secret}".encode()).decode()
    data = "grant_type=client_credentials".encode()
    headers = {
        "Authorization": f"Basic {creds}",
        "Content-Type": "application/x-www-form-urlencoded"
    }
    req = urllib.request.Request(SPOTIFY_TOKEN_URL, data=data, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            result = json.loads(resp.read().decode())
            return result.get("access_token")
    except urllib.error.HTTPError as e:
        body = e.read().decode() if e.fp else ""
        print(json.dumps({"error": f"Auth failed HTTP {e.code}: {body}"}))
        sys.exit(1)
    except Exception as e:
        print(json.dumps({"error": f"Auth failed: {str(e)}"}))
        sys.exit(1)

def api_request(endpoint, access_token, params=None):
    url = f"{SPOTIFY_API_BASE}{endpoint}"
    if params:
        query = "&".join(f"{k}={v}" for k, v in params.items() if v is not None)
        if query:
            url = f"{url}?{query}"

    headers = {"Authorization": f"Bearer {access_token}"}
    req = urllib.request.Request(url, headers=headers, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        body = e.read().decode() if e.fp else ""
        return {"error": f"HTTP {e.code}: {body}"}
    except Exception as e:
        return {"error": str(e)}

def extract_id(id_or_uri):
    if ":" in id_or_uri:
        return id_or_uri.split(":")[-1]
    if "/" in id_or_uri:
        return id_or_uri.rstrip("/").split("/")[-1]
    return id_or_uri
