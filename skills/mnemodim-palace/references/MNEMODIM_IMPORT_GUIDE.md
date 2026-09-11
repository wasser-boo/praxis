# mnemodim v1: package generation, import errors, and prevention

Standalone instructions for an AI skill or generator in **any harness**. No pi-specific setup is required. Give this document to the skill before it creates a `.mnemodim` file.

This guide reflects the importer audited at repository commit `8276140`, format version **1**. It describes file validation and the additional backend checks; it is not a historical log of every user's failed imports. Recheck against the importer if the format changes.

## 1. Mandatory instructions for the generating skill

Copy this contract into the skill's instructions or have it read this file for every package-generation task:

> - Produce a real ZIP named `.mnemodim`, not renamed JSON.
> - Every workbook cell has exactly `column`, `type`, and `value`. **`value` is always a string, including number, boolean, and JSON cells.**
> - A number cell uses `"value":"7"`; a boolean cell uses `"value":"true"`; a JSON cell uses `"value":"[1,2]"`. Native numbers, booleans, arrays, objects, missing values, and null are invalid as `cell.value`.
> - Do not stringify the whole document indiscriminately. Locus coordinates, stage order, asset sizes, and manifest version remain JSON numbers. Shared row scope is the actual JSON null.
> - Use `json-cells-v1`: JSON-serialize every present CSV data field, then CSV-escape it with a real CSV writer. Do not build CSV using string concatenation.
> - Use the exact manifest fields, CSV columns, portable IDs, scopes, limits, and embedded-media rules below. Do not invent fields.
> - Include only listed files, with no enclosing folder, explicit directory entries, README, or operating-system metadata inside the ZIP.
> - Embed supported images/audio. Store asset IDs in references, never URLs, file paths, or base64 payloads.
> - Preserve text and leading zeroes. Do not silently truncate data, remove media, or change learning content just to pass validation.
> - Parse every formula cell and locus binding before handing off the file. A successful structural preview alone does not validate formula syntax or behavior.
> - Use the app's `encodePackage`, `decodePackage`, and `parseFormula` when available. Reopen and validate the final saved bytes, not just an intermediate object.
> - Report what was actually verified. Local validation is not proof of successful upload or backend import. Never claim an unrun check passed.

For new language/vocabulary palaces, follow the app's authoring convention to use `contentType: "quiz"` with valid comma-separated correct answers. Generate pronunciation audio only when explicitly requested. These authoring conventions are distinct from import validation: existing `recall` content can be valid and must not be silently converted during a repair.

## 2. Confirmed incident: the Hiragana palace

### Files

- Original: `kana_palast_hiragana_a_ka.mnemodim`
- Repaired: `kana_palast_hiragana_a_ka_fixed.mnemodim`
- Both are in `/home/wasser/Documents`.
- Offending archive member: `palaces/p0/tables/Kana.csv`
- Error reproduced with the real decoder: **`Invalid cell`**.

The skill emitted native JSON numbers for all ten `order` cells. The importer calls a string validator on `cell.value` **before** checking the declared cell type. `type: "number"` does not waive the string requirement.

| Exact cell      | Original value (number) | Required value (string) |
| --------------- | ----------------------- | ----------------------- |
| `Kana.a.order`  | `1`                     | `"1"`                   |
| `Kana.i.order`  | `2`                     | `"2"`                   |
| `Kana.u.order`  | `3`                     | `"3"`                   |
| `Kana.e.order`  | `4`                     | `"4"`                   |
| `Kana.o.order`  | `5`                     | `"5"`                   |
| `Kana.ka.order` | `6`                     | `"6"`                   |
| `Kana.ki.order` | `7`                     | `"7"`                   |
| `Kana.ku.order` | `8`                     | `"8"`                   |
| `Kana.ke.order` | `9`                     | `"9"`                   |
| `Kana.ko.order` | `10`                    | `"10"`                  |

Incorrect:

```json
{ "column": "order", "type": "number", "value": 1 }
```

Correct:

```json
{ "column": "order", "type": "number", "value": "1" }
```

### Repair verification

Only those ten cell values were changed. The repaired file was saved and reopened with the application's `decodePackage` successfully. The package contains **1 palace, 2 stages, 10 loci, 10 workbook rows, 12 images, and 10 audio files**.

- The original file is unchanged.
- The only archive member with changed content is `palaces/p0/tables/Kana.csv`.
- The manifest, stage/locus CSVs, and all 22 media files are byte-for-byte identical to their original contents.
- Recompression changed the ZIP bytes and size; this does not mean images/audio were altered.
- No backend upload or import was performed during repair.

SHA-256 checksums:

```text
original: 64feb7a41fbc02223019d4647b7b918ae1d87c014e37f3808805b9892f0b2d2e
repaired: b0fc62d4efa85cf955fdf286d252207f93844cb43e48b37e0a06d590deffcd28
```

Repaired file size: **13,836,556 bytes**. Import the `_fixed.mnemodim` file, not the original.

## 3. Workbook cell contract

Every cell must look like this:

```json
{ "column": "someColumn", "type": "text", "value": "some text" }
```

Exactly these types are supported, with case-sensitive names:

| `type`    | Example `value` in JSON       | Interpretation                                                      |
| --------- | ----------------------------- | ------------------------------------------------------------------- |
| `text`    | `"007"`                       | Literal text; preserves leading zeroes.                             |
| `number`  | `"7"`                         | Nonempty string convertible to a finite, bounded number.            |
| `boolean` | `"true"`                      | Exactly `"true"` or `"false"`, lowercase, without extra whitespace. |
| `json`    | `"[1,2,3]"`                   | A string containing valid JSON, parsed later.                       |
| `formula` | `"Number(Inputs.amount) + 1"` | An expression in the app's formula language, not JavaScript.        |
| `asset`   | `"a_image"`                   | Portable ID of an embedded asset.                                   |
| `palace`  | `"p_home"`                    | Portable ID of a palace included in the package.                    |
| `stage`   | `"s_room"`                    | Portable ID of a stage included in the package.                     |

A JSON object cell needs JSON **inside** its string:

```json
{ "column": "settings", "type": "json", "value": "{\"enabled\":true,\"count\":3}" }
```

