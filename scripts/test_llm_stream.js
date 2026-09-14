#!/usr/bin/env node
// Execute the actual streaming UI handler against a tiny synthetic DOM/EventSource.
// No browser, network, credentials or running Praxis instance is used.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../static/app.js'), 'utf8');
const start = source.indexOf('function startChatStream() {');
const end = source.indexOf('\nfunction stopChatStream()', start);
assert(start >= 0 && end > start);
const nodes = [];
const feedback = [];
let es;
const container = { querySelector: () => null, appendChild: node => nodes.push(node) };
const context = {
  chatUserId: 'synthetic', authToken: 'not-a-credential', chatBotName: 'Test',
  chatSeenIds: new Set(), chatEventSource: null,
  console: { log() {}, warn() {}, error(...args) { throw new Error(args.join(' ')); } },
  performance: { now: () => 0 },
  EventSource: class {
    constructor() { this.handlers = {}; es = this; }
    addEventListener(name, handler) { this.handlers[name] = handler; }
  },
  document: {
    body: { classList: { contains: () => false, add() {}, remove() {} } },
    getElementById: () => container,
    createElement() {
      const content = { innerHTML: '', classList: { remove() {} } };
      return { content, get isConnected() { return nodes.includes(this); }, querySelector: () => content, remove() { nodes.splice(nodes.indexOf(this), 1); } };
    }
  },
  stopChatStream() {}, stopChatTimer() {}, autoScrollChat() {},
  addAgentLoopBanner() {}, updateAgentUI() {},
  escapeHtml: text => text, renderMarkdown: text => text,
  addChatMessage: (role, text) => feedback.push([role, text]),
};
vm.runInNewContext(source.slice(start, end) + '\nstartChatStream();', context);
function emit(name, text = '') { assert(es.handlers[name], `missing event ${name}`); es.handlers[name]({ data: JSON.stringify({ data: text }) }); }
emit('char', 'unfinished old reply');
assert.equal(nodes.length, 1);
emit('feedback', 'LLM failed; progress retained');
emit('stream_abort');
assert.equal(nodes.length, 0, 'failed provisional reply must be removed');
emit('char', 'new ');
emit('char', 'reply');
assert.equal(nodes[0].content.innerHTML, 'new reply', 'old stream buffer must be cleared');
emit('assistant', 'new reply');
emit('stream_abort');
assert.equal(nodes.length, 1, 'a later abort must not erase a committed reply');
assert.equal(nodes[0].content.innerHTML, 'new reply');
assert.equal(feedback[0][0], 'feedback');
console.log('test_llm_stream: ok');
