#!/usr/bin/env python3
import json
import sys
from spotify_common import get_config, get_access_token, api_request, extract_id

def main():
    args, client_id, client_secret = get_config()
    token = get_access_token(client_id, client_secret)

    seed_artists = args.get("seed_artists", "")
    seed_tracks = args.get("seed_tracks", "")
    seed_genres = args.get("seed_genres", "")
    limit = min(args.get("limit", 10), 100)

    if not seed_artists and not seed_tracks and not seed_genres:
        print(json.dumps({"error": "At least one seed (seed_artists, seed_tracks, or seed_genres) is required"}))
        sys.exit(1)

    params = {"limit": str(limit)}

    if seed_artists:
        ids = ",".join(extract_id(a.strip()) for a in seed_artists.split(","))
        params["seed_artists"] = ids

    if seed_tracks:
        ids = ",".join(extract_id(t.strip()) for t in seed_tracks.split(","))
        params["seed_tracks"] = ids

    if seed_genres:
        params["seed_genres"] = seed_genres

    result = api_request("/recommendations", token, params)

    if "error" in result:
        print(json.dumps(result))
        sys.exit(1)

    tracks = []
    for t in result.get("tracks", []):
        album = t.get("album", {})
        tracks.append({
            "id": t.get("id"),
            "name": t.get("name", ""),
            "artists": [a.get("name", "") for a in t.get("artists", [])],
            "album": album.get("name", ""),
            "release_date": album.get("release_date"),
            "duration_ms": t.get("duration_ms"),
            "popularity": t.get("popularity"),
            "preview_url": t.get("preview_url"),
            "external_url": t.get("external_urls", {}).get("spotify", "")
        })

    seeds = result.get("seeds", [])
    output = {
        "seeds": [{"id": s.get("id"), "type": s.get("type"), "initialPoolSize": s.get("initialPoolSize")} for s in seeds],
        "tracks": tracks
    }

    print(json.dumps(output, indent=2))

if __name__ == "__main__":
    main()