Rules that commonly cause mistakes:

- `cell.value` must exist, be a string of at most 16,000 JavaScript string code units, and contain no actual NUL character.
- Empty strings are allowed for text; they are not valid numeric, boolean, or JSON content. Use real nonempty IDs for reference cells.
- Number cells must convert to a finite number with absolute value at most `9007199254740991`. Fractions are allowed. Do not use locale formatting such as `"1,5"`.
- Use `"type":"text"` for digit identifiers such as `"007"` when leading zeroes matter.
- `"type":"text","value":"=1+2"` remains text. A leading `=` does not turn text into a formula.
- `"True"`, `"FALSE"`, `"yes"`, and `"1"` are not boolean cell values.
- A JSON cell containing the JSON value null is `"value":"null"`, not `"value":null`.
- Native booleans/objects/arrays are appropriate **inside the parsed JSON string**, not as `cell.value` itself.
- Do not apply `JSON.stringify` indiscriminately to an already-correct text cell value: it adds literal quotes. Choose serialization according to the cell type.
- In Python, `str(True)` produces `"True"`, which is wrong. Use `"true" if enabled else "false"`. Use `json.dumps` for JSON cells, not `str(dict)`.

### Fields that must NOT use the cell-value string rule

| Field                                             | Required JSON representation                                                                  |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| Manifest `version`                                | Number `1`.                                                                                   |
| Stage `orderIndex`                                | Nonnegative safe integer, e.g. `0`.                                                           |
| Locus `x`, `y`                                    | Numbers between `0` and `1`, e.g. `0.5`.                                                      |
| Optional locus `zoom`                             | Number between `0.01` and `100`.                                                              |
| Optional locus `panX`, `panY`                     | Finite numbers within the supported safe-number range.                                        |
| Asset `size`                                      | Positive integer equal to the embedded file's byte count.                                     |
| Shared row/sheet `palaceId`                       | Actual `null`, not `"null"` or an empty field.                                                |
| Locus `alternatives`, `bindings`, and row `cells` | Arrays, not strings in the in-memory document. CSV encoding serializes the entire array once. |

## 4. Container, schema, and cross-reference rules

### ZIP layout

```text
manifest.json
palaces/p0/stages.csv
palaces/p0/loci.csv
palaces/p0/tables/Kana.csv       # only when declared in sheets
shared/tables/Inputs.csv        # only when declared in sheets
assets/a_image                 # only when declared in assets
```

- `.mnemodim` is an ordinary nonencrypted ZIP, using stored or DEFLATE compression.
- Use normal ZIP output, not split archives or ZIP64 extensions.
- `manifest.json` is at the root: do not add an enclosing directory.
- Each palace needs both its stage and locus CSV files, even if a file contains only its header.
- Custom sheet files must contain at least one row and be declared in `sheets`. If there are no workbook rows, use `sheets: []` and omit custom sheet files.
- Do not add directory entries such as `palaces/`, hidden files, a README, thumbnails outside the asset system, or this Markdown guide to the ZIP.
- Paths are case-sensitive. Asset files have the form `assets/<id>` with no added filename extension.
- Keep paths ASCII and generated from the schema. Avoid absolute paths, empty path segments, traversal, backslashes, and blocked names.

### Manifest

Required root fields:

```json
{
	"format": "mnemodim",
	"version": 1,
	"kind": "palace",
	"name": "Example",
	"entryPalaceId": "p0",
	"palaces": [{ "id": "p0", "name": "Example", "background": "" }],
	"assets": [],
	"csvEncoding": "json-cells-v1",
	"sheets": []
}
```

- `kind` is `palace` or `collection`. A palace package has exactly one palace; a collection has at least one.
- Palace records have `id`, `name`, `background`, and optional `thumbnailImagePath`.
- `background` is required. Use `""` for none, or an asset ID. It is not a CSS color or URL.
- Asset records have exactly `id`, `path`, `mime`, and `size`; `path` is `assets/<id>`.
- Sheet descriptors have `path`, `palaceId`, and `sheet`. A palace-local path is `palaces/<palaceId>/tables/<sheet>.csv`; shared scope uses `palaceId: null` and `shared/tables/<sheet>.csv`.
- `stages`, `loci`, and `rows` are reconstructed from CSVs. They belong in the in-memory `PalaceDocument`, not as a replacement for the CSV files. Prefer `encodePackage` to build the manifest/layout for you.
- Do not add user IDs, sessions, XP, mastery, recall counters, timestamps, or database `_id` fields. Import creates new private copies and resets personal progress.

### Exact CSV header order

Headers are ordinary CSV strings, not JSON strings inside CSV. Every expected column must be present, including optional columns.

Stages:

```text
id,palaceId,name,orderIndex,imagePath
```

Loci:

```text
id,palaceId,stageId,label,memory,x,y,zoom,panX,panY,notes,imagePath,soundPath,contentType,description,quiz,alternatives,bindings
```

Workbook rows:

```text
id,palaceId,sheet,key,cells
```

Stage `imagePath` is optional. Locus `id`, `palaceId`, `stageId`, `label`, `memory`, `x`, `y`, and `contentType` are required; the other locus columns are optional. All five workbook-row properties are required, including `palaceId` as a string or null.

Use `recall`, `quiz`, or `guess` for supported learning modes. The schema currently accepts a general string for `contentType`; an invented mode passing validation is not proof the UI knows how to use it.

### Identity and scope

- Portable IDs match `^[A-Za-z0-9_-]{1,128}$` and are globally unique across palaces, stages, loci, rows, and assets. Prefixes such as `p_`, `s_`, `l_`, `r_`, and `a_` help avoid collisions. Avoid reserved path names such as `__proto__` and `constructor`.
- `entryPalaceId` must name an included palace.
- Every stage references an included palace.
- Every locus references an included stage whose palace matches the locus's `palaceId`.
- Row scope is an included palace ID, or actual null for shared scope.
- `(palaceId, sheet, key)` must be unique. The same local/shared key in different scopes is allowed; local rows shadow shared rows at runtime.
- Sheet and column names match `^[A-Za-z][A-Za-z0-9_]{0,63}$`. Columns must be unique within a row.
- Row keys are nonempty strings of at most 64 code units. Numeric-looking keys are strings. For simple dot references, prefer identifier-like keys; use bracket syntax for keys such as `"07"`.
- `__proto__`, `prototype`, and `constructor` are blocked identifiers/keys. `View` is a reserved sheet for transient run state.
- Typed palace/stage references must target objects included in the package. Include the containing collection rather than leaving external links.
- Omit absent optional properties. Do not replace them with null, which usually fails their validators.

