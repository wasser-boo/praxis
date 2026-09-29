#!/usr/bin/env node
/* Real TUI event-loop smoke test through util-linux script(1)'s PTY.
 * Requires a built praxis binary and Node 22+ (node:sqlite).
 * Synthetic temporary DB + loopback gateway only. No real clipboard writes:
 * terminal output including OSC 52 is captured, never forwarded to stdout.
 * Usage: node scripts/test_tui_selection_pty.js /path/to/praxis
 */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const assert = require('node:assert/strict');
const {spawn} = require('node:child_process');
const {DatabaseSync} = require('node:sqlite');
const binary = path.resolve(process.argv[2] || 'target/debug/praxis');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'praxis-selection-pty-'));
const prose = "  PTY-FIRST 世界 e\u0301 👩‍💻\n\n\tprintf 'literal \\n'\nPTY-LAST";
// Run the identical real PTY regression with a structured receipt as well.
const toolResult = process.env.PRAXIS_PTY_TOOL_RESULT === '1';
const text = toolResult ? JSON.stringify({stdout:prose,stderr:'warning\nlast',exit_code:3}) : prose;
const role = toolResult ? 'tool' : 'assistant';
const requests = [];
const server = http.createServer((req, res) => {
    requests.push([req.method, req.url]);
    if (req.headers.authorization !== 'Bearer synthetic-key') {
        res.writeHead(401).end(); return;
    }
    if (req.url.startsWith('/v1/events/')) {
        res.writeHead(200, {'Content-Type': 'text/event-stream'});
        res.write(': synthetic keepalive\n\n'); return;
    }
    res.setHeader('Content-Type', 'application/json');
    if (req.url === '/v1/sessions') {
        res.end(JSON.stringify({sessions: [{id: 'synthetic', name: 'Synthetic', username: 'tester'}]}));
    } else if (req.url === '/v1/messages/synthetic') {
        res.end(JSON.stringify({messages: [{id: 1, role, content: text}]}));
    } else { res.writeHead(404).end('{}'); }
});
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const quote = s => "'" + s.replaceAll("'", "'\\''") + "'";
async function run(mode, prepare) {
    const data = path.join(root, mode);
    fs.mkdirSync(data, {recursive: true});
    if (prepare) prepare(data);
    const port = server.address().port;
    const cmd = `stty cols 100 rows 24; exec ${quote(binary)} chat --gateway-key synthetic-key` +
        (mode === 'remote' ? ` --gateway-url http://127.0.0.1:${port}` : '');
    // A deliberately minimal environment prevents inherited backend credentials,
    // dotenv files, or live data paths from entering the subprocess.
    const child = spawn('script', ['-q', '-e', '-E', 'never', '-c', cmd, '/dev/null'], {
        cwd: root, env: {PATH: process.env.PATH, LD_LIBRARY_PATH: process.env.LD_LIBRARY_PATH || '',
            HOME: root, ROOT_DIR: root, TERM: 'xterm-256color', USER: 'tester',
            DATA_DIR: data, LOG_DIR: path.join(root, 'logs'), GATEWAY_PORT: String(port)},
        stdio: ['pipe', 'pipe', 'pipe'],
    });
    let output = '', stderr = '', exited = false;
    child.stdout.on('data', chunk => { output += chunk.toString(); });
    child.stderr.on('data', chunk => { stderr += chunk.toString(); });
    const finished = new Promise((resolve, reject) => {
        child.on('error', reject);
        child.on('close', (code, signal) => { exited = true; resolve({code, signal}); });
    });
    async function waitFor(predicate, label) {
        const until = Date.now() + 15000;
        while (!predicate()) {
            if (exited || Date.now() > until) throw new Error(`${mode}: ${label}; exited=${exited}; stderr=${JSON.stringify(stderr)}; tail=${JSON.stringify(output.slice(-400))}`);
            await sleep(25);
        }
    }
    async function key(bytes) { child.stdin.write(bytes); await sleep(150); }
    try {
        await waitFor(() => output.includes('F3 copy'), 'initial draw');
        if (mode === 'init') { await key('\x11'); }
        else {
            await waitFor(() => output.includes('PTY-FIRST'), 'synthetic history');
            const mouseStart = output.length;
            await key('\x1bOQ'); // F2 in xterm
            await waitFor(() => output.slice(mouseStart).includes('\x1b[?1000l'), 'mouse capture disabled');
            await key('\x1b[5~'); // PageUp
            const restoreStart = output.length;
            await key('\x1bOQ');
            await waitFor(() => output.slice(restoreStart).includes('\x1b[?1000h'), 'mouse capture restored');
            await key('\x1b[200~UNSENT\r\n  世界\x1b[201~');
            await key('\x1bOR'); // F3 in xterm
            await waitFor(() => output.includes('COPY 1/1'), 'copy mode');
            assert(!output.includes('\x1b]52;'), 'mode entry cannot write clipboard');
            await key('\r'); // Enter must NOT submit draft
            await key('\x03'); // Ctrl+C must copy, not quit
            await waitFor(() => output.includes('\x1b]52;c;'), 'clipboard request');
            const copies = [...output.matchAll(/\x1b\]52;c;([^\x07]*)\x07/g)];
            assert.equal(copies.length, 1);
            assert.equal(Buffer.from(copies[0][1], 'base64').toString(), text);
            assert(!exited, 'Ctrl+C in copy mode must not quit');
            await key('\x11'); // Ctrl+Q intentionally quits, even in copy mode
        }
        await waitFor(() => exited, 'clean quit');
        const result = await finished;
        assert.equal(result.code, 0, JSON.stringify(result));
        assert(output.includes('\x1b[?1049l'), 'alternate screen restored');
        assert(output.includes('\x1b[?2004l'), 'bracketed paste disabled on teardown');
        assert(output.includes('\x1b[?1000l'), 'mouse capture disabled on teardown');
        console.log(`PASS ${mode}: real PTY event loop${mode === 'init' ? ' startup/quit' : ', F2/F3, paste, explicit OSC 52, Ctrl+Q, teardown'}`);
    } finally {
        if (!exited) child.kill('SIGTERM');
        child.stdin.destroy();
    }
}
(async () => {
    try {
        assert(fs.existsSync(binary), `Build binary first: ${binary}`);
        await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
        await run('init'); // create a DB using real migrations, never guess its schema
        await run('local', data => {
            fs.copyFileSync(path.join(root, 'init/praxis.db'), path.join(data, 'praxis.db'));
            const db = new DatabaseSync(path.join(data, 'praxis.db'));
            db.prepare('INSERT INTO contexts (user_id, data) VALUES (?, ?)').run('tui_default', '{}');
            db.prepare('INSERT INTO messages (user_id, role, content) VALUES (?, ?, ?)').run('tui_default', role, text);
            db.close();
        });
        await run('remote');
        assert(requests.some(([method, url]) => method === 'GET' && url === '/v1/messages/synthetic'));
        assert(requests.every(([method]) => method === 'GET'), `No chat/command submission allowed: ${JSON.stringify(requests)}`);
        const db = new DatabaseSync(path.join(root, 'local/praxis.db'));
        assert.equal(db.prepare('SELECT COUNT(*) AS n FROM messages').get().n, 1);
        assert.equal(db.prepare('SELECT content FROM messages').get().content, text, 'formatting/copy must not rewrite stored history');
        db.close();
        console.log('PASS: no draft sent, no gateway mutation, no real clipboard or live service touched');
    } finally {
        server.closeAllConnections();
        server.close();
        fs.rmSync(root, {recursive: true, force: true});
    }
})().catch(error => { console.error(error.message); process.exitCode = 1; });
