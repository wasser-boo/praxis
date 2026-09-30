#!/usr/bin/env node
/* Isolated TUI/SSE tests for small development containers.
 * Compiles the actual TUI sources and Message types. DB/config/delegation
 * backends are stubs that panic if invoked: this is NOT full integration/DB
 * coverage. No live service, real secrets, or provider calls are used.
 * Requires Cargo/Rust and a C compiler. Uses the project's dependency versions.
 * Run: node scripts/test_tui_isolated.js [cargo test filter]
 */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const root = path.resolve(__dirname, '..');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'praxis-tui-tests-'));
const lib = fs.readFileSync(path.join(root, 'tests/tui_isolated/lib.rs'), 'utf8');
const build = fs.readFileSync(path.join(root, 'tests/tui_isolated/build.rs'), 'utf8');
try {
    fs.mkdirSync(path.join(work, 'src'));
    const rustRoot = JSON.stringify(root).slice(1, -1);
    fs.writeFileSync(path.join(work, 'src/lib.rs'), lib.replaceAll('__ROOT__', rustRoot));
    fs.writeFileSync(path.join(work, 'build.rs'), build.replaceAll('__ROOT__', rustRoot));
    const manifest = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
    const wanted = ["anyhow","tokio","serde","serde_json","reqwest","futures-util","ratatui","crossterm","unicode-segmentation","urlencoding","tracing","rand","chrono","tempfile","wiremock","base64"];
    const deps = wanted.map(name => {
        const line = manifest.split('\n').find(line => line.startsWith(name + ' ='));
        if (!line) throw new Error('Missing dependency: ' + name);
        return line;
    });
    fs.writeFileSync(path.join(work, 'Cargo.toml'),
        '[package]\nname="praxis-tui-investigation"\nversion="0.0.0"\nedition="2021"\n[dependencies]\n' + deps.join('\n') + '\n');
    // Cargo prunes the copied lock to the selected dependencies. The real lock
    // remains untouched. Use a separate persistent target dir for fast reruns.
    fs.copyFileSync(path.join(root, 'Cargo.lock'), path.join(work, 'Cargo.lock'));
    const run = spawnSync('cargo', ['test', '--manifest-path', path.join(work, 'Cargo.toml'),
        ...process.argv.slice(2), '--', '--test-threads=1'], {
        cwd: root, stdio: 'inherit', env: {
            ...process.env,
            CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR || path.join(os.tmpdir(), 'praxis-tui-test-target'),
            CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS || '2',
            CARGO_PROFILE_DEV_DEBUG: '0', CARGO_PROFILE_TEST_DEBUG: '0', CARGO_INCREMENTAL: '0',
        },
    });
    if (run.error) throw run.error;
    process.exitCode = run.status == null ? 1 : run.status;
} finally {
    fs.rmSync(work, {recursive: true, force: true});
}
