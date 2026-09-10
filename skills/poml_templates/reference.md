# POML template-authoring reference

Source checked: https://microsoft.github.io/poml/stable/language/template/
Basic syntax: https://microsoft.github.io/poml/stable/language/basic/
Component reference: https://microsoft.github.io/poml/stable/language/components/
The installed Microsoft CLI is the compatibility authority; test it, not an XML parser alone.

## Microsoft template syntax

- One `<poml>...</poml>` root is recommended. Useful prompt components include `<role>`, `<task>`, `<cp caption="...">`, `<p>`, `<list>`, `<item>`, `<code lang="...">`, and `<output-format>`.
- `{{name}}`, `{{a + b}}`, `{{object.field}}`, `{{items[0]}}`, function calls and ternaries are **JavaScript expressions**. Attribute interpolation works too: `caption="Task #{{index}}"`.
- `<let name="greeting">Hello!</let>` defines literal text. `<let name="greeting" value="'Hello!'" />` evaluates JavaScript: the inner quotes are required for a string literal. `value="10"` is a number. `value="{{ base + increment }}"` is also supported.
- Inline JSON in a `<let>` can define objects/arrays; `type="integer"`, `type="boolean"` and other documented types control casting.
- `<let name="config" src="defaults.json" />` imports JSON. Paths are relative to the POML file. For the tested Praxis CLI, set `type="string"` for verbatim source/Markdown, including a `.poml` file you want to SHOW rather than execute. Compatibility caveat: the stable documentation mentions `text`/`json`/`csv` import types, but this installed CLI rejects `type="text"`; use `string` for literal text and omit the type for auto-detected JSON (or use `object`/`array`). An unnamed JSON-object import adds its properties to the current context; prefer named imports to avoid collisions. Never import credentials or private configuration.
- `<item for="item in items">{{item}}</item>` iterates a list. `loop.index` starts at **0**; `loop.length`, `loop.first`, and `loop.last` are available in the loop. Use `{{loop.index + 1}}` for human numbering.
- `<p if="showDetails">Details</p>` conditionally renders. `if` and `for` take expressions, not Jinja blocks. For a branch, use another element with the opposite condition; do not invent `<else>`.
- `<include src="snippet.poml" />` injects another POML file with the current variables available. Both `if` and `for` work on includes: `<include src="row.poml" for="i in [1,2,3]" />`. Keep includes local, relative and acyclic. Save dependencies before validating the caller.
- Component boolean/number/object attributes auto-cast according to that component's documented type. `syntax` controls output formatting, not the code language (`lang`).
- POML is XML-like, not generic XML. Microsoft documents escapes such as `#lt;`, `#gt;`, `#quot;`, `#amp;`, `#lbrace;` and `#rbrace;`; do not assume HTML entity or CDATA behavior. For long literal code examples, importing text and interpolating the resulting string avoids accidental evaluation of nested braces/tags.

## Missing, null and empty inputs

A missing identifier throws even inside `if="user_prompt"`. Use a local default:

```xml
<poml>
  <let name="request_text" value="typeof user_prompt === 'string' ? user_prompt : ''" />
  <let name="items" value="typeof custom_data === 'undefined' || custom_data === null ? [] : (Array.isArray(custom_data.items) ? custom_data.items : [])" />
  <task>Answer accurately and concisely.</task>
  <p if="request_text">{{request_text}}</p>
  <p if="!request_text">Use the latest conversation message.</p>
  <list><item for="item in items">{{loop.index + 1}}. {{item}}</item></list>
  <output-format>Use short paragraphs.</output-format>
</poml>
```

`typeof` guards the identifier itself; optional chaining alone does not protect an undeclared identifier. Define locals before using them. Rendering input strings containing `{{...}}` must preserve those strings as data, not recursively evaluate them.

## Praxis contracts

- Native skills are `skills/NAME/skill.json` plus `skill.poml`, not Pi's `SKILL.md` format. Call `use_skill` with `{"name":"poml_templates","parameters":{"user_request":"Create a concise tutor template"}}`.
- Skill parameters become the top-level render variables. Only the explicit parameters are passed: stored contexts/secrets are not implicitly injected. `required_parameters` declares required non-empty string inputs.
- Both system/user runtime paths and preview share current `user_prompt`/`user_message`, canonical `settings`, `sm_file`, `active_state`, `skills`, enabled `tools`, user-scoped `memory`, `custom_data`, unchanged `cl_data`, identity and token/runtime metadata. `custom_data.user_prompt` is refreshed before SM routing. See `docs/CONTEXT_VARIABLES.md` for the complete contract and `examples/poml-test-context.json` for synthetic fixtures. Guard optional identifiers so isolated renders still work.
- Dedicated compaction receives `conversation_text`. Arbitrary settings are not flattened into new top-level identifiers. Template expressions do not persist state: use normal context tools for that.
- Default profile: `templates/standard.poml`. Reuse `shared/runtime.poml` to advertise skills, enabled tools, memory and current input; reuse `shared/blueprint.poml` for a semantic task contract. Adjust relative paths for subdirectories. Blueprint data defines scene_goal, participants, action_to_complete (predicate, agent, patient), constraints, success_criteria and output_format. Use concise public plans/results, not exposed private chain-of-thought.
- `contexts/standard.sm` routes explicit role-change requests before rendering. `settings.sm_file` overrides root `sm_file`; `.sm` is canonical, `.cl` and `cl_file` inputs are legacy-compatible. Do not rename `cl_data`. Persona names/regexes belong in editable SM files, not Rust. A manual `settings.system_template` persists unless an SM rule overrides it.
- Discord `/skill skillname:poml_templates` persists skill instructions for subsequent tasks; `off` clears. Loading/selection never executes scripts or bypasses disabled use_skill. Runtime appends active instructions separately; do not duplicate them in ordinary system templates.
- `update_template` arguments: `{"name":"tasks/my_prompt","content":"<poml>...</poml>","context":{}}`. An explicit `context` is the complete validation context; omit it for synthetic system defaults. The name allows letters/numbers/underscore/hyphen and `/` separators; no `.poml`, absolute paths, traversal or symlinked subdirectories.
- Template selection is `settings.system_template = "tasks/my_prompt"`. Do not confuse it with `settings.active_templates`/`current_template`. Creation must not silently switch the user's current template.
- `update_template` validates a temporary sibling file and only then replaces the destination. A read-only Node overlay ensures imports of the final path see the prospective content, not the old template, so active direct/transitive self-includes fail validation. Failed validation leaves any old file unchanged. File persistence and the subsequent DB mirror are not a cross-storage transaction: a DB failure is reported explicitly.

## Real validation

From the Praxis working directory, with Node and Microsoft's built JavaScript CLI installed:

```bash
export POML_CLI=/absolute/path/to/poml/js/cli.js
python3 skills/poml_templates/scripts/validate.py templates/tasks/my_prompt.poml \
  --context-file /tmp/empty.json --context-file /tmp/populated.json
# Equivalent single-case call:
node "$POML_CLI" --file templates/tasks/my_prompt.poml \
  --context-file /tmp/populated.json --strict --speakerMode=false
```

The JSON context files must contain objects. The helper prints the rendered output and fails if the CLI is missing, the render fails or output is empty. It does not call an LLM, modify templates, read .env, or activate anything. Use synthetic inputs, not live context dumps containing private data. Test includes from a different working directory too.

Before claiming success, inspect the actual rendered text for required instructions, correct substitutions, loop order, and included/excluded conditional content. Fallback string substitution, mocked output and HTTP reachability do not prove a template renders.
