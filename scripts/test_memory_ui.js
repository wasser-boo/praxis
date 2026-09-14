#!/usr/bin/env node
// Actual dashboard memory renderer with synthetic data; no browser/network.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const js = fs.readFileSync(require('node:path').join(__dirname, '../static/app.js'), 'utf8');
const start = js.indexOf('async function loadMemory() {');
const end = js.indexOf('// ═══ VM', start);
const container = { innerHTML: '' };
const context = {
    document: { getElementById: id => id === 'memory-user-id' ? { value: 'alice' } : container },
    apiGet: async () => ({ ok: true, json: async () => ({ learned_facts: [], last_topics: [], user_preferences: { language: 'French' }, custom_variables: { srs_items: [{ item: '<img src=x onerror=alert(1)>' }], xp: 4 } }) }),
    escapeHtml: s => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;'),
};
(async () => {
    await vm.runInNewContext(js.slice(start, end) + '\nloadMemory();', context);
    assert(!container.innerHTML.includes('<img'), 'Memory JSON is data, not executable markup');
    assert(container.innerHTML.includes('&lt;img'));
    assert(container.innerHTML.includes('French') && container.innerHTML.includes('srs_items'));
    console.log('test_memory_ui: ok');
})().catch(e => { console.error(e); process.exitCode = 1; });