### Media

Allowed MIME types:

```text
image/png image/jpeg image/webp image/gif image/avif
audio/mpeg audio/wav audio/ogg audio/webm audio/mp4 audio/x-wav
```

The decoder checks file signatures, declared sizes, and ZIP checksums. MIME labels and filename changes do not convert media. Convert SVG and unsupported formats to a supported raster/audio format before packaging.

Background, thumbnail, stage image, locus image/sound, and nonempty asset-cell references must identify declared embedded assets. A URL, `assets/<id>` path, local filename, or base64 image is not a valid asset reference; use the asset ID alone. Reuse one asset ID when several objects use the same media.

### Quizzes and bindings

- A quiz's saved `memory` contains 1–20 distinct nonempty comma-separated correct answers. Whitespace is trimmed and case-insensitive duplicates are ignored. Commas are answer separators, not an escapeable part of an answer in this representation.
- Formula-bound quizzes still need a valid saved `memory` fallback; import validates it before evaluating anything.
- Bindings are `{ "field": "memory", "expression": "Inputs.answer" }` objects.
- Allowed fields: `label`, `memory`, `description`, `imagePath`, `x`, `y`, `zoom`, `visible`, `active`.
- Bind each field at most once. Expressions must be strings with no NUL and at most 2,048 code units.
- Formula syntax is not JavaScript. Do not use JS methods, object literals, `eval`, or ambient globals. Use the application's documented functions and references.
- A parsed formula can still fail during use: missing rows/functions, circular references, bad indices, incorrect result types, or operation/result limits are runtime problems. Test actual learning flows as well as file structure.

## 5. CSV encoding: two layers, not one

For every **present data property**:

1. Serialize its value as JSON.
2. Put that JSON text in a CSV field using RFC 4180 quoting.

Examples before CSV quoting:

| In-memory property        | CSV field content after JSON serialization, before CSV quoting |
| ------------------------- | -------------------------------------------------------------- |
| String key `007`          | `"007"`                                                        |
| Coordinate number `0.5`   | `0.5`                                                          |
| Shared scope null         | `null`                                                         |
| Empty string              | `""`                                                           |
| Missing optional property | Empty field; do not JSON-serialize a placeholder null.         |
| `cells` array             | `[{"column":"order","type":"number","value":"1"}]`             |

The nested `value` stays a string **inside the JSON-encoded cells array**. This is the exact layer that the Hiragana generator got wrong.

Use UTF-8 without a BOM. Do not use ordinary spreadsheet export that strips JSON quotes, rewrites `007`, changes headers, or interprets leading `=`. Use JSON and CSV libraries, not hand-escaped strings.

### Working standalone Python writer

This stdlib-only example writes a small valid package with a number, boolean, and JSON cell. It demonstrates serialization; it is **not a replacement for full validation of arbitrary generated data**. Save it as a script and pass a new output filename. Mode `x` deliberately refuses to overwrite an existing file.

```python
import csv
import io
import json
import sys
import zipfile
from pathlib import Path

STAGE_COLUMNS = ["id", "palaceId", "name", "orderIndex", "imagePath"]
LOCUS_COLUMNS = [
    "id", "palaceId", "stageId", "label", "memory", "x", "y", "zoom",
    "panX", "panY", "notes", "imagePath", "soundPath", "contentType",
    "description", "quiz", "alternatives", "bindings",
]
ROW_COLUMNS = ["id", "palaceId", "sheet", "key", "cells"]


def json_text(value):
    return json.dumps(
        value, ensure_ascii=False, allow_nan=False, separators=(",", ":")
    )


def encode_table(rows, columns):
    stream = io.StringIO(newline="")
    writer = csv.writer(stream, lineterminator="\r\n", quoting=csv.QUOTE_ALL)
    writer.writerow(columns)
    for row in rows:
        writer.writerow([
            json_text(row[column]) if column in row else ""
            for column in columns
        ])
    return stream.getvalue().encode("utf-8")


manifest = {
    "format": "mnemodim",
    "version": 1,
    "kind": "palace",
    "name": "Serialization example",
    "entryPalaceId": "p0",
    "palaces": [{"id": "p0", "name": "Example", "background": ""}],
    "assets": [],
    "csvEncoding": "json-cells-v1",
    "sheets": [{
        "path": "palaces/p0/tables/Data.csv",
        "palaceId": "p0",
        "sheet": "Data",
    }],
}
stages = [{"id": "s0", "palaceId": "p0", "name": "Room", "orderIndex": 0}]
loci = [{
    "id": "l0", "palaceId": "p0", "stageId": "s0",
    "label": "Desk", "memory": "Example answer",
    "x": 0.5, "y": 0.5, "contentType": "recall",
}]
rows = [{
    "id": "r0", "palaceId": "p0", "sheet": "Data", "key": "example",
    "cells": [
        {"column": "order", "type": "number", "value": str(1)},
        {"column": "enabled", "type": "boolean", "value": "true"},
        {"column": "items", "type": "json", "value": json_text([1, 2])},
    ],
}]

output = Path(sys.argv[1] if len(sys.argv) > 1 else "example.mnemodim")
if output.suffix.lower() != ".mnemodim":
    raise ValueError("Use a .mnemodim output filename")

with zipfile.ZipFile(output, "x", compression=zipfile.ZIP_DEFLATED) as archive:
    archive.writestr("manifest.json", json_text(manifest).encode("utf-8"))
    archive.writestr("palaces/p0/stages.csv", encode_table(stages, STAGE_COLUMNS))
    archive.writestr("palaces/p0/loci.csv", encode_table(loci, LOCUS_COLUMNS))
    archive.writestr("palaces/p0/tables/Data.csv", encode_table(rows, ROW_COLUMNS))

print(output)
```

