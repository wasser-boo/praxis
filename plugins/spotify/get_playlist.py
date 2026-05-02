#!/usr/bin/env python3
import json
import sys
from spotify_common import get_config, get_access_token, api_request, extract_id

def main():
    args, client_id, client_secret = get_config()
    token = get_access_token(client_id, client_secret)

    playlist_id = args.get("playlist_id", "")
    if not playlist_id:
        print(json.dumps({"error": "playlist_id parameter is required"}))
        sys.exit(1)

    playlist_id = extract_id(playlist_id)
    playlist = api_request(f"/playlists/{playlist_id}", token)

    if "error" in playlist:
        print(json.dumps(playlist))
        sys.exit(1)

    tracks = []
    for item in playlist.get("tracks", {}).get("items", [])[:50]:
        track = item.get("track", {})
        if not track:
            continue
        tracks.append({
            "id": track.get("id"),
            "name": track.get("name", ""),
            "artists": [a.get("name", "") for a in track.get("artists", [])],
            "album": track.get("album", {}).get("name", ""),
            "duration_ms": track.get("duration_ms"),
            "added_at": item.get("added_at"),
            "preview_url": track.get("preview_url"),
            "external_url": track.get("external_urls", {}).get("spotify", "")
        })

    owner = playlist.get("owner", {})
    output = {
        "id": playlist.get("id"),
        "name": playlist.get("name", ""),
        "description": playlist.get("description", ""),
        "owner": owner.get("display_name", ""),
        "followers": playlist.get("followers", {}).get("total"),
        "tracks_total": playlist.get("tracks", {}).get("total", 0),
        "public": playlist.get("public"),
        "collaborative": playlist.get("collaborative"),
        "images": playlist.get("images", []),
        "external_url": playlist.get("external_urls", {}).get("spotify", ""),
        "tracks": tracks
    }

    print(json.dumps(output, indent=2))

if __name__ == "__main__":
    main()
