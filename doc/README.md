# Hyper

The repository ships an **AOT compiler** (`hyper run` / `hyper compile`) with a Rust-like dual backend (default **LLVM IR + clang**; **Cranelift** for objects / opt-in AOT) and this mdBook. Hyper is **compiler-only** — there is no interpreter. The compile path targets real small programs; the [full vision](overview/why-hyper.md) describes where Hyper is going.

## Quick start

```bash
cargo run -- run your_file.hyp
# same AOT path (temp exe + execute):
cargo run -- compile your_file.hyp
```

Build instructions, flags, and mdBook setup: [Building from source](building.md).

## How this book is organized

Prose lives under `doc/` and is published as this mdBook. **Runnable Hyper code** lives separately in `doc/examples/` — `.hyp` files only, with short comments, not long-form guides.

```text
doc/
├── readme.md                 Entry point (this page)
├── SUMMARY.md                mdBook sidebar / table of contents
├── building.md               Clone, build, CLI, mdBook
├── COMMIT_CONVENTION.md      Contributor commit prefixes
│
├── overview/                 Project goals
├── toolchain/                run vs compile today
├── langref/                  Written language reference (mirrors examples/)
├── compiler/                 Compile pipeline, support matrix, gaps
├── standard-library/         open, with, json, mmap
├── errors/                   Error kinds (prose reference)
└── examples/                 Syntax samples (.hyp only — runnable counterparts)
```

Browse chapters from the sidebar ([`SUMMARY.md`](SUMMARY.md)) or use the map below.

## Documentation map

| Section | Document | What you will find |
|:--------|:---------|:-------------------|
| Introduction | [readme.md](readme.md) | Orientation, layout, quick start |
| Building | [building.md](building.md) | Prerequisites, `cargo build`, CLI subcommands, mdBook |
| Why Hyper | [overview/why-hyper.md](overview/why-hyper.md) | Official vision: Python compat, speed, AI, safety |
| Dual backend | [toolchain/dual-backend.md](toolchain/dual-backend.md) | LLVM (default AOT) vs Cranelift (`--emit-obj` / opt-in) |
| Language reference | [langref/README.md](langref/README.md) | Written reference for every language topic (mirrors `examples/`) |
| Compiler overview | [compiler/overview.md](compiler/overview.md) | AST → IR → LLVM/Cranelift pipeline; flags |
| Supported features | [compiler/supported-features.md](compiler/supported-features.md) | Constructs lowered by `hyper compile` today |
| Known limitations | [compiler/known-limitations.md](compiler/known-limitations.md) | Unimplemented or partial compile paths |
| File handling | [standard-library/file-handling.md](standard-library/file-handling.md) | `open`, `with`, file methods, `open_mmap` |
| JSON module | [standard-library/json-module.md](standard-library/json-module.md) | `import json`; `loads`, `dumps`, `load`, `dump` |
| Error kinds | [errors/overview.md](errors/overview.md) | `SyntaxError`, `IndentationError`, `RuntimeError`; exit codes |
| Contributing | [COMMIT_CONVENTION.md](COMMIT_CONVENTION.md) | Commit message prefixes |

## Code samples (`doc/examples/`)

These directories contain **executable examples**, not markdown chapters. Each topic has a matching written page under [`langref/`](langref/README.md). Open `.hyp` files to run; open langref to read the rules.

| Topic | Path | Contents |
|:------|:-----|:---------|
| Variables | `examples/variable/` | Immutable and mutable `let` |
| Functions | `examples/function/` | Simple functions, strict types, `ref` |
| Operators | `examples/operator/` | Arithmetic, comparison, boolean, assignment |
| Conditionals | `examples/conditional/` | `if` / `elif` / `else`, ternary |
| Loops | `examples/loop/` | `for`, `while`; `advanced/` for `@parallel` / `@vectorize` |
| Collections | `examples/collection/` | Lists, arrays, dictionaries |
| Data types | `examples/data_type/` | Numbers, strings, booleans, `None` |
| Structs | `examples/struct/` | Objects, inheritance, traits (some aspirational) |
| Modules | `examples/module/` | `math.hyp`, `import.hyp` |
| File I/O | `examples/file_handling/` | `standard.hyp`, `json_io.hyp`, `mmap.hyp` sketch |
| I/O builtins | `examples/io/` | `print`, `input` |
| Error demos | `examples/errors/` | Programs that trigger each error kind when run |

Run a sample from the repository root:

```bash
cargo run -- run doc/examples/io/print.hyp
```

## Errors: reference vs runnable demos

Two locations serve different purposes. They are **not** duplicates.

| | `doc/errors/` | `doc/examples/errors/` |
|:--|:--------------|:-----------------------|
| **Format** | Markdown (this book) | `.hyp` source files |
| **Purpose** | Explain error kinds, messages, exit codes, `run` vs `compile` | Reproduce errors in the terminal |
| **Typical use** | Read before writing tests or CI checks | `hyper run doc/examples/errors/runtime_error.hyp` |

Full reference: [Error kinds](errors/overview.md).
