# The `.herald` rule DSL

Herald rules are plain-text `.herald` files. The goal: **readable by a beginner,
yet zero false positives.** A rule has three parts: `meta`, an engine block
(`match` / `measure` / `use`), and `tests`.

```
rule <id> {
  meta  { type: …  severity: …  message: "…" }
  match { <langs>: <shape> … }      # the engine block (see below)
  when  <conditions>                # optional, refines match
  tests { <lang> { "<code>" => fail | pass } … }
}
```

`meta.type`: `security` | `code-smell`. `meta.severity`: `error` | `warning` | `info`.

The legacy header form `rule <id> [<category>/<severity>] { message "…" … }` is
still accepted, as is `examples { flag/ok … }` instead of `tests`.

## 1. `match` / `when` — pattern by example (the common case)

Write the **shape** of the dangerous code, using `$VARS` as holes, then refine
with `when` conditions. Herald turns the shape into a precise AST query; the
`when` conditions keep it free of false positives.

```
rule command_injection [security/error] {
  message "Shell command built from dynamic input"
  when $ARG is dynamic
  match js, ts: exec($ARG)
  match py:     os.system($ARG)
}
```

- `match <langs>: <shape>` — one shape per language. Repeat `match` for more
  shapes / languages. A bare `match: <shape>` (no languages) is tried against
  every grammar.
- `match <langs>: /<regex>/` — regex form: match against **identifiers and
  string literals only** (never comments or raw text).
- `when` is written once and applies to every `match` of the rule.
- `find` (and the brace form `find <langs> { <code> }`) is still accepted as an
  alias, but `match` is the preferred keyword.
- `$NAME` is a metavariable: a hole that matches any expression and is captured
  under `NAME` so `when` can talk about it. Concrete tokens (`exec`, `.query`)
  must match literally.

### `when` conditions

Combine with `and` / `or` / `not` and parentheses.

| condition | true when the captured node… |
|-----------|------------------------------|
| `$x is dynamic`     | is built at runtime: concatenation, template/f-string interpolation, `format`/`Sprintf` |
| `$x is constant`    | is a fixed literal (string/number/bool, no interpolation) — not a variable, call, or env lookup |
| `$x is user-input`  | reads request/CLI input (`req.query`, `request.args`, `argv`, `getParameter`, …) |
| `$x matches /re/`   | its text matches the regex |
| `$x ~ /re/` / `!~`  | same as `matches` / negated |
| `entropy($x) > 4`   | Shannon entropy over the threshold (also `len`, `lines`) |
| `placeholder($x)`   | looks like a placeholder/env ref (`changeme`, `process.env.*`, `<...>`) |

```
rule hardcoded_secret [security/error] {
  message "Hardcoded secret"
  when $KEY matches /(?i)(secret|token|api[_-]?key)/
    and $VAL is literal
    and entropy($VAL) > 3.5
  match js, ts: const $KEY = $VAL
  match py:     $KEY = $VAL
}
```

## 2. `measure` — counting metrics

```
rule function-too-long [code-smell/warning] {
  message "Function too long ({value} > {threshold} lines)"
  measure function_lines > 50 (java 70, go 60)
}
```

Keys: `function_lines`, `params`, `complexity`, `nesting`, `class_methods`,
`file_lines`, `line_length`. The `(lang N, …)` list overrides per language.

## 3. `use` — built-in analyzers

For checks whose logic lives in Rust (cross-file, frequency maps, flow):

```
rule xxe [security/error] { use xxe }
```

## 4. `examples` — inline tests

`flag` = must be detected, `ok` = must NOT be detected (the zero-FP proof). Each
example is a single line. Run them with `herald test [filter]`.

```
examples {
  flag js: exec("ls " + dir)
  ok   js: exec("ls -la")
}
```

## Legacy precise form

The original explicit form is still accepted for rules that need full
tree-sitter control (`lang … { pattern (…) @match where @match ~ /…/ }`,
`metric` + `threshold`). Prefer `match`/`when` for new rules.

Comments: `#` or `//` to end of line.
