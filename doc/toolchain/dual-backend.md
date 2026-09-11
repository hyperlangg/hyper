# Dual backends (LLVM + Cranelift)

Hyper is a **compiled language**. There is no interpreter and **no JIT**. Everyday AOT follows the same split Rust uses:

| Path | Role (Rust analogy) |
|------|---------------------|
| **Default AOT** | Hyper IR → **LLVM IR** → `clang` + C runtime (`hyper_rt*.c`) → executable (like **rustc** + LLVM) |
| **Cranelift** | Fast/debug object path and opt-in AOT: Hyper IR → machine object (like **rustc_codegen_cranelift**) |

## Selecting a backend

| Mechanism | Effect |
|-----------|--------|
| (default) / `HYPER_CODEGEN=llvm` / `--backend llvm` | `run`, `compile`, `--emit-exe` use **LLVM IR + clang** |
| `HYPER_CODEGEN=cranelift` / `--backend cranelift` | same commands use **Cranelift object + host linker** |
| `--emit-obj` | **Always Cranelift** (object emission niche) |
| `--emit-llvm [path]` | Write `.ll` only (default `out.ll`); does not link |

`clang` is **required** for the LLVM AOT path. If it is missing, install LLVM/clang or switch to `--backend cranelift` (which can link with `gcc` / `cc` / MSVC `cl` as well).

## Commands

| Command | Backend | Use when |
|---------|---------|----------|
| `hyper run file.hyp` | Default LLVM AOT (temp exe + execute) | Everyday execution |
| `hyper compile file.hyp` | Same as `run` | Everyday execution |
| `hyper compile file.hyp --emit-ir` | Compiler | Debug Hyper IR lowering |
| `hyper compile file.hyp --emit-llvm out.ll` | LLVM IR dump | Inspect / feed clang yourself |
| `hyper compile file.hyp --emit-obj out.o` | Cranelift object | Inspect or link the object yourself |
| `hyper compile file.hyp --emit-exe out` | Selected AOT + C runtime | Keep a standalone binary |
| `hyper typecheck file.hyp` | Semantic analysis only | Types without running |

## Platforms

Linux, macOS, and native Windows are first-class. On Windows, WSL is optional — not required. **LLVM AOT needs `clang`**; Cranelift AOT needs any of `clang` / `gcc` / `cc` / MSVC `cl`. See [Building from source](../building.md).

## Semantics

- Type errors are **fatal** before codegen (`run` and `compile`).
- Both backends share the same Hyper IR and C runtime ABI (`i64` payloads + kind tags; `__main__` returns `i64`).
- Remaining language gaps: [Known limitations](../compiler/known-limitations.md).

## Direction

Grow the compiler (SIMD/GPU `@vectorize`, library interop, denser unboxed ABI). Do not reintroduce a tree-walk interpreter or Cranelift JIT.
