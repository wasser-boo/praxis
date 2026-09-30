#!/usr/bin/env node
// Derive every app/browser icon from the same generated bitmap, never redraw it.
// node scripts/build_logo.mjs static/logo-source.png static /path/to/sharp
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
const require = createRequire(import.meta.url);
const [input, out = 'static', modulePath = 'sharp'] = process.argv.slice(2);
if (!input) throw new Error('Usage: build_logo.mjs INPUT.png [OUTPUT_DIR] [SHARP_MODULE]');
const sharp = require(modulePath);
await mkdir(out, { recursive: true });
const master = await sharp(resolve(input)).flatten({ background: '#f7f2e6' })
  .resize(512, 512, { fit: 'contain', background: '#f7f2e6' }).png().toBuffer();
await writeFile(resolve(out, 'logo.png'), master);
for (const [name, size] of [['favicon.png', 32], ['apple-touch-icon.png', 180]]) {
  await sharp(master).resize(size, size).png().toFile(resolve(out, name));
}
const sizes = [16, 32, 48, 64, 128, 256];
const bitmaps = await Promise.all(sizes.map(size => sharp(master).resize(size, size).png().toBuffer()));
const header = Buffer.alloc(6 + sizes.length * 16);
header.writeUInt16LE(1, 2); header.writeUInt16LE(sizes.length, 4);
let offset = header.length;
for (let i = 0; i < sizes.length; i++) {
  const index = 6 + i * 16;
  header[index] = header[index + 1] = sizes[i] === 256 ? 0 : sizes[i];
  header.writeUInt16LE(1, index + 4); header.writeUInt16LE(32, index + 6);
  header.writeUInt32LE(bitmaps[i].length, index + 8); header.writeUInt32LE(offset, index + 12);
  offset += bitmaps[i].length;
}
await writeFile(resolve(out, 'favicon.ico'), Buffer.concat([header, ...bitmaps]));
console.log('Generated logo.png, favicon.png, favicon.ico, apple-touch-icon.png from one master.');
