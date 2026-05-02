#!/usr/bin/env python3
import json
import sys
from spotify_common import get_config, get_access_token, api_request

def main():
    args, client_id, client_secret = get_config()
    token = get_access_token(client_id, client_secret)

    query = args.get("query", "")
    if not query:
        print(json.dumps({"error": "query parameter is required"}))
        sys.exit(1)

    search_type = args.get("type", "track")
    limit = min(args.get("limit", 10), 50)

    result = api_request("/search", token, {
        "q": query,
        "type": search_type,
        "limit": limit
    })

    if "error" in result:
        print(json.dumps(result))
        sys.exit(1)

    output = {"type": search_type, "results": []}

    key = f"{search_type}s"
    items = result.get(key, {}).get("items", [])

    for item in items:
        entry = {"id": item.get("id"), "name": item.get("name", "")}

        if search_type == "track":
            entry["artists"] = [a.get("name", "") for a in item.get("artists", [])]
            entry["album"] = item.get("album", {}).get("name", "")
            entry["duration_ms"] = item.get("duration_ms")
            entry["popularity"] = item.get("popularity")
            entry["preview_url"] = item.get("preview_url")
            entry["external_url"] = item.get("external_urls", {}).get("spotify", "")

        elif search_type == "album":
            entry["artists"] = [a.get("name", "") for a in item.get("artists", [])]
            entry["release_date"] = item.get("release_date")
            entry["total_tracks"] = item.get("total_tracks")
            entry["album_type"] = item.get("album_type")
            entry["external_url"] = item.get("external_urls", {}).get("spotify", "")

        elif search_type == "artist":
            entry["genres"] = item.get("genres", [])
            entry["popularity"] = item.get("popularity")
            entry["followers"] = item.get("followers", {}).get("total")
            entry["external_url"] = item.get("external_urls", {}).get("spotify", "")

        elif search_type == "playlist":
            entry["owner"] = item.get("owner", {}).get("display_name", "")
            entry["tracks_total"] = item.get("tracks", {}).get("total", 0)
            entry["description"] = item.get("description", "")
            entry["external_url"] = item.get("external_urls", {}).get("spotify", "")

        output["results"].append(entry)

    print(json.dumps(output, indent=2))

if __name__ == "__main__":
    main()
