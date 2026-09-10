# .mnemodim format cheatsheet

Ported from the Pi mnemodim-palace skill's v1 format reference. If available, compare with the target application's MNEMODIM_FORMAT.md and package/document code before editing: this summary is not a replacement for its importer.

## Container

`.mnemodim` is a ZIP package with only listed files. The app validates paths, sizes, CRCs, signatures, references, and limits before import.

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

`kind: "palace"` must contain exactly one palace. `kind: "collection"` can contain ordered multiple palaces. `entryPalaceId` must be a member.

## CSV encoding

Headers are conventional CSV column names. Each non-empty data field is a JSON value inside CSV quoting.

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

## Workbook cells

Cell shape:

```json
{ "column": "object", "type": "text", "value": "Tee" }
```

Valid types: `text`, `number`, `boolean`, `json`, `formula`, `asset`, `palace`, `stage`.

Rules:
- Sheet and column names are identifiers: start with a letter, then letters/numbers/underscore, max 64.
- Row keys are stable IDs, max 64 characters; display names are not identity.
- `number` cell values must parse to finite safe numbers.
- `boolean` cell values are literal `true` or `false`.
- `json` cells hold bounded JSON lists/records/scalars.
- `asset`, `palace`, and `stage` cells reference package IDs.

## Formulas

References:
- `Sheet.key.column`
- `Sheet.key` means `.value` if that column exists, otherwise an object of columns.
- Numeric/special keys use brackets: `Major["07"].object`.
- `$input1` means `Inputs.input1.value`.
- `View.reveal` is run state and is not a saved sheet.

Built-ins include `IF/WENN`, `Number`, `Integer`, `Text`, `Floor`, `Abs`, `Mod`, `Min`, `Max`, `Length`, `Digits`, `Reverse`, `Concat`, `SetAt`, and `Major`.

`Major(size,digits,optionalSheet)` splits a digit string into groups and looks up rows in the selected sheet. Example: `Major(2,"487")` looks up `48` then `7`.

## Locus fields

- `label`: shown name/question prompt.
- `memory`: answer or recall content. For quiz, comma-separated correct answers.
- `description`: mnemonic/visualization notes.
- `quiz`: legacy/optional quiz field; current quiz answers live in `memory`, distractors in `alternatives`.
- `alternatives`: list of wrong answers/distractors or text alternatives.
- `contentType`: `recall`, `quiz`, or `guess`.
- `x`, `y`: normalized marker coordinates (0..1).
- `zoom`: focus scale, usually 1.4..2.2.
- `imagePath`: locus/object image asset ID.
- `soundPath`: locus audio asset ID.
- `bindings`: formula array.

Binding shape:

```json
{ "field": "memory", "expression": "Major[\"7\"].object" }
```

Allowed binding fields: `label`, `memory`, `description`, `imagePath`, `x`, `y`, `zoom`, `visible`, `active`.

## Limits

- 25 palaces
- 100 stages
- 500 loci
- 500 workbook rows
- 24 cells per row
- 200 assets
- 20 MiB per asset
- 100 MiB total package/expanded contents
- 700 KB metadata
- 1000 ZIP entries

## Media

Supported signatures/MIME types:
- Images: `image/png`, `image/jpeg`, `image/webp`, `image/gif`, `image/avif`
- Audio: `audio/mpeg`, `audio/wav`, `audio/x-wav`, `audio/ogg`, `audio/webm`, `audio/mp4`

SVG is not imported; convert to raster first.