## 6. Limits to check before generating a large package

| Item                                | Current v1 limit                                                                                                        |
| ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Palaces                             | 25 per package/workspace; exactly 1 for `kind: "palace"`.                                                               |
| Stages                              | 100 total.                                                                                                              |
| Loci                                | 500 total.                                                                                                              |
| Workbook rows                       | 500 total.                                                                                                              |
| Cells                               | 24 per row.                                                                                                             |
| Sheet descriptors                   | 500.                                                                                                                    |
| Assets                              | 200.                                                                                                                    |
| ZIP entries                         | 1,000.                                                                                                                  |
| Reconstructed document metadata     | 700,000 UTF-8 bytes of JSON, not just `manifest.json`.                                                                  |
| Individual non-asset ZIP entry      | 700,000 uncompressed bytes. CSV escaping can make a table larger than its object representation.                        |
| Individual asset                    | 1–20 MiB inclusive; exact positive integer byte count.                                                                  |
| ZIP file size                       | At most 100 MiB; a structurally valid ZIP is also required.                                                             |
| Total expanded ZIP contents         | At most 100 MiB, including metadata.                                                                                    |
| Sum of declared asset sizes         | At most 100 MiB. Leave room for metadata and ZIP overhead.                                                              |
| Ordinary strings / cell values      | 16,000 JavaScript string code units; no actual NUL.                                                                     |
| Portable IDs / MIME strings         | 128 code units; further pattern/type checks apply.                                                                      |
| Sheet/column identifiers / row keys | 64 code units; further naming checks apply.                                                                             |
| Locus alternatives                  | 100 strings.                                                                                                            |
| Locus bindings                      | 9, with unique allowed fields.                                                                                          |
| Quiz correct answers                | 1–20 distinct nonempty comma-separated answers.                                                                         |
| JSON cell structure                 | 4,000 visited nodes, depth at most 24, bounded finite numbers.                                                          |
| Formula / binding expression        | 2,048 code units; formula tokenizer allows at most 512 tokens; parser depth at most 48.                                 |
| CSV parser safeguards               | 64 columns and 2,001 rows including header for normally terminated records; actual schema/document limits are stricter. |

`MiB` means `1024 * 1024` bytes. JavaScript string length uses UTF-16 code units; Python `len()` is not an exact substitute for strings containing non-BMP characters. Imported package limits do not guarantee a particular live backend transaction will fit every service/resource quota.

## 7. Error catalogue: messages, likely causes, and corrections

These are the explicit rejection families in the audited import path, plus propagated library/platform failures. Angle-bracket placeholders represent dynamic names. A backend error may be wrapped in a generic Convex error rather than displaying the exact underlying text.

### 7.1 File selection and ZIP container

| Message                                         | Cause / correction                                                                                                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Choose a .mnemodim file`                       | Filename does not end in `.mnemodim`. Use the correct extension on a real ZIP.                                                                                |
| `Package exceeds 100 MiB`                       | Selected/generated ZIP exceeds the compressed-size budget. Reduce or split content.                                                                           |
| `Invalid package size`                          | Input is under 22 bytes or over 100 MiB. Recreate a complete bounded ZIP.                                                                                     |
| `Not a ZIP package`                             | No valid ZIP end directory was found. Common cause: JSON renamed to `.mnemodim`, truncation, or extra incompatible trailing data.                             |
| `Split ZIP files are unsupported`               | Multi-disk/split archive. Produce one ordinary ZIP.                                                                                                           |
| `Invalid ZIP directory`                         | Empty/excessive entry count or inconsistent central-directory size/offset. Check the 1,000-entry limit and ZIP writer.                                        |
| `Invalid ZIP entry` / `Truncated ZIP directory` | Missing or malformed central-directory records. Rebuild from intact inputs.                                                                                   |
| `Unsafe or unsupported package path`            | Entry filename does not match allowed metadata/asset path patterns. Remove directory entries, OS metadata, unsupported suffixes, and extra enclosing folders. |
| `Unsafe package path`                           | Absolute path, empty component, traversal, or blocked path component. Generate paths from safe IDs.                                                           |
| `Duplicate ZIP path`                            | Two entries have the same filename. Deduplicate before writing.                                                                                               |
| `Encrypted/unsupported ZIP entry`               | Encryption or compression other than stored/DEFLATE. Change writer settings.                                                                                  |
| `ZIP entry exceeds size limit`                  | An entry's declared uncompressed size exceeds the asset or metadata budget. Reduce content; do not falsify sizes.                                             |
| `Expanded package is too large`                 | Sum of uncompressed entries exceeds 100 MiB. Compression alone cannot fix this.                                                                               |
| `Invalid local ZIP header`                      | Entry offset/header is missing or invalid. Rebuild the ZIP.                                                                                                   |
| `ZIP headers disagree`                          | Local filename or compressed-data bounds disagree with the central directory. Repair the writer, not just the manifest.                                       |
| `ZIP methods disagree`                          | Compression method or flags differ between local and central headers. Rebuild consistently.                                                                   |
| `ZIP sizes disagree`                            | Local and central sizes disagree where explicit sizes are required. Rebuild consistently.                                                                     |
| `Unexpected ZIP directory data`                 | Central-directory contents do not end at the expected location. Use ordinary supported ZIP output.                                                            |
| `Overlapping ZIP entries`                       | Entry byte ranges overlap. Treat as corrupt/unsafe and rebuild.                                                                                               |
| `Unexpected local ZIP entry`                    | Local entry is absent from the central directory or repeats. Rebuild the archive.                                                                             |
| `Uncompressed size mismatch`                    | Streaming decoder reports a size different from the preflighted entry. Rebuild from intact data.                                                              |
| `Expanded ZIP entry exceeds its declared size`  | Decompression produces more data than declared. Possible corruption or unsafe expansion; do not bypass the guard.                                             |
| `Corrupt package entry`                         | Decompressed length or CRC checksum differs. Recopy/regenerate the affected data or ZIP.                                                                      |
| `Incomplete package`                            | Not all declared entries were fully decoded. Usually truncation or malformed compressed data.                                                                 |
| Other decompressor errors                       | The ZIP library can report malformed DEFLATE streams directly. Regenerate with a standard writer; do not suppress the exception.                              |

### 7.2 Manifest and CSV layout

| Message                                                | Cause / correction                                                                                                                                                                                       |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Missing <path>`                                       | Required file is absent or stored under the wrong name/case/root. Include the exact expected path, including header-only stage/locus CSVs.                                                               |
| `Duplicate manifest path`                              | The manifest makes the decoder consume one file twice, e.g. repeated palace/sheet/asset paths. Make descriptors unique.                                                                                  |
| `Package contains unlisted files`                      | Files remain after all declared content is consumed. Remove extras or correctly declare real assets/tables.                                                                                              |
| `Unsupported CSV encoding`                             | `csvEncoding` is not `json-cells-v1`, or `sheets` is not an array. Correct the manifest, not just the filename.                                                                                          |
| `Unsupported manifest`                                 | Wrong format/version, invalid palace/assets arrays, or too many manifest palaces. Use the v1 shape and limits.                                                                                           |
| `Invalid palace ID`                                    | Manifest palace ID is missing, not a string, too long, or contains illegal characters.                                                                                                                   |
| `CSV palace scope mismatch`                            | Stage/locus CSV has a row whose `palaceId` differs from its containing palace. Correct the row or file placement.                                                                                        |
| `Too many sheets`                                      | More than 500 sheet descriptors. Consolidate/split the document.                                                                                                                                         |
| `Invalid sheet descriptor`                             | Sheet name/path is not a string, or descriptor shape is wrong.                                                                                                                                           |
| `Invalid sheet path`                                   | Descriptor path does not match its palace/shared scope and sheet name. Use the exact layout above.                                                                                                       |
| `CSV sheet scope mismatch`                             | Listed custom sheet is empty, or its rows disagree on sheet/scope with the descriptor. Remove empty custom sheets or fix the rows.                                                                       |
| `Malformed CSV quoting`                                | Quote appears in an illegal position, or characters follow a closed quote incorrectly. Use a CSV library.                                                                                                |
| `Unterminated CSV field`                               | An opening quote is never closed. Avoid manual escaping and truncated output.                                                                                                                            |
| `Unexpected CSV columns`                               | Header names/order differ, required optional-column headers are omitted, or a BOM/extra line disturbs the header. Use the exact lists above.                                                             |
| `CSV row width mismatch`                               | Data record has a different number of fields from the header. Preserve empty optional fields and quote commas/newlines.                                                                                  |
| `CSV column limit exceeded` / `CSV row limit exceeded` | Parser safety budgets exceeded, sometimes because broken quoting splits records incorrectly. Fix encoding or reduce size.                                                                                |
| JSON `SyntaxError`                                     | Invalid JSON in `manifest.json`, a nonempty CSV field, or a JSON cell. Use double-quoted JSON strings/keys, no trailing commas, and the correct JSON-inside-CSV layers. Exact message varies by runtime. |

