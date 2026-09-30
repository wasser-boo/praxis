#!/usr/bin/env python3
"""Praxis tool: exact text -> ElevenLabs -> local MP3. No autoplay or retries."""
import math
import re
import sys
import urllib.parse
from media_common import MediaError, OutputBatch, api_key, base_url, inputs, main, post, setting, text, timeout_seconds


def generate():
    args, context, secrets = inputs()
    allowed = {"text", "voice_id", "model_id", "language_code", "stability", "similarity_boost", "style", "speed", "use_speaker_boost"}
    if set(args) - allowed:
        raise MediaError("invalid_arguments", "Unexpected ElevenLabs tool arguments")
    speech = text(args.get("text"), "text", 5000)
    voice = text(setting(args, context, "voice_id", "elevenlabs_tts_voice_id", "ELEVENLABS_VOICE_ID"), "voice_id")
    model = text(setting(args, context, "model_id", "elevenlabs_tts_model_id", "ELEVENLABS_TTS_MODEL", "eleven_multilingual_v2"), "model_id")
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", voice):
        raise MediaError("invalid_arguments", "voice_id must be an ElevenLabs voice identifier, not a path or URL")
    body = {"text": speech, "model_id": model}
    if "language_code" in args:
        language = text(args["language_code"], "language_code", 2)
        if not re.fullmatch(r"[a-z]{2}", language):
            raise MediaError("invalid_arguments", "language_code must be a lowercase ISO 639-1 code")
        if model == "eleven_multilingual_v2":
            raise MediaError("invalid_arguments", "eleven_multilingual_v2 does not support language_code; omit it for automatic language or choose a supported model")
        body["language_code"] = language
    voice_settings = {}
    for name, low, high in [("stability", 0, 1), ("similarity_boost", 0, 1), ("style", 0, 1), ("speed", 0.7, 1.2)]:
        if name in args:
            value = args[name]
            if type(value) not in (int, float) or not math.isfinite(value) or not low <= value <= high:
                raise MediaError("invalid_arguments", name + " is outside its supported numeric range")
            voice_settings[name] = value
    if "use_speaker_boost" in args:
        if not isinstance(args["use_speaker_boost"], bool):
            raise MediaError("invalid_arguments", "use_speaker_boost must be a boolean")
        voice_settings["use_speaker_boost"] = args["use_speaker_boost"]
    if voice_settings:
        body["voice_settings"] = voice_settings
    key = api_key(secrets, "elevenlabs_api_key", "ELEVENLABS_API_KEY")
    base = base_url("ELEVENLABS_API_BASE", "https://api.elevenlabs.io")
    timeout = timeout_seconds("ELEVENLABS_TTS_TIMEOUT_SECONDS", 120)
    url = base + "/v1/text-to-speech/" + urllib.parse.quote(voice, safe="") + "?output_format=mp3_44100_128"
    with OutputBatch() as output:
        audio = post(url, {"xi-api-key": key, "Accept": "audio/mpeg"}, body, timeout, 16 * 1024 * 1024)
        # Accept ID3-tagged MP3 or an MPEG frame, never a JSON/HTML error page.
        is_mp3 = len(audio) >= 10 and (audio.startswith(b"ID3") or (audio[0] == 255 and audio[1] & 0xE0 == 0xE0))
        if not is_mp3:
            raise MediaError("invalid_response", "ElevenLabs did not return non-empty MP3 audio")
        file = output.save([(audio, ".mp3", "audio/mpeg")], "elevenlabs")[0]
    return {"provider": "elevenlabs", "model": model, "voice_id": voice, "characters": len(speech), **file}


if __name__ == "__main__":
    sys.exit(main(generate))
