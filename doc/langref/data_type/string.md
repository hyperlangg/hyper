# Strings

Strings are text values written in double quotes. The type name is `string`.

```hyper
let name: string = "Hyper"
let mut greeting: string = "Hello, World!"
```

Immutable bindings hold a fixed string; `let mut` allows reassignment (including growing via concatenation such as `s = s + "x"`). F-strings are supported on the compile path for interpolation.

## Operations and methods

- Concatenation uses `+`. Store-back forms like `s = s + "…"` reclaim the previous owned string on the compile path (see [String concat stress](../loop/str-concat-stress.md)).
- Length: method `s.len()` or builtin `len(s)`.
- A full Python-compatible method set is available on the compile path (`upper`, `lower`, `strip`, `split`, `replace`, `find`, …). AOT case transforms cover ASCII, Latin-1, Latin Extended-A, Cyrillic, Greek, and the `ß` / `İ` / `ı` expansions — not a full Unicode case-folding database.

## Notes

- Prefer `string` in annotations; literals infer as strings.
- User-facing output goes through `print`; diagnostics go to stderr via the runtime, not a logging API.

## Example

Runnable sample: [`examples/data_type/string.hyp`](../../examples/data_type/string.hyp)