### 7.3 Document fields, IDs, and parent relationships

| Message                                                                   | Cause / correction                                                                                                                                             |
| ------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Document metadata is too large`                                          | Reconstructed document exceeds 700,000 UTF-8 JSON bytes. Reduce/split metadata, not just ZIP compression.                                                      |
| `Expected an object`                                                      | An object position contains an array, primitive, or null. Supply the expected record shape.                                                                    |
| `Unknown document field`                                                  | Extra property on a validated document, palace, stage, locus, row, cell, binding, or asset. Remove invented/database-only fields; do not rename schema fields. |
| `Unsupported .mnemodim format/version`                                    | Shared document validator requires `format: "mnemodim"` and numeric `version: 1`. Earlier archive checks may report `Unsupported manifest` first.              |
| `Invalid package kind`                                                    | `kind` must be `palace` or `collection`.                                                                                                                       |
| `Invalid <text field>`                                                    | Required string missing/wrong type, too long, or containing NUL. Optional fields should be omitted rather than set to null.                                    |
| `Name is required` / `Palace name is required` / `Stage name is required` | Corresponding name is empty or whitespace-only.                                                                                                                |
| `Too many or invalid <list>`                                              | `palaces`, `stages`, `loci`, `rows`, `assets`, `cells`, `bindings`, or `alternatives` is not an array or exceeds its limit.                                    |
| `A palace package must contain exactly one palace`                        | No palaces, or multiple palaces with `kind: "palace"`. For multiple palaces use `collection`; an empty collection is also rejected.                            |
| `Invalid ID` / `Invalid or duplicate portable ID`                         | ID has wrong string form/length, fails the allowed-character pattern, or duplicates any portable entity ID. Prefix by entity kind.                             |
| `Entry palace must be a member`                                           | Entry ID does not identify an included palace.                                                                                                                 |
| `Unknown stage palace`                                                    | Stage's palace is missing from the document.                                                                                                                   |
| `Invalid locus parent`                                                    | Locus references a missing palace/stage or a stage belonging to another palace. Keep both parent references consistent.                                        |
| `Invalid stage order` / `Stage order must be an integer`                  | `orderIndex` is not a nonnegative safe integer. Use a JSON number, not a string.                                                                               |
| `Invalid x` / `Invalid y`                                                 | Coordinate is not a finite number in `[0,1]`. Convert pixels/percentages to normalized numbers.                                                                |
| `Invalid zoom`                                                            | Not a finite number in `[0.01,100]`.                                                                                                                           |
| `Invalid panX` / `Invalid panY`                                           | Not a finite number within ±`Number.MAX_SAFE_INTEGER`.                                                                                                         |
| `Invalid quiz answers`                                                    | Saved quiz memory has zero usable answers or more than 20 distinct comma-separated answers. Supply a valid fallback even if memory is formula-bound.           |
| `Unknown row scope`                                                       | Row `palaceId` is neither an included palace ID nor actual null.                                                                                               |
| `Duplicate scoped row`                                                    | Duplicate `(palaceId, sheet, key)`; references would be ambiguous.                                                                                             |
| `A table references another palace; export its collection instead`        | Typed palace cell points outside the package. Include the containing collection or correct the reference.                                                      |
| `A table references another stage; export its collection instead`         | Typed stage cell points outside the package. Include that stage and its palace.                                                                                |

The dynamic text-field labels include `name`, `palace name`, `background`, `thumbnail`, `stage name`, `stage image`, `label`, `memory`, `contentType`, `notes`, `imagePath`, `soundPath`, `description`, `quiz`, `alternative`, `sheet`, `row key`, `column`, `cell`, `expression`, `ID`, and `MIME`. An `Invalid ...` message identifies the validation label, not necessarily the exact JSON field spelling.

### 7.4 Workbook cells and bindings

| Message                                          | Cause / correction                                                                                                                                                                         |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `Invalid cell`                                   | `cell.value` is missing, non-string, over 16,000 code units, or contains NUL. For number/boolean/JSON cells serialize according to section 3. This is not the same as `Invalid cell type`. |
| `Invalid cell type`                              | Unsupported or incorrectly capitalized type. Use one of the eight exact types above.                                                                                                       |
| `Invalid numeric cell`                           | String is empty/whitespace, cannot convert to a finite number, or exceeds the safe-number magnitude.                                                                                       |
| `Boolean cells must be true or false`            | Value string is not exactly `true` or `false`.                                                                                                                                             |
| `JSON cell is too complex`                       | Parsed JSON exceeds 4,000 nodes or depth 24. Simplify/split the structure.                                                                                                                 |
| `Forbidden JSON key`                             | JSON object/structure contains `__proto__`, `prototype`, or `constructor`. Rename/remove unsafe keys.                                                                                      |
| `Invalid JSON number`                            | Parsed JSON number is nonfinite or outside the bounded number range. Avoid overflow and excessive magnitudes.                                                                              |
| `JSON cell is too large`                         | JSON helper has its own 16,000-code-unit limit. In normal package imports, the earlier cell-string check usually reports `Invalid cell` first.                                             |
| `Use an identifier for the sheet name`           | Sheet name violates the ASCII identifier pattern or uses a blocked name. Keep display text separate from identifiers.                                                                      |
| `Invalid row key`                                | Missing/non-string key, empty string, more than 64 code units, NUL, or blocked name. Preserve numeric-looking keys as strings.                                                             |
| `Invalid column` / `Invalid or duplicate column` | Column has invalid text/identifier form, uses a blocked name, or repeats in a row.                                                                                                         |
| `View is reserved for run state`                 | Saved row uses the runtime-owned `View` sheet. Choose a different sheet.                                                                                                                   |
| `Unknown binding field`                          | Field is not one of the nine supported binding targets.                                                                                                                                    |
| `Duplicate binding field`                        | Two bindings target the same field. Keep only one expression per target.                                                                                                                   |
| `Invalid expression`                             | Binding expression is missing/non-string, contains NUL, or exceeds 2,048 code units.                                                                                                       |

### 7.5 Formula syntax at confirmed import

Preview validates document structure, but the current backend parses formula cells and locus bindings only when the user confirms. Preflight them locally to avoid a late failure after media upload.

| Message                                                | Cause / correction                                                                                                                                                            |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Formula exceeds 2048 characters`                      | Formula cell is over 2,048 code units, even though ordinary cell text allows 16,000. Split/simplify the expression.                                                           |
| `Formula is too complex`                               | Token budget exceeded. Simplify the expression.                                                                                                                               |
| `Formula nesting limit exceeded`                       | Parser recursion depth exceeds 48. Flatten/split deeply nested expressions.                                                                                                   |
| `Unterminated string`                                  | Quoted formula string or escape is incomplete. Close quotes and escape correctly.                                                                                             |
| `Unexpected character '<character>'`                   | Unsupported syntax, e.g. `{` from a JavaScript object literal. Use the app's expression language.                                                                             |
| `Expected '<token>', got '<token>'`                    | Missing delimiter or unexpected trailing tokens. Match parentheses/brackets and separators.                                                                                   |
| `Expected a value, got '<token>'`                      | Missing operand/expression, e.g. `1+` or `bad(`.                                                                                                                              |
| `Forbidden identifier`                                 | A blocked identifier such as `constructor` occurs in the expression.                                                                                                          |
| `Invalid field`                                        | Invalid field name after a dot. Use valid identifiers or appropriate bracket syntax.                                                                                          |
| `Cannot read properties of undefined (reading 'text')` | Known diagnostic bug in the audited parser: `Inputs.` reproduces this instead of a helpful syntax error. Complete the field reference. This guide does not change the parser. |

