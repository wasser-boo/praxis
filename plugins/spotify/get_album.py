#!/usr/bin/env python3
import json
import sys
from spotify_common import get_config, get_access_token, api_request, extract_id

def main():
    args, client_id, client_secret = get_config()
    token = get_access_token(client_id, client_secret)

    album_id = args.get("album_id", "")
    if not album_id:
        print(json.dumps({"error": "album_id parameter is required"}))
        sys.exit(1)

    album_id = extract_id(album_id)
    album = api_request(f"/albums/{album_id}", token)

    if "error" in album:
        print(json.dumps(album))
        sys.exit(1)

    tracks = []
    for t in album.get("tracks", {}).get("items", []):
        tracks.append({
            "id": t.get("id"),
            "name": t.get("name", ""),
            "artists": [a.get("name", "") for a in t.get("artists", [])],
            "track_number": t.get("track_number"),
            "duration_ms": t.get("duration_ms"),
            "preview_url": t.get("preview_url"),
            "external_url": t.get("external_urls", {}).get("spotify", "")
        })

    output = {
        "id": album.get("id"),
        "name": album.get("name", ""),
        "artists": [a.get("name", "") for a in album.get("artists", [])],
        "album_type": album.get("album_type"),
        "release_date": album.get("release_date"),
        "total_tracks": album.get("total_tracks"),
        "genres": album.get("genres", []),
        "popularity": album.get("popularity"),
        "label": album.get("label", ""),
        "images": album.get("images", []),
        "external_url": album.get("external_urls", {}).get("spotify", ""),
        "tracks": tracks
    }

    print(json.dumps(output, indent=2))

if __name__ == "__main__":
    main()
