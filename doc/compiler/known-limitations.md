# Compiler known limitations

Hyper v0.1 targets a **working compiler for core programs**, not full language parity. When a construct is unsupported, lowering reports a **`SyntaxError`** with a line number before codegen starts (multiple errors collected in one pass when possible).

## vs CPython weaknesses

Hyper is aimed at fixing CPython’s classic bottlenecks. Status today:

| CPython weakness | Hyper today |
|------------------|-------------|
| **GIL** (one bytecode thread at a time) | **No GIL** — Hyper is AOT native code, not a bytecode VM. `@parallel` for-loops whose body only uses the induction variable (plus callees like `print`) are outlined and run on an **OS thread pool** via `hyper_rt_parallel_for`. Bodies that capture/mutate outer locals still lower sequentially (same per-index results). |
| **Heavy memory from dynamic objects** | **Partial** — no interpreter object model; values are compact tagged `payload`+`kind` pairs and native heaps. Still not fully unboxed C/Rust layouts for every local. Further ABI specialization is planned. |
| **Type errors only at runtime** | **Mostly fixed for annotated / known types** — `typecheck` is **fatal** before codegen on `run` / `compile` (exit 65). Struct field reads/writes check field existence, mutability, and types. Untyped/`Any` holes and some dynamic ops can still fail at runtime (exit 70). Prefer annotations for the strongest guarantees. |

## Behavioral notes

| Construct | Compiler behavior |
|-----------|-------------------|
| `@parallel` on `for` | **Threaded** when the body is outlineable (induction variable + callees only). Otherwise sequential with the same per-index semantics. |
| `@vectorize` on `for` | SIMD-oriented hint; still runs every index sequentially until SIMD/GPU backends land. |
| Type errors | **Fatal** under `run` and `compile` before codegen |

## Loop control flow

`break` and `continue` bind to the innermost enclosing `while` / `for` / `for-in` loop. Two cases are rejected:

- Outside any loop — `SyntaxError: line N: break outside loop` from the type checker (fatal before codegen).
- Inside a `@parallel` / `@parallel @vectorize` `for` body, where iterations are split across threads and an early exit has no single meaning.

A function body does not inherit the loop around its declaration, so a `break` inside a nested `fn` is an error.

## Language features not implemented (any backend)

- Generics (`make_it_speak[T: Speaker]` in docs is aspirational)
- Production GPU / SIMD codegen for `@vectorize`
- Full reclaim of every temporary string on the compile path (containers free overwritten elements; file/mmap handles free on close)
- `try` / `except` — Hyper uses explicit `raise` / `raises` / `handle` instead (see [Errors](../errors/overview.md))

## String methods

String methods share one runtime on **`run` and `compile`**. `split()` / `rsplit()` with no separator follow Python whitespace rules. `--emit-exe` case transforms are ASCII-oriented in the C runtime; the Rust reference runtime uses full Unicode case mapping.

## Struct method resolution

The compiler must know the struct type at the call site. It follows:

- Constructor assignments (`let p = Point(...)`)
- Field access chains
- Annotated parameters and return types
- Function return tracking

If a method call fails to resolve, you get a compile error naming the missing field or method — add an annotation or restructure so the type is known earlier.

Field **types**, **mutability**, and **existence** are checked when the receiver’s struct type is known.

## Error message format

All diagnostics use:

```text
SyntaxError: line N: …
IndentationError: line N: …
RuntimeError: line N: …
```

User programs should use **`print()`** only — errors go to stderr through the runtime, not via language-level logging APIs.