Passing `parseFormula` does not check that a function exists or that a reference resolves. For example, `NoSuchFunction(1)` and `Missing.row` parse but may fail at runtime. Do not mislabel these as guaranteed import failures. Actions are not executed during import.

### 7.6 Embedded media

| Message                                               | Cause / correction                                                                                                                                                                                  |
| ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Invalid asset path`                                  | Manifest asset path is not exactly `assets/<id>`.                                                                                                                                                   |
| `Invalid MIME`                                        | MIME is missing/non-string, too long, or contains NUL.                                                                                                                                              |
| `Unsupported media type (use raster images or audio)` | MIME is not one of the supported types. Convert SVG/other formats rather than relabeling them.                                                                                                      |
| `Invalid asset size`                                  | Size is nonnumeric, nonfinite, fractional, zero/negative, or above 20 MiB. Use actual byte count.                                                                                                   |
| `Media budget exceeded`                               | Declared media total exceeds 100 MiB. Reduce/split assets.                                                                                                                                          |
| `Missing embedded asset: <reference>`                 | Media-bearing field or nonempty asset cell refers to an undeclared asset, URL, path, or color. Declare/embed it and reference its ID.                                                               |
| `Asset size mismatch`                                 | Actual embedded byte count differs from the manifest's `size`. Recalculate after any conversion.                                                                                                    |
| `Media signature does not match its MIME type`        | Binary signature does not match declared MIME, including HTML/error text downloaded as an image. Embed the real supported media bytes.                                                              |
| `Missing media <id>`                                  | The app's package encoder cannot find the declared binary or its size differs. Supply the matching entry in the encoder's media map. This is an authoring/export error, not a later upload failure. |

### 7.7 Upload, authorization, retry, and destination checks

A locally valid file can still fail here. These conditions usually require an account, network, destination, or integration fix rather than editing the archive.

| Message                                                                                                                | Cause / correction                                                                                                                                                                                                                                                                                       |
| ---------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Not authenticated`                                                                                                    | No valid session or tester allowlist denies the account. Sign in with an authorized account.                                                                                                                                                                                                             |
| `Collection not available`                                                                                             | Target collection was deleted, does not exist, or belongs to another user. Pick an owned destination.                                                                                                                                                                                                    |
| `Collection is full`                                                                                                   | Existing plus incoming palace count exceeds 25. Import separately or reduce membership.                                                                                                                                                                                                                  |
| `Combined collection exceeds document limits`                                                                          | Existing plus incoming stage/locus/row counts exceed workspace limits. Import separately.                                                                                                                                                                                                                |
| `Collection palace limit exceeded` / `Workbook row limit exceeded` / `Workspace exceeds the supported document limits` | Existing destination data is already beyond a supported loading limit. Repair/reduce the workspace before appending.                                                                                                                                                                                     |
| `Invalid collection membership`                                                                                        | Destination contains a palace not owned by the authorized user. Requires repairing membership, not changing incoming cell values.                                                                                                                                                                        |
| `Shared table keys conflict. Import as a separate collection or rename the conflicting source keys first.`             | Incoming shared sheet/key collides with an existing shared row. Import refuses overwrite. Use a separate collection or deliberately rename keys and update formulas/references.                                                                                                                          |
| `Could not upload <asset-id>`                                                                                          | Upload HTTP response failed or did not contain a valid storage ID. Check network/service state and retry.                                                                                                                                                                                                |
| `File not available`                                                                                                   | Upload belongs to another user, is not registered, or its claim is missing/expired/invalid. Claims last 10 minutes. A fresh file selection starts fresh uploads; do not reuse another user's IDs.                                                                                                        |
| `Media map mismatch`                                                                                                   | Number of uploaded media mappings differs from declared asset count. Fix the client/integration mapping.                                                                                                                                                                                                 |
| `Duplicate media mapping`                                                                                              | Same asset ID appears twice in the upload mapping. Deduplicate mappings.                                                                                                                                                                                                                                 |
| `Unknown media`                                                                                                        | Mapping names an asset not declared in the document.                                                                                                                                                                                                                                                     |
| `Uploaded media differs from the manifest`                                                                             | Stored upload is missing, or its size/MIME does not exactly match the declaration. Ensure bytes and upload Content-Type match the manifest.                                                                                                                                                              |
| `Invalid import identity`                                                                                              | Import key is not 1–128 ASCII letters/digits/underscore/hyphen, or digest is not 64 lowercase hex characters. Normal UI generates them automatically.                                                                                                                                                    |
| `Import identity was already used`                                                                                     | Retry key was reused with a different digest or an incompatible requested destination. Retry the same preview unchanged; use a new intentional import for different content.                                                                                                                             |
| `Package operation failed`                                                                                             | UI fallback when the thrown value is not an Error object. Inspect browser/backend diagnostics for the underlying reason.                                                                                                                                                                                 |
| Browser / network / Convex errors                                                                                      | File reads, Web Crypto availability, requests, backend argument/schema validation, resource limits, or service failures can throw their own messages. Use HTTPS/localhost for browser crypto and inspect the actual exception. No finite catalogue can enumerate every external-library/platform string. |

