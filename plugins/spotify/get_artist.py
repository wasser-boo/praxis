#!/usr/bin/env python3
import json
import sys
from spotify_common import get_config, get_access_token, api_request, extract_id

def main():
    args, client_id, client_secret = get_config()
    token = get_access_token(client_id, client_secret)

    artist_id = args.get("artist_id", "")
    if not artist_id:
        print(json.dumps({"error": "artist_id parameter is required"}))
        sys.exit(1)

    artist_id = extract_id(artist_id)
    artist = api_request(f"/artists/{artist_id}", token)

    if "error" in artist:
        print(json.dumps(artist))
        sys.exit(1)

    top_tracks_result = api_request(f"/artists/{artist_id}/top-tracks", token, {"market": "US"})
    top_tracks = []
    for t in top_tracks_result.get("tracks", []):
        top_tracks.append({
            "id": t.get("id"),
            "name": t.get("name", ""),
            "album": t.get("album", {}).get("name", ""),
            "popularity": t.get("popularity"),
            "preview_url": t.get("preview_url"),
            "external_url": t.get("external_urls", {}).get("spotify", "")
        })

    albums_result = api_request(f"/artists/{artist_id}/albums", token, {"limit": "10", "include_groups": "album,single"})
    albums = []
    for a in albums_result.get("items", []):
        albums.append({
            "id": a.get("id"),
            "name": a.get("name", ""),
            "album_type": a.get("album_type"),
            "release_date": a.get("release_date"),
            "total_tracks": a.get("total_tracks"),
            "external_url": a.get("external_urls", {}).get("spotify", "")
        })

    output = {
        "id": artist.get("id"),
        "name": artist.get("name", ""),
        "genres": artist.get("genres", []),
        "popularity": artist.get("popularity"),
        "followers": artist.get("followers", {}).get("total"),
        "images": artist.get("images", []),
        "external_url": artist.get("external_urls", {}).get("spotify", ""),
        "top_tracks": top_tracks,
        "albums": albums
    }

    print(json.dumps(output, indent=2))

if __name__ == "__main__":
    main()
