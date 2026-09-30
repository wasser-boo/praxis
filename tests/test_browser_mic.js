// Exercise the actual microphone handler with synthetic browser interfaces.
// No device access, network, personal recording, model, or paid speech service.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(__dirname + '/../static/app.js', 'utf8');
const code = app.slice(app.indexOf('async function chatToggleMic()'), app.indexOf('// Reply playback and replay controls'));
assert.ok(code.startsWith('async function chatToggleMic()'));
function fixture({failRecorder = false, switchAfterUpload = false} = {}) {
  const button = {disabled: false, classList: {add() {}, remove() {}}};
  const input = {value: 'draft', focus() {}};
  let stopped = 0, type = 'vosk';
  const uploads = [], timerDelays = [];
  const stream = {getTracks: () => [{stop() {stopped++;}}]};
  class Recorder {
    constructor() { if (failRecorder) throw new Error('No supported recorder'); this.state = 'inactive'; this.mimeType = 'audio/ogg;codecs=opus'; }
    start() { this.state = 'recording'; }
    stop() { this.state = 'inactive'; this.ondataavailable({data: new Blob(['synthetic fixture'])}); this.stopped = this.onstop(); }
  }
  const sandbox = {
    Blob, FormData, MediaRecorder: Recorder,
    chatUserId: 'original', chatMediaRecorder: null, chatMicStream: null,
    navigator: {mediaDevices: {getUserMedia: async () => stream}},
    document: {getElementById: id => id === 'chat-mic-btn' ? button : input},
    setTimeout: (f, ms) => {timerDelays.push(ms); return 1;}, clearTimeout() {},
    apiGet: async () => ({ok: true, json: async () => ({settings: {voice_stt_type: type}})}),
    chatPrepareSttAudio: async blob => {assert.equal(blob.type, 'audio/ogg;codecs=opus'); return {blob: new Blob(['synthetic PCM'], {type: 'audio/wav'}), filename: 'speech.wav'};},
    apiFetch: async (url, options) => {uploads.push({url, audio: options.body.get('audio')}); if (switchAfterUpload) sandbox.chatUserId = 'other'; return {text: async () => JSON.stringify({text: 'recognized fixture'})};},
    addChatMessage() {},
  };
  vm.createContext(sandbox); vm.runInContext(code, sandbox);
  return {sandbox, button, input, uploads, timerDelays, stopped: () => stopped, setType: v => {type = v;}};
}
(async () => {
  const normal = fixture(); await normal.sandbox.chatToggleMic();
  assert.deepEqual(normal.timerDelays, [60000]);
  await normal.sandbox.chatToggleMic(); await normal.sandbox.chatMediaRecorder.stopped;
  assert.equal(normal.stopped(), 1); assert.equal(normal.button.disabled, false);
  assert.equal(normal.uploads[0].url, '/api/stt?user_id=original');
  assert.equal(normal.uploads[0].audio.name, 'speech.wav'); assert.equal(normal.uploads[0].audio.type, 'audio/wav');
  assert.equal(normal.input.value, 'draft recognized fixture');
  const switched = fixture(); await switched.sandbox.chatToggleMic(); switched.sandbox.chatUserId = 'other';
  await switched.sandbox.chatToggleMic(); await switched.sandbox.chatMediaRecorder.stopped;
  assert.equal(switched.uploads.length, 0); assert.equal(switched.input.value, 'draft'); assert.equal(switched.button.disabled, false);
  const late = fixture({switchAfterUpload: true}); await late.sandbox.chatToggleMic();
  await late.sandbox.chatToggleMic(); await late.sandbox.chatMediaRecorder.stopped;
  assert.equal(late.uploads[0].url, '/api/stt?user_id=original'); assert.equal(late.input.value, 'draft');
  const changed = fixture(); await changed.sandbox.chatToggleMic(); changed.setType('elevenlabs');
  await changed.sandbox.chatToggleMic(); await changed.sandbox.chatMediaRecorder.stopped;
  assert.equal(changed.uploads.length, 0); assert.equal(changed.stopped(), 1);
  const failed = fixture({failRecorder: true}); await failed.sandbox.chatToggleMic();
  assert.equal(failed.stopped(), 1); assert.equal(failed.button.disabled, false);
  console.log('PASS: microphone lifecycle, WAV upload, original-session routing, context/provider switches, failure cleanup');
})().catch(e => {console.error(e); process.exitCode = 1;});