`Too many media links` also exists in a shared backend helper. Ordinary imports attach media to newly created rows, so existing-link overflow is not expected on that path; it is not a reason to delete media from a valid package speculatively.

### 7.8 Related authoring/export-only errors

Do not confuse these with failures importing an already downloaded file:

- `Too many package entries`: encoder would write more than 1,000 entries.
- `Cannot export an empty collection`: there is no palace to export.
- `Too many media assets`: snapshot references more than 200 distinct assets.
- `Referenced file is missing`: export could not resolve referenced media.
- `A table references another palace. Export the entire collection instead.`: single-palace export has typed links outside its scope.
- `Unsupported media URL`: export refuses the URL scheme/origin combination.
- `A referenced image or sound could not be downloaded. Export was cancelled rather than omitting it.`: download failed or had no response body.
- `Media exceeds 20 MiB` / `Package media exceeds 100 MiB`: downloaded media exceed export budgets.
- `Unsupported media. Export supports PNG, JPEG, WebP, GIF, AVIF and audio; convert SVG or other formats first.`: downloaded bytes have no supported signature.

Export shares document validation, so many document/cell errors above can appear while exporting as well.

## 8. Required preflight before handing a package to the user

### When the app repository is available

Preferred generation path:

1. Construct a `PalaceDocument` using the authoritative validators/types.
2. Construct a media map keyed by portable asset ID.
3. Call `validateDocument(document)` and parse every formula cell/locus binding.
4. Call `encodePackage(document, media)` instead of inventing a ZIP/CSV dialect.
5. Call `decodePackage(encodedBytes)`, save to a new `.mnemodim` file, then reopen the saved bytes and decode again.
6. Check actual formulas/actions in the intended workbook context separately; parsing is not evaluation.

This command validates an existing file locally using the repository's real decoder and formula parser. Run from the mnemodim repository root with its dependencies installed. It does not call Convex, upload media, or create a palace.

