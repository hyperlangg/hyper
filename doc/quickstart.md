# Quickstart

Hyper is **compiler-only** and **AOT-only**. `run` and `compile` lower to LLVM IR, link the C runtime with `clang`, run a temporary executable, and delete it.

You need [Rust](https://www.rust-lang.org/tools/install) (stable), Git, and `clang`. On Windows, [LLVM](https://releases.llvm.org/) supplies `clang`; WSL is optional. Linux, macOS, and native Windows are all first-class. Full platform notes: [Building from source](building.md).

Save this as `hello.hyp`:

```hyper
fn greet(name):
    return f"Hello, {name}"

print(greet("Hyper"))

let mut n = 0
while n < 3:
    print(n)
    n += 1
```

Clone, build, and run — every command is in this one block:

```bash
git clone https://github.com/hyperlangg/hyper.git
cd hyper
cargo build --release
cargo run --release -- run hello.hyp
cargo run --release -- compile hello.hyp
cargo run --release -- typecheck hello.hyp
cargo run --release -- compile hello.hyp --emit-ir
cargo run --release -- compile hello.hyp --emit-llvm out.ll
cargo run --release -- compile hello.hyp --emit-obj out.o
cargo run --release -- compile hello.hyp --emit-exe hello
cargo run --release -- run hello.hyp --backend cranelift
```

| Command | What it does |
|---------|----------------|
| `run` / `compile` | Default AOT: LLVM IR + `clang`, then execute. `compile` with no emit flag is the same path as `run`. |
| `typecheck` | Types only. No codegen. Fatal type errors exit **65**. |
| `--emit-ir` | Print Hyper IR. Does not link. |
| `--emit-llvm` | Write LLVM IR (default `out.ll`). Does not link. |
| `--emit-obj` | Always a **Cranelift** object (default `a.o`), even if the selected backend is LLVM. |
| `--emit-exe` | Keep a standalone binary (default `hyper_out`; `.exe` is added on Windows). |
| `--backend cranelift` | Same `run` / `compile` / `--emit-exe` path, but Cranelift instead of LLVM. Also `HYPER_CODEGEN=cranelift`. Does not need `clang`. |

Type errors stop before codegen. A runtime failure (for example division by zero that is not a literal) exits **70**. After `cargo build --release`, the binary is `target/release/hyper` (`hyper.exe` on Windows) if you prefer not to use `cargo run`.

Next: [Building from source](building.md) for prerequisites and Windows/MSVC notes, and [Why Hyper](overview/why-hyper.md) for the product description.
