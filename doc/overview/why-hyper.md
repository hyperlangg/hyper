# Why Hyper

Hyper is a **compiled** programming language for teams that want **Python’s ergonomics** with **systems-level speed**, **hardware utilization**, and a toolchain aimed at **AI and large-scale data**.

This page is Hyper’s **official product description**. Implementation details and the current v0.1 gap list live in [First release scope](first-release-scope.md) and [Known limitations](../compiler/known-limitations.md).

## Full Python compatibility (design goal)

Hyper’s syntax is **very close to Python**: indentation-based blocks, familiar operators, modules, and collection literals. The long-term goal is **full compatibility with the Python surface** so that:

- Existing Python scripts can be ported with minimal edits.
- Python ecosystems — including libraries such as **NumPy** — can run in the **Hyper environment** without rewriting the mental model.

v0.1 does not claim every Python feature or every third-party wheel yet; the **direction** is unambiguous: Hyper should feel like Python that compiles to native code.

## Maximum speed and efficiency

Hyper targets **C and C++-grade memory discipline** and **direct use of hardware**:

- **CPU:** native AOT code via Cranelift object codegen and clang/host linking, buffered I/O, and low runtime overhead.
- **GPU and SIMD:** the language surface includes `@vectorize` and parallel loop forms; codegen for real GPU backends is on the roadmap, with sequential lowering today where parallelism is not yet emitted.

Programs that spend time in tight numeric loops and data pipelines are expected to run **10×–100× faster** than equivalent CPython — the range depends on workload, but that order of magnitude is the design target, not an afterthought.

## Built for artificial intelligence

Hyper is **purpose-built for AI workloads**:

- Training and inference pipelines that stress memory bandwidth and CPU/GPU throughput.
- Large files and datasets (`open`, `open_mmap`, JSON) without paying per-op interpreter cost — Hyper executes compiled code end-to-end.
- A toolchain that will grow toward tensor-friendly builtins and accelerator integration.

If your work involves **neural networks**, **batch processing**, or **multi-terabyte data**, Hyper is meant to be the language layer — not a slow glue script around native libraries.

## Security and modern architecture

Hyper’s implementation is **Rust-hosted** and follows a **memory-safe** systems style:

- Clear error kinds (`SyntaxError`, `IndentationError`, `RuntimeError`) instead of silent corruption.
- A compiled runtime with explicit value kinds and bounded buffers for I/O.
- **No GIL** (native AOT, not a bytecode VM). **`@parallel`** outlines eligible range loops onto an OS thread pool (`hyper_rt_parallel_for`). Bodies that capture outer locals still run sequentially with the same per-index results.
- **Compile-time typechecking** that fails before codegen for known/annotated mismatches (including struct fields), so many errors surface while writing/compiling — not only when the process is deep into a run.

The goal is **safe concurrency** plus **predictable performance**, not “fast but fragile” native code.

## What the toolchain offers today

| Area | Today |
|------|--------|
| **Syntax** | Python-shaped core: functions, structs, modules, collections, typed bindings |
| **Execution** | `hyper run` / `hyper compile` (AOT temp exe); `--emit-exe` to keep a binary — **no interpreter, no JIT** |
| **I/O & JSON** | `open`, `with`, file methods, `open_mmap`, `import json`, `input()` on the compile path |
| **Parallelism** | `@parallel` uses an OS thread pool when the loop body is outlineable; `@vectorize` still sequential until SIMD/GPU backends land |
| **Gaps** | Generics, full Python/stdlib parity — see [Known limitations](../compiler/known-limitations.md) |

## Why pick Hyper over …

**Python (CPython)** — Keep readability and library-oriented workflows; compile hot paths to native code instead of rewriting in C++ or Rust.

**Rust / C++** — Less ceremony for data and ML scripts; Hyper prioritizes approachability first, then performance, with safety built into the runtime rather than manual ownership everywhere.

**Other compiled Python-family languages** — Hyper bets on **Python familiarity** as the on-ramp, plus a **single compiler-only binary** (no tree-walk interpreter).

## Where Hyper is headed

1. **Now** — Compiler-only, AOT-only toolchain; CI smokes for `run` / `compile` and `--emit-exe`.
2. **Next** — Deeper Python/library interop, real `@parallel` codegen, GPU backends.
3. **Long term** — Hyper as the default runtime for **Python-compatible, AI-scale, native-speed** code.

See [First release scope](first-release-scope.md) for the v0.1 checklist.
