# Building from source

Hyper **v0.1.0** is the first public release. Clone the repository and build the reference toolchain with Rust. See [CHANGELOG.md](../CHANGELOG.md) for what shipped.

Everything lives in a **single Cargo package** (`hyper`). The `src/` tree holds the language frontend and compiler:

| Module / path | Role |
|---------------|------|
| `scanner.rs`, `parser.rs`, `ast.rs`, `driver.rs` | Lexer, parser, AST, program driver |
| `semantic.rs` | Type checker |
| `environment.rs` | Host `HyperValue` bridge for JSON (not an execution backend) |
| `fileio.rs`, `json.rs`, `module.rs` | Shared I/O / JSON / module resolution used by the compile runtime |
| `compiler/` (`ir`, `lowering`, `codegen`, `llvm_emit`, `runtime`) | IR, LLVM + Cranelift AOT codegen, C runtime for linking |
| `main.rs` | CLI (`tokenize`, `parse`, `run`, `typecheck`, `compile`, …) |

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable) — `cargo` + `rustc`
- Git
- **C toolchain (required for `run` / `compile` / `--emit-exe`):** Hyper is AOT-only. Everyday execution emits a temporary executable and links the C runtime, so a host linker is required even when you are not keeping an `--emit-exe` artifact.

Hyper targets **Linux, macOS, and Windows** equally. WSL is **not** required on Windows.

### C toolchain

| Platform | Typical compilers |
|----------|-------------------|
| Linux | `clang` (**required** for default LLVM AOT); `gcc`/`cc` OK for Cranelift AOT |
| macOS | `clang` via Xcode Command Line Tools |
| Windows | `clang` / `clang-cl` for LLVM AOT; MinGW `gcc` or MSVC `cl` also work for Cranelift AOT |

Override the linker/compiler with `CC` where applicable. Select the AOT backend with `HYPER_CODEGEN=llvm|cranelift` or `--backend llvm|cranelift` (default **llvm**). See [Dual backends](toolchain/dual-backend.md).

On Windows, install any one of: [LLVM](https://releases.llvm.org/) (clang), [MinGW-w64](https://www.mingw-w64.org/), or [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the C++ workload. Then `hyper run`, `hyper compile`, and `hyper compile file.hyp --emit-exe app` work natively (Hyper adds `.exe` when needed).

## Clone and build

```bash
git clone https://github.com/muhammadyusufpov/hyper.git
cd hyper
cargo build
```

Debug binary:

```text
target/debug/hyper
```

Release binary:

```bash
cargo build --release
# target/release/hyper
```

## Run a program

Hyper is **compiler-only** and **AOT-only**. By default, `run` / `compile` lower to **LLVM IR**, invoke **clang** with the C runtime, run the temp binary, and delete it. Opt into Cranelift with `--backend cranelift` or `HYPER_CODEGEN=cranelift`.

```bash
cargo run -- run your_file.hyp
cargo run -- compile your_file.hyp
cargo run -- run your_file.hyp --backend cranelift
# or: HYPER_CODEGEN=cranelift cargo run -- run your_file.hyp
```

**Compiler (dump IR / emit artifacts):**

```bash
cargo run -- compile your_file.hyp --emit-ir
cargo run -- compile your_file.hyp --emit-llvm out.ll  # LLVM IR
cargo run -- compile your_file.hyp --emit-obj out.o    # Cranelift object
cargo run -- compile your_file.hyp --emit-exe my_app   # keep AOT binary
```

## Quick sanity check

```bash
cargo run -- run ci/smoke/smoke.hyp
cargo run -- compile ci/smoke/smoke.hyp
```

Both should finish without syntax errors and print the same output (both AOT-execute).

## Docs site (optional)

Documentation is built with [mdBook](https://rust-lang.github.io/mdBook/):

```bash
cargo install mdbook
mdbook serve --open
```

Open the URL printed by `mdbook serve` (usually `http://localhost:3000`).

To build static HTML into `book/`:

```bash
mdbook build
```

## What is not supported yet

Hyper is under active development. See [Compiler known limitations](compiler/known-limitations.md) for remaining gaps (generics, GPU/SIMD `@vectorize`, and related items).

Unsupported constructs are reported as **`SyntaxError: line N: …`** (or `IndentationError` / `RuntimeError` at runtime) before code generation starts when possible, and the compiler collects multiple lowering errors in one pass instead of stopping at the first one.

The compiler resolves struct methods statically — see [Compiler supported features](compiler/supported-features.md).

`run` and `compile` both enforce type errors before codegen.

There are no published packages or installers — building from source is the only supported way to get the toolchain today.
