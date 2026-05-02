#!/usr/bin/env python3
import json
import sys
sys.path.insert(0, sys.path[0] if sys.path else ".")
from mimo_common import get_config, chat_completion

def main():
    args, api_key = get_config()

    url = args.get("url", "")
    if not url:
        print(json.dumps({"error": "url parameter is required"}))
        sys.exit(1)

    prompt = args.get("prompt", "Describe the content of this audio in detail. Identify speech, music, sounds, and any other audio elements.")

    messages = [
        {
            "role": "system",
            "content": "You are MiMo, a multimodal AI assistant. Analyze audio carefully and provide detailed, accurate descriptions."
        },
        {
            "role": "user",
            "content": [
                {
                    "type": "input_audio",
                    "input_audio": { "data": url }
                },
                {
                    "type": "text",
                    "text": prompt
                }
            ]
        }
    ]

    result = chat_completion(api_key, messages)
    print(json.dumps({"result": result}))

if __name__ == "__main__":
    main()
