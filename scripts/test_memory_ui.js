#!/usr/bin/env node
// Actual dashboard renderer, synthetic profiles; no browser/network or live data.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const js = fs.readFileSync(require('node:path').join(__dirname, '../static/app.js'), 'utf8');
const start = js.indexOf('let memoryLoadGeneration = 0;');
const end = js.indexOf('// ═══ VM', start);
assert(start >= 0 && end > start, 'Locate real memory renderer');
const user = { value: 'alice' };
const container = { innerHTML: '' };
const selector = { addEventListener: (_, fn) => { selector.change = fn; } };
const calls = [];
const sample = (name, profile = 'language_instructor') => ({
    profile, active_profile: 'language_instructor', profile_exists: true,
    profiles: ['standard', 'language_instructor', 'project_one'], shared: { name },
    learned_facts: [], last_topics: [], user_preferences: { language: 'French' },
    custom_variables: { srs_items: [{ item: '<img src=x onerror=alert(1)>' }], xp: 4 },
});
const response = data => ({ ok: true, json: async () => data });
let api = async () => response(sample('Alice'));
const context = {
    document: { getElementById: id => id === 'memory-user-id' ? user : id === 'memory-profile-view' ? selector : container },
    apiGet: async path => { calls.push(path); return api(path); },
    escapeHtml: s => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;'),
};
vm.runInNewContext(js.slice(start, end), context);
(async () => {
    await context.loadMemory();
    assert(!container.innerHTML.includes('<img'), 'Memory JSON is data, not executable markup');
    assert(container.innerHTML.includes('&lt;img'));
    assert(container.innerHTML.includes('French') && container.innerHTML.includes('srs_items'));
    assert(container.innerHTML.includes('language_instructor') && container.innerHTML.includes('Alice'));
    assert(container.innerHTML.includes('shared facts'));
    api = async () => response(sample('Alice', 'project_one'));
    await selector.change({ target: { value: 'project_one' } });
    assert.equal(calls.at(-1), '/api/memory/alice?profile=project_one');
    // Only GET exists in this harness: browsing must not activate/modify a profile.
    for (const lateResponse of [response(sample('ALICE_STALE')), { ok: false }]) {
        let release;
        api = () => new Promise(resolve => { release = resolve; });
        user.value = 'alice';
        const pending = context.loadMemory();
        user.value = 'bob'; api = async () => response(sample('BOB_CURRENT'));
        await context.loadMemory();
        release(lateResponse); await pending;
        assert(container.innerHTML.includes('BOB_CURRENT'));
        assert(!container.innerHTML.includes('ALICE_STALE'));
        assert(!container.innerHTML.includes('Failed to load'));
    }
    console.log('test_memory_ui: profiles, shared, escaping and stale-response isolation passed');
})().catch(e => { console.error(e); process.exitCode = 1; });
