# Media in Praxis mnemodim workflows

This port uses **Praxis** tools, not Pi's Codex/Kokoro tools. Loading this reference
never generates media. Check the currently enabled tools; keys and voice IDs must
already be configured through the user's existing secret/context controls. Never
read or print credentials to troubleshoot.

## Existing assets

Use the bundled helper with a **new output directory**:

```bash
python3 /absolute/skill_dir/scripts/mnemodim_tool.py extract-assets INPUT.mnemodim NEW_DIRECTORY
```

It validates archive paths, entry sizes, IDs and basic package references before
extracting. It refuses existing output directories and cleans up a failed batch.
Use actual image-understanding/viewing capabilities before choosing visual locus
anchors. Merely extracting an image does not mean you have seen it.

## Explicit image generation

Only when the user clearly requests generation, call enabled
`openrouter_image_generate` with a prompt describing the subject, composition,
style and purpose (stage background versus recall/object image). Ask about
essential missing style/route requirements. Supported options depend on the
chosen model; HTTPS reference images may be used only when appropriate and
sharing those references is authorized.

The tool returns saved local paths and image metadata. Do not download
unspecified remote response URLs or store Base64 in conversation history.
Inspect output before using it as a spatial map. It is a paid single-request tool
with no automatic retry/fallback: an explicit new invocation may incur another
charge. Do not blindly retry failures.

## Explicit speech/audio

Only when the user requests narration, pronunciation or audio, call enabled
`elevenlabs_tts` with the actual text in the requested language. A configured voice
ID is required; do not choose another person's voice, clone voices or silently
change providers. Select a compatible model explicitly if needed.

The plugin defaults to `eleven_multilingual_v2`. That model does not support the
optional `language_code` argument: omit it, or explicitly choose a supporting
model such as `eleven_flash_v2_5`. The tool does not translate; translate first
only when the user requests that. It returns a local MP3. No autoplay is implied.

## Embed and validate

Apply `MNEMODIM_IMPORT_GUIDE.md` sections 6–8 before hand-off, including MIME
signatures, sizes and ZIP integrity. A helper structural PASS does not verify
media signatures/decodability or successful backend uploads.

Read the returned local file bytes, assign a unique stable asset ID and add its
MIME, ZIP path `assets/<id>` and exact size to `manifest.assets`. Reference that ID
from `stage.imagePath`, `locus.imagePath`, `locus.soundPath`, or an asset table
cell. Do not confuse stage and recall imagery. Respect 20 MiB per asset, 200
assets and the total package limits. SVG must be rasterized before import.

Plugin downloads are temporary source material, not already embedded palace
assets. Repack and validate the new package and all references before reporting
completion. More provider details: the installation's `docs/MEDIA_PLUGINS.md`.
