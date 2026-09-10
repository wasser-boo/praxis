'use strict';
// Read-only prospective filesystem view for Microsoft's CLI. A candidate is
// rendered from a temporary sibling; any import of its final destination must
// see the candidate, never the OLD on-disk template. No disk file is replaced.
const fs = require('node:fs');
const path = require('node:path');
const { fileURLToPath } = require('node:url');
const realpath = fs.realpathSync.bind(fs);
function canonical(file) {
  if (file instanceof URL) file = fileURLToPath(file);
  if (Buffer.isBuffer(file)) file = file.toString();
  if (typeof file !== 'string') return null; // preserve file-descriptor reads
  const absolute = path.resolve(file);
  try { return realpath(absolute); }
  catch {
    try { return path.join(realpath(path.dirname(absolute)), path.basename(absolute)); }
    catch { return absolute; }
  }
}
const destination = canonical(process.env.PRAXIS_POML_DESTINATION);
const candidate = process.env.PRAXIS_POML_CANDIDATE;
if (!destination || !candidate) throw new Error('Missing POML candidate overlay paths');
function redirect(file) { return canonical(file) === destination ? candidate : file; }
const readSync = fs.readFileSync.bind(fs);
const read = fs.readFile.bind(fs);
const readAsync = fs.promises.readFile.bind(fs.promises);
fs.readFileSync = (file, ...args) => readSync(redirect(file), ...args);
fs.readFile = (file, ...args) => read(redirect(file), ...args);
fs.promises.readFile = (file, ...args) => readAsync(redirect(file), ...args);
require('node:module').syncBuiltinESMExports();