```bash
MNEMODIM_FILE="/absolute/path/to/palace.mnemodim" node --input-type=module <<'NODE'
import { readFile, stat } from 'node:fs/promises';
import { createServer } from 'vite';

const path = process.env.MNEMODIM_FILE;
if (!path || !path.toLowerCase().endsWith('.mnemodim')) {
  throw new Error('Set MNEMODIM_FILE to a .mnemodim path');
}
if ((await stat(path)).size > 100 * 1024 * 1024) {
  throw new Error('Package exceeds 100 MiB');
}
const server = await createServer({
  configFile: false,
  server: { middlewareMode: true },
  appType: 'custom',
  logLevel: 'error'
});
try {
  const { decodePackage } = await server.ssrLoadModule('/shared/package.ts');
  const { parseFormula } = await server.ssrLoadModule('/shared/formulas.ts');
  const opened = decodePackage(new Uint8Array(await readFile(path)));
  function checkFormula(expression, location) {
    try {
      parseFormula(expression);
    } catch (error) {
      throw new Error(`${location}: ${error.message}`);
    }
  }
  for (const locus of opened.document.loci) {
    for (const binding of locus.bindings ?? []) {
      checkFormula(binding.expression, `${locus.id}.bindings.${binding.field}`);
    }
  }
  for (const row of opened.document.rows) {
    for (const cell of row.cells) {
      if (cell.type === 'formula') {
        checkFormula(cell.value, `${row.id}: ${row.sheet}.${row.key}.${cell.column}`);
      }
    }
  }
  console.log('PASS: saved package integrity, document/media validation, formula syntax');
  console.log(JSON.stringify({
    palaces: opened.document.palaces.length,
    stages: opened.document.stages.length,
    loci: opened.document.loci.length,
    rows: opened.document.rows.length,
    assets: opened.document.assets.length
  }, null, 2));
} finally {
  await server.close();
}
NODE
```

If this fails with `Invalid cell`, inspect **every** workbook row/cell, not only the first one. Record its archive path, row ID, sheet/key, column, declared type, actual value type, and offending value. After correction, rerun the whole decoder; later media/formula errors may previously have been hidden.

### When only this guide is available in the other harness

- Follow the schema/serialization rules above and use an actual JSON/CSV/ZIP library.
- Add deterministic generator assertions for string cell values, allowed types, numeric/boolean/JSON content, limits, IDs, parent relationships, scope, references, sizes, and signatures.
- Reopen the final file. Parse each nonempty CSV data field as JSON and verify that every nested `cell.value` is still a string after decoding.
- Do not use an unbounded unzip operation on arbitrary untrusted packages as a substitute for the app's bounded integrity checks.
- Obtain access to the real validator or test through app preview before claiming compatibility. Preview alone still does not check formula syntax or the live backend conditions.
- This document is a hand-off, not an automatically installed skill update. The external skill must actually read/reference it and perform these checks on future output.

### Negative regression cases for a generator

A generation pipeline should catch all of these before delivery:

| Input mistake                                          | Expected rejection                                      |
| ------------------------------------------------------ | ------------------------------------------------------- |
| Number cell with native `1`                            | `Invalid cell`                                          |
| Boolean cell with native `true`                        | `Invalid cell`                                          |
| JSON cell with native object/array                     | `Invalid cell`                                          |
| Missing/null cell value                                | `Invalid cell`                                          |
| Cell string longer than 16,000 or containing NUL       | `Invalid cell`                                          |
| Number cell with `""` or `"seven"`                     | `Invalid numeric cell`                                  |
| Boolean cell with `"True"`                             | `Boolean cells must be true or false`                   |
| JSON cell containing `{a:1}`                           | JSON parse error                                        |
| Unknown cell type `integer`                            | `Invalid cell type`                                     |
| Stage `orderIndex: "0"`                                | `Invalid stage order`                                   |
| Locus `x: 50`                                          | `Invalid x`                                             |
| Unembedded media reference                             | `Missing embedded asset: ...`                           |
| Extra `README.md` or explicit directory entries in ZIP | Path/file validation error                              |
| Incomplete formula `1+`                                | Formula parser error, even if structural preview passes |

Also keep positive cases for number `"1"`, boolean `"true"`, JSON `"[1,2]"`, text `"007"`, multiline text, and non-ASCII learning content. A fix must not corrupt these valid cases.

## 9. What happens on failure and how to retry safely

1. **Preview:** browser reads and validates the ZIP, CSVs, document, references, and media. No uploads or palace writes occur. The Hiragana error happened here.
2. **Confirmation:** media upload/registration happens first. Backend checks auth, destination, identity, document, media mapping, and formula syntax.
3. **Commit:** metadata import is one atomic mutation. Palaces/stages/loci/rows/membership all commit, or none do. Already uploaded files can remain unattached after a failure or cancellation.
4. **Retry:** the same preview retains successful upload IDs and its import identity. Retry it unchanged after a transient failure. Re-selecting a file starts a new intentional copy; do not use this blindly after a possibly successful import if duplication matters.

The current UI displays the **first** thrown error, usually without a row/column path. It does not collect every invalid cell. The dedicated import ledger records successful commits, not every failed preview. A generic error does not prove the file has only one problem.

For repairs, leave the original untouched, write a separate `_fixed.mnemodim`, change only the diagnosed data, compare unchanged entries/media, reopen with the actual decoder, and report the validation scope. Do not change quiz/recall mode, mnemonic text, image/audio bytes, or IDs as a side effect of fixing string serialization.

## 10. Implementation references (optional, for maintainers)

These paths are relative to the mnemodim repository; the generator rules in this guide do not require access to these files, but running the authoritative validator does.

- `convex/documentValidators.ts`: portable and persisted shapes; `cell.value` is `v.string()`.
- `shared/document.ts`: string/numeric/list rules, limits, row/binding validation, IDs, parents, asset references.
- `shared/package.ts`: ZIP validation, media signatures, exact CSV columns, encoder/decoder.
- `shared/csv.ts`: JSON-inside-CSV encoding and parser.
- `shared/formulas.ts`: formula tokenizer, parser, and runtime (distinct validation stages).
- `shared/quizAnswers.ts`: comma-separated quiz rules and the 20-answer limit.
- `src/lib/services/packageService.ts`: browser preview, upload orchestration, retry identity, export.
- `src/lib/services/fileService.ts` and `convex/files.ts`: uploads, registration, 10-minute claims.
- `convex/packages.ts`: atomic import, ownership/media checks, destination conflicts, formula parsing, successful-import ledger.
- `convex/lib/workbookAccess.ts`, `convex/lib/authAccess.ts`, `convex/lib/storageUrls.ts`: workspace limits, account access, and media ownership.
- `src/lib/components/DocumentActions.svelte`: first-error display and preview/confirmation flow.
- `docs/MNEMODIM_FORMAT.md`: broader format/workbook/runtime documentation.
- `src/lib/packages.spec.ts`, `convex/packages.test.ts`: existing round-trip and rejection tests.
