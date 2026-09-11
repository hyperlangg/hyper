# Compiler supported features

This list reflects what **`hyper compile`** / **`hyper run`** can lower today (AOT). For syntax samples see `doc/examples/`.

## Language constructs

- Variables: `let`, `let mut`, typed bindings (`name: Type = …`, `Array[T]`, `Dict[K, V]`)
- Functions: `fn` / `def`, parameters including `ref` (mutable binding; struct instances share field storage)
- Control flow: `if` / `elif` / `else`, `while`, `for` / `for-in`, `break` / `continue`, ternary `a if cond else b`
- Error flow: `raise`, `raises` on functions, `handle attempt else fallback` (no `try` / `except`)
- Operators: arithmetic (`+`, `-`, `*`, `/`, `//`, `%`, `**`), comparisons, `and` / `or`, compound assignment
- Literals: integers, floats, strings, f-strings, lists, dicts, `None`, booleans
- Structs: fields with `pub` / `mut`, methods, `__init__`, field get/set; traits (method name + arity)
- Modules: `import m`, `import m as alias`, `from m import name`
- Decorators: `@parallel` (OS thread pool when body is outlineable; otherwise sequential), `@vectorize` on `for` (sequential today; same per-index results)

## Builtins and standard library (compile path)

| Feature | Notes |
|---------|--------|
| `print(...)` | Variadic |
| `open(path, mode?)` | Buffered file handle |
| `with open(...) as f:` | Auto-close; file methods |
| File methods | `read`, `readline`, `readlines`, `write`, `seek`, `tell`, `size`, `flush`, `close`, … |
| `with open_mmap(path) as m:` | `read_chunk(offset, size)` |
| `input(prompt?)` | Stdin line read |
| `clock()` | Seconds since UNIX epoch (`f64`) |
| Collection methods | list/array `len()`, `append(x)`; dict `len()`, `keys()`; string `len()` |
| dict get/set | Hash map (open addressing in AOT C runtime); insertion order kept |
| Builtins | `len`, `abs`, `min`/`max`, `sum`, `round`, `pow`, `divmod`, `chr`/`ord`, `bin`/`hex`/`oct`, `int`/`float`/`str`/`bool`, `all`/`any`, `sorted`, `reversed`, `range`, `enumerate` |
| String methods | Full Python-compatible set on compile path: `upper`/`lower`/`capitalize`/`title`/`swapcase`, `strip`/`lstrip`/`rstrip`, `startswith`/`endswith`, `split`/`rsplit`, `replace`, `join`, `find`/`rfind`/`index`/`rindex`, `count`, `isdigit`/`isalpha`/`isalnum`/`isspace`/`islower`/`isupper`/`istitle`/`isascii`, `center`/`ljust`/`rjust`/`zfill`, `removeprefix`/`removesuffix`, `partition`/`rpartition` |
| `import json` | `loads`, `dumps`, `load`, `dump` |

Integer `/`, `%`, `//` guard division by zero at runtime.

## Codegen modes

- AOT run via temp executable (`hyper run` / `hyper compile`)
- Cranelift object emission (`--emit-obj`) — Hyper-IR → machine object
- Executable linking with C runtime (`--emit-exe`); linker prefers LLVM `clang`

## CI-verified programs

| Program | What it checks |
|---------|----------------|
| `ci/smoke/smoke.hyp` | Core language; `run` / `compile` / `--emit-exe` output parity |
| `ci/control/divzero.hyp` | `RuntimeError` exit code 70 |
| `ci/io/io_compile.hyp` | File I/O on compile path |
| `ci/io/json_compile.hyp` | JSON module on compile path |
| `ci/io/mmap_compile.hyp` | Memory-mapped files on compile path |
| `ci/io/input_compile.hyp` | `input()` on compile path |
| `ci/io/clock_compile.hyp` | `clock()` on compile path |
| `ci/collections/collections_compile.hyp` | list/array/dict `len`, `append`, `keys` on compile path |
| `ci/collections/dict_compile.hyp` | 256-key dict get/set, overwrite, insertion-order print/keys() |
| `ci/collections/builtins_compile.hyp` | Builtins (`len`/`abs`/`enumerate`/`zip`/`range`/…) on compile path |
| `ci/collections/strings_compile.hyp` | string methods on compile path |
| `ci/control/break_continue.hyp` | `break` / `continue` in `while`, `for` and `for-in`; output parity |
| `ci/control/raise_handle.hyp` | `raise` / `raises` / `handle` on run and compile |
| `ci/lang/traits_compile.hyp` | Trait conformance on compile path |
| `ci/lang/pub_mut.hyp` | `pub` / `mut` field rules on compile path |
| `ci/lang/ref_compile.hyp` | `ref` + shared struct fields on compile path |
| `ci/lang/vectorize_compile.hyp` | `@vectorize` / `@parallel` compile |

## Loop control flow

`break` and `continue` lower on the compile path for every loop form. `while` sends `continue` back to the condition header; `for` and `for-in` route it through a dedicated increment block so the induction variable still advances. Both are rejected outside a loop and inside a `@parallel` `for` body — see [Known limitations](known-limitations.md).

## Not compiled (see limitations)

Generics, list/dict shared `ref` payloads, production GPU/SIMD for `@vectorize`, Python library interop — [Known limitations](known-limitations.md).
