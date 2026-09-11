# .mnemodim format cheatsheet

Compact v1 reminder, corrected against the bundled [import guide and error catalogue](MNEMODIM_IMPORT_GUIDE.md), audited at mnemodim commit `8276140`. Read that guide before every creation/edit/repair; it takes precedence over older summaries. Compare against the target importer if its version changes. This summary and the Python helper are not replacements for the real decoder and formula parser.

## Container

`.mnemodim` is a ZIP package with only listed files. Use stored/DEFLATE compression, no encryption or ZIP64, no enclosing folder, explicit directory entries, README or OS metadata. The app validates paths, sizes, CRCs, signatures, references, and limits before import.

Required root entry:

```json
{
  "format": "mnemodim",
  "version": 1,
  "kind": "palace",
  "name": "Example",
  "entryPalaceId": "p0",
  "palaces": [{ "id": "p0", "name": "Example", "background": "stage0", "thumbnailImagePath": "stage0" }],
  "assets": [{ "id": "stage0", "mime": "image/jpeg", "path": "assets/stage0", "size": 12345 }],
  "csvEncoding": "json-cells-v1",
  "sheets": []
}
```

`kind: "palace"` must contain exactly one palace. `kind: "collection"` contains at least one ordered palace. `entryPalaceId` must be a member. Palace `background` is required: use `""` for none, otherwise an embedded asset ID, never a CSS color or URL.

Portable IDs match `^[A-Za-z0-9_-]{1,128}$` and must be globally unique across palaces, stages, loci, rows and assets. `__proto__`, `prototype` and `constructor` are blocked names. Do not add unknown/database-only fields.

## CSV encoding

Headers are conventional CSV column names in the **exact complete order below**, even for absent optional properties. Each non-empty data field is a JSON value inside RFC 4180 CSV quoting. Use JSON and CSV libraries, never hand-built quoting. UTF-8 without BOM. Shared `palaceId` is a present JSON null, not an empty field or the string `"null"`.

Example logical row:

```json
{
  "id": "l1",
  "palaceId": "p0",
  "stageId": "s1",
  "label": "1",
  "memory": "Tee",
  "x": 0.27,
  "y": 0.12,
  "zoom": 2,
  "contentType": "recall",
  "imagePath": "a20"
}
```

Appears as fields like `"""l1"""` for strings and `"0.27"` for numbers. Empty optional columns mean absent property.

## CSV columns

Stages:

```text
id,palaceId,name,orderIndex,imagePath
```

Loci:

```text
id,palaceId,stageId,label,memory,x,y,zoom,panX,panY,notes,imagePath,soundPath,contentType,description,quiz,alternatives,bindings
```

Workbook tables:

```text
id,palaceId,sheet,key,cells
```

A table path is either:

```text
palaces/<palaceId>/tables/<Sheet>.csv
shared/tables/<Sheet>.csv
```

and must be listed in `manifest.sheets` as `{ path, palaceId, sheet }`.
Every custom table needs at least one row. With no rows use `sheets: []` and omit
custom tables. Every palace still needs stage/locus CSVs, even if header-only.
Stage `id,palaceId,name,orderIndex`, locus `id,palaceId,stageId,label,memory,x,y,contentType`,
and all five table row properties are required. Omit absent optional properties
instead of replacing them with null.

## Workbook cells

Cell shape:

```json
{ "column": "object", "type": "text", "value": "Tee" }
```

Valid types: `text`, `number`, `boolean`, `json`, `formula`, `asset`, `palace`, `stage`.

