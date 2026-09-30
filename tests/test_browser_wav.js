// Pure JS / mocked Web Audio tests: no browser, recording, network or speech API.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
let closed = 0;
let duration = 0.2;
class DecodeContext {
  async decodeAudioData() { return {duration, numberOfChannels: 1}; }
  async close() { closed++; }
}
class OfflineContext {
  constructor(channels, frames, rate) { assert.equal(channels, 1); assert.equal(rate, 16000); this.frames = frames; this.destination = {}; }
  createBufferSource() { return {connect() {}, start() {}}; }
  async startRendering() { return {getChannelData: () => new Float32Array(this.frames)}; }
}
const sandbox = {Blob, Uint8Array, Float32Array, DataView, AudioContext: DecodeContext, OfflineAudioContext: OfflineContext};
vm.createContext(sandbox);
vm.runInContext(fs.readFileSync(__dirname + '/../static/chat-audio.js', 'utf8'), sandbox);
(async () => {
  const blob = sandbox.chatEncodeMonoWav(new Float32Array([-1, 0, 1]), 16000);
  const bytes = await blob.arrayBuffer(); const view = new DataView(bytes);
  assert.equal(Buffer.from(bytes).toString('ascii', 0, 4), 'RIFF');
  assert.equal(Buffer.from(bytes).toString('ascii', 8, 12), 'WAVE');
  assert.equal(view.getUint16(22, true), 1); assert.equal(view.getUint32(24, true), 16000);
  assert.equal(view.getInt16(44, true), -32768); assert.equal(view.getInt16(46, true), 0); assert.equal(view.getInt16(48, true), 32767);
  for (const samples of [new Float32Array(), new Float32Array([NaN]), new Float32Array([Infinity])]) {
    assert.throws(() => sandbox.chatEncodeMonoWav(samples, 16000));
  }
  const original = new Blob(['synthetic fixture'], {type: 'audio/webm'});
  const unchanged = await sandbox.chatPrepareSttAudio(original, {voice_stt_type: 'elevenlabs'});
  assert.equal(unchanged.blob, original); assert.equal(closed, 0);
  const prepared = await sandbox.chatPrepareSttAudio(original, {voice_stt_type: 'vosk'});
  assert.equal(prepared.filename, 'speech.wav'); assert.equal(prepared.blob.type, 'audio/wav');
  assert.equal(prepared.blob.size, 44 + 3200 * 2); assert.equal(closed, 1);
  duration = 60.02; // normal timer/codec tail is trimmed to fit the dashboard limit
  const longest = await sandbox.chatPrepareSttAudio(original, {voice_stt_type: 'vosk'});
  assert.equal(longest.blob.size, 44 + 60 * 16000 * 2);
  assert.ok(longest.blob.size + 65536 < 2 * 1024 * 1024);
  duration = 62;
  await assert.rejects(sandbox.chatPrepareSttAudio(original, {voice_stt_type: 'vosk'}));
  assert.equal(closed, 3);
  console.log('PASS: PCM WAV headers/sign, invalid samples, Vosk conversion/cleanup, 60s upload bound, other-provider preservation');
})().catch(e => { console.error(e); process.exitCode = 1; });
