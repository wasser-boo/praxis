#!/usr/bin/env node
// Real Chromium MediaRecorder + Web Audio codec/resampling checks.
// Synthetic oscillator/stereo WAV only; no microphone, live API, credentials or TTS.
const assert = require('node:assert/strict');
const path = require('node:path');
let chromium;
for (const mod of ['playwright', '/tmp/praxis-ui-browser/node_modules/playwright']) {
    try { ({chromium} = require(mod)); break; } catch (_) {}
}
assert(chromium, 'Install Playwright under /tmp/praxis-ui-browser');
(async () => {
    const browser = await chromium.launch({headless: true, args: ['--autoplay-policy=no-user-gesture-required']});
    try {
        const page = await browser.newPage();
        await page.addScriptTag({path: path.resolve(__dirname, '../static/chat-audio.js')});
        const result = await page.evaluate(async () => {
            const context = new AudioContext();
            const destination = context.createMediaStreamDestination();
            const oscillator = context.createOscillator();
            const gain = context.createGain(); gain.gain.value = 0.05;
            oscillator.connect(gain); gain.connect(destination);
            const recorder = new MediaRecorder(destination.stream);
            const chunks = [];
            recorder.ondataavailable = event => { if (event.data.size) chunks.push(event.data); };
            const finished = new Promise(resolve => {recorder.onstop = resolve;});
            let original;
            try {
                await context.resume(); oscillator.start(); recorder.start();
                await new Promise(resolve => setTimeout(resolve, 350));
                recorder.stop(); await finished;
                original = new Blob(chunks, {type: recorder.mimeType});
            } finally {
                oscillator.stop(); destination.stream.getTracks().forEach(track => track.stop());
                await context.close();
            }
            const prepared = await chatPrepareSttAudio(original, {voice_stt_type: 'vosk'});
            const buffer = await prepared.blob.arrayBuffer();
            const view = new DataView(buffer);
            let nonzero = 0;
            for (let i = 44; i < buffer.byteLength; i += 2) if (view.getInt16(i, true)) nonzero++;
            const other = await chatPrepareSttAudio(original, {voice_stt_type: 'elevenlabs'});
            // Known stereo DC signal tests actual downmix + 48kHz -> 16kHz resampling.
            const samples = new Int16Array(4800 * 2);
            for (let i = 0; i < samples.length; i += 2) {samples[i] = -10000; samples[i + 1] = 20000;}
            const stereo = new Uint8Array(44 + samples.byteLength);
            const header = new DataView(stereo.buffer);
            for (const [offset, text] of [[0,'RIFF'],[8,'WAVE'],[12,'fmt '],[36,'data']]) for (let i=0;i<text.length;i++) stereo[offset+i]=text.charCodeAt(i);
            header.setUint32(4, stereo.length-8, true);header.setUint32(16,16,true);header.setUint16(20,1,true);header.setUint16(22,2,true);
            header.setUint32(24,48000,true);header.setUint32(28,192000,true);header.setUint16(32,4,true);header.setUint16(34,16,true);header.setUint32(40,samples.byteLength,true);
            stereo.set(new Uint8Array(samples.buffer),44);
            const mono = await chatPrepareSttAudio(new Blob([stereo],{type:'audio/wav'}), {voice_stt_type:'vosk'});
            const monoView = new DataView(await mono.blob.arrayBuffer());
            return {sourceMime: original.type, mime: prepared.blob.type, filename: prepared.filename,
                channels: view.getUint16(22,true), rate: view.getUint32(24,true), bits: view.getUint16(34,true),
                bytes: buffer.byteLength, nonzero, otherUnchanged: other.blob === original,
                downmixSample: monoView.getInt16(44 + 800 * 2,true), downmixBytes: mono.blob.size};
        });
        assert.match(result.sourceMime, /webm|ogg|mp4/);
        assert.equal(result.mime, 'audio/wav'); assert.equal(result.filename, 'speech.wav');
        assert.equal(result.channels,1); assert.equal(result.rate,16000); assert.equal(result.bits,16);
        assert.ok(result.bytes > 44 && result.nonzero > 0 && result.bytes < 2 * 1024 * 1024);
        assert.equal(result.otherUnchanged,true);
        assert.ok(Math.abs(result.downmixSample - 5000) < 10); assert.equal(result.downmixBytes, 44+1600*2);
        console.log('PASS: real MediaRecorder codec -> Vosk PCM16 WAV, nonzero audio, stereo downmix/resampling, other-provider preservation');
    } finally {await browser.close();}
})().catch(error => {console.error(error);process.exitCode=1;});
