#!/usr/bin/env python3
import json
import sys
from spotify_common import get_config, get_access_token, api_request, extract_id

def main():
    args, client_id, client_secret = get_config()
    token = get_access_token(client_id, client_secret)

    track_id = args.get("track_id", "")
    if not track_id:
        print(json.dumps({"error": "track_id parameter is required"}))
        sys.exit(1)

    track_id = extract_id(track_id)
    track = api_request(f"/tracks/{track_id}", token)

    if "error" in track:
        print(json.dumps(track))
        sys.exit(1)

    album = track.get("album", {})
    output = {
        "id": track.get("id"),
        "name": track.get("name", ""),
        "artists": [a.get("name", "") for a in track.get("artists", [])],
        "album": {
            "id": album.get("id"),
            "name": album.get("name", ""),
            "release_date": album.get("release_date"),
            "total_tracks": album.get("total_tracks"),
            "images": album.get("images", [])
        },
        "duration_ms": track.get("duration_ms"),
        "popularity": track.get("popularity"),
        "preview_url": track.get("preview_url"),
        "explicit": track.get("explicit"),
        "track_number": track.get("track_number"),
        "disc_number": track.get("disc_number"),
        "external_url": track.get("external_urls", {}).get("spotify", ""),
        "available_markets_count": len(track.get("available_markets", []))
    }

    print(json.dumps(output, indent=2))

if __name__ == "__main__":
    main()