Rules:
- Every cell has exactly `column`, `type`, `value`. **`value` is always a string**, including numbers, booleans and JSON. Native values, missing values and null cause `Invalid cell` before type-specific validation.
- Sheet/column names use ASCII identifiers: letter first, then letters/digits/underscore, max 64. Blocked names are forbidden; `View` is a reserved sheet. Columns must be unique in each row.
- Row keys are nonempty strings, max 64 UTF-16 code units; `(palaceId,sheet,key)` must be unique. Display names are not identity.
- `number`: `"value":"7"`; nonempty string convertible to a finite number of magnitude at most `9007199254740991`. Use text `"007"` for leading-zero identifiers.
- `boolean`: `"value":"true"` or `"value":"false"`, exactly lowercase **strings**, not native booleans or Python `str(True)`.
- `json`: `"value":"[1,2]"` or `"value":"null"`; a string containing bounded JSON, not a native list/record/null. At most 4,000 nodes, depth 24, no forbidden keys or unbounded numbers.
- `asset`, `palace`, `stage`: string package IDs, not URLs or paths. Include referenced objects in the package.
- Cell values allow at most 16,000 UTF-16 code units and no actual NUL. Do not double-stringify already-correct text.
- This rule applies ONLY to `cell.value`: manifest version, stage order, coordinates, zoom/pan and asset sizes remain JSON numbers; arrays remain arrays and shared scope remains actual null before CSV encoding.

## Formulas

References:
- `Sheet.key.column`
- `Sheet.key` means `.value` if that column exists, otherwise an object of columns.
- Numeric/special keys use brackets: `Major["07"].object`.
- `$input1` means `Inputs.input1.value`.
- `View.reveal` is run state and is not a saved sheet.

Built-ins include `IF/WENN`, `Number`, `Integer`, `Text`, `Floor`, `Abs`, `Mod`, `Min`, `Max`, `Length`, `Digits`, `Reverse`, `Concat`, `SetAt`, and `Major`.

`Major(size,digits,optionalSheet)` splits a digit string into groups and looks up rows in the selected sheet. Example: `Major(2,"487")` looks up `48` then `7`.

Parse **every** formula cell and locus binding with the app's `parseFormula`
before delivery (guide section 8). Preview/structural checks do not parse them;
confirmed backend import does. Expressions allow 2,048 UTF-16 code units, 512
tokens and parser depth 48. Parsing is separate from testing references,
functions, result types and actions at runtime. Never evaluate formulas as JS/Python.

## Locus fields

- `label`: shown name/question prompt.
- `memory`: answer or recall content. For quiz, 1–20 distinct nonempty comma-separated correct answers; a formula-bound quiz still needs a valid saved fallback.
- `description`: mnemonic/visualization notes.
- `quiz`: legacy/optional quiz field; current quiz answers live in `memory`, distractors in `alternatives`.
- `alternatives`: list of wrong answers/distractors or text alternatives.
- `contentType`: `recall`, `quiz`, or `guess`.
- `x`, `y`: normalized marker coordinates (0..1).
- `zoom`: number in 0.01..100, usually 1.4..2.2. `panX/panY` are finite safe-range numbers. Stage `orderIndex` is a nonnegative safe integer.
- `imagePath`: locus/object image asset ID.
- `soundPath`: locus audio asset ID.
- `bindings`: formula array.

Binding shape:

```json
{ "field": "memory", "expression": "Major[\"7\"].object" }
```

Allowed binding fields: `label`, `memory`, `description`, `imagePath`, `x`, `y`, `zoom`, `visible`, `active`. Bind each field at most once (maximum 9); expression strings allow no NUL. Alternatives allow at most 100 strings.

## Limits

- 25 palaces
- 100 stages
- 500 loci
- 500 workbook rows
- 24 cells per row
- 200 assets
- 20 MiB per asset
- 100 MiB total package/expanded contents
- 700,000 UTF-8 bytes of reconstructed document JSON; each non-asset ZIP entry also has a 700,000-byte limit (not 700 KiB, and not the sum of CSV sizes)
- 1000 ZIP entries

See guide section 6 for full string, list, parser and media limits. Python `len()`
is not JavaScript UTF-16 string length for non-BMP characters. Reopen final saved
bytes, run the helper, then the real decoder/formula parser when available.
Report unrun checks; backend auth, destination conflicts/limits and uploads
cannot be guaranteed by a local file check.

## Media

Supported signatures/MIME types:
- Images: `image/png`, `image/jpeg`, `image/webp`, `image/gif`, `image/avif`
- Audio: `audio/mpeg`, `audio/wav`, `audio/x-wav`, `audio/ogg`, `audio/webm`, `audio/mp4`

SVG is not imported; convert to raster first.
