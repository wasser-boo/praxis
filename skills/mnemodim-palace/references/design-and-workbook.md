# Locus design and workbook logic

Adapted from the Pi mnemodim-palace skill. Before authoring or serializing these
structures, read `MNEMODIM_IMPORT_GUIDE.md` and the format cheatsheet. The guide's
import contract takes precedence; this is workflow guidance, not executable code.

## Loci and visual placement

View/extract the stage image before selecting coordinates. If image understanding
is unavailable or the image is unclear, state that limitation; do not claim to
have seen anchors. Use enabled Praxis image-understanding tools if available.

- Coordinates x/y are normalized: x=0 left, x=1 right; y=0 top, y=1 bottom.
- Choose stable anchors: doors, lamps, corners, furniture, windows, shelves,
  stairs, signs or path turns. Distinct anchors support recall better than clusters.
- Prefer a consistent route: entrance-to-exit, clockwise, left-to-right,
  near-to-far, or an explicit room sequence. Explain its rationale briefly.
- Zoom 1.4–2.2 suits ordinary object loci. Higher zoom is for small details.
- Stage `imagePath` is the background; locus `imagePath` is the recall image;
  `palace.background` and `thumbnailImagePath` are palace-level imagery.
- `soundPath` is optional audio, generated only on explicit request.
- `contentType`: `recall`, `quiz` (multiple-choice/multi-answer), or `guess`
  (typed answer). Quiz correct answers live as comma-separated `memory`;
  distractors go in `alternatives`. A quiz needs 1–20 distinct nonempty answers
  even when memory is formula-bound. Do not invent an answer set. For new
  language/vocabulary palaces follow the app's `quiz` authoring convention;
  never silently convert existing valid `recall` content during a repair.

## Tables, formulas and bindings

Rows have stable `sheet` + `key` identity. Local rows shadow shared rows of the
same sheet/key. Numeric keys use brackets: `Major["07"].object`.
Every cell's stored `value` is a string: number `"7"`, boolean `"true"`, JSON
`"[1,2]"`. These become typed values at runtime, not in the serialized cell.
Shared rows require explicit `palaceId: null` (not a missing or string value).

Common sheets:

- `Inputs`: literal rows become run prompts. `$input1` means `Inputs.input1.value`.
- `Major`: number/object mappings. Required `object` column; optional `image`
  asset column. `Major(2,"487")` looks up groups `48`, then `7`.
- `Functions`: key is the function name, text `params`, formula `body`.
- `Actions`: buttons; `kind` can be `set`, `navigate`, `reveal`, `reset`.
- `Values`, `RAM` or custom sheets: calculated values or run-local state.

Formula operators: `+ - * / ^ mod %`, comparisons, `and/or/not`, `&&/||/!`,
arrays, field access and zero-based indexing. Built-ins: `IF/WENN`, `Number`,
`Integer`, `Text`, `Floor`, `Abs`, `Mod`, `Min`, `Max`, `Length`, `Digits`,
`Reverse`, `Concat`, `SetAt`, `Major`. There is no JavaScript eval, IO, network,
ambient globals, arbitrary recursion or formula side effect. Do not evaluate
package-supplied formulas as Python/JavaScript code.

Locus bindings can calculate `label`, `memory`, `description`, `imagePath`,
`x`, `y`, `zoom`, `visible`, `active`.

- `active` must be boolean; false excludes a locus from Explore/Journey.
- `visible` changes marker display only.
- Do not overwrite a bound field literally until its binding is deliberately
  removed or edited.
- Image bindings must resolve to known embedded/workspace media IDs, not URLs.

Validate stage/locus/palace ownership, every media reference, declared sheets,
formula syntax and actual importer behavior where available. The bundled helper
checks structural references but does not execute or type-check formulas.
