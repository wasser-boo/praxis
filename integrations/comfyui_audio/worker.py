"""One process/model load per complete reply, not per language segment.

Invoked only by the trusted ComfyUI node, via JSON stdin. Its model allocations
are released when it exits. Qwen is offline-only and uses an isolated venv.
"""
import json
import os
from pathlib import Path
import sys

if __package__:
    from .audio import write_speech
    from .segments import speech_segments
else:
    from audio import write_speech
    from segments import speech_segments

QWEN_MODEL_DIR = '/workspace/qwen3-tts/1.7B-Base'
QWEN_LANGUAGES = {'auto': 'Auto', 'en': 'English', 'zh': 'Chinese', 'ja': 'Japanese',
                  'ko': 'Korean', 'de': 'German', 'fr': 'French', 'ru': 'Russian',
                  'pt': 'Portuguese', 'es': 'Spanish', 'it': 'Italian'}


def xtts(request):
    if os.environ.get('COQUI_TOS_AGREED') != '1':
        raise RuntimeError('Explicit XTTS license acceptance required')
    # Mixed work is validated before a model load. Fixed-language XTTS retains
    # its previous tts_to_file path, including its internal sentence handling.
    mixed = request['language'] == 'de-ja'
    segments = speech_segments(request['text'], 'de-ja') if mixed else None
    import torch
    from TTS.api import TTS
    if not torch.cuda.is_available():
        raise RuntimeError('XTTS requires CUDA')
    model = TTS('tts_models/multilingual/multi-dataset/xtts_v2').to('cuda')
    if not mixed:
        model.tts_to_file(text=request['text'], language=request['language'],
                          speaker_wav=request['reference'], file_path=request['output'])
        return
    rate = int(model.synthesizer.output_sample_rate)
    def generate(language, text):
        return model.tts(text=text, language=language, speaker_wav=request['reference']), rate
    write_speech(request['output'], segments, generate)


def qwen3(request):
    segments = speech_segments(request['text'], request['language'])
    directory = Path(os.environ.get('PRAXIS_QWEN_TTS_MODEL_DIR', QWEN_MODEL_DIR))
    required = ['config.json', 'model.safetensors', 'speech_tokenizer/config.json',
                'speech_tokenizer/model.safetensors', 'tokenizer_config.json', 'vocab.json', 'merges.txt']
    if not directory.is_absolute() or not all((directory / name).is_file() for name in required):
        raise RuntimeError('Install the pinned Qwen3-TTS Base model locally before synthesis')
    # Do not let inference lazily fetch weights/processors or send telemetry.
    os.environ.update({'HF_HUB_OFFLINE': '1', 'TRANSFORMERS_OFFLINE': '1', 'HF_HUB_DISABLE_TELEMETRY': '1'})
    import soundfile as sf
    info = sf.info(request['reference'])
    if info.channels not in (1, 2) or info.samplerate <= 0 or not 0 < info.frames / info.samplerate <= 120:
        raise ValueError('Use a non-empty mono/stereo reference of at most 120 seconds')
    import torch
    from qwen_tts import Qwen3TTSModel
    if not torch.cuda.is_available():
        raise RuntimeError('Qwen3-TTS requires CUDA')
    model = Qwen3TTSModel.from_pretrained(str(directory), device_map='cuda:0',
                                         dtype=torch.bfloat16, attn_implementation='sdpa',
                                         local_files_only=True, trust_remote_code=False)
    reference_text = request.get('reference_text', '').strip()
    prompt = model.create_voice_clone_prompt(ref_audio=request['reference'],
                                             ref_text=reference_text or None,
                                             x_vector_only_mode=not bool(reference_text))
    def generate(language, text):
        wavs, rate = model.generate_voice_clone(text=text, language=QWEN_LANGUAGES[language],
                                               voice_clone_prompt=prompt, max_new_tokens=2048)
        if len(wavs) != 1:
            raise ValueError('Expected one waveform per segment')
        return wavs[0], int(rate)
    write_speech(request['output'], segments, generate)


def main():
    raw = sys.stdin.read(65537)
    if len(raw) > 65536:
        raise ValueError('Worker request exceeds limit')
    request = json.loads(raw)
    if request.get('backend') == 'xtts':
        xtts(request)
    elif request.get('backend') == 'qwen3':
        qwen3(request)
    else:
        raise ValueError('Unknown speech backend')


if __name__ == '__main__':
    main()
