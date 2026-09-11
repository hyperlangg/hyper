<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://github.com/user-attachments/assets/c3edacd4-1094-4e7d-91d4-8b42a439debf">
    <source media="(prefers-color-scheme: light)" srcset="https://github.com/user-attachments/assets/0427f9c4-be17-4784-a3e5-8ac388b9ee9b">
    <img alt="The Hyper Programming Language" src="https://github.com/user-attachments/assets/0427f9c4-be17-4784-a3e5-8ac388b9ee9b" width="50%">
  </picture>

  [Getting started] | [Documentation] | [Contributing]
</div>

This is the main source code repository for **Hyper**. It currently contains the native compiler, and documentation.

[Getting Started]: https://github.com/hyperlangg/hyper/blob/main/doc/building.md
[Documentation]: https://github.com/hyperlangg/hyper/tree/main/doc
[Contributing]: CONTRIBUTING.md

## What is Hyper?

**Hyper** is a compiled programming language built for **Python familiarity**, **native performance**, and **AI-scale workloads**. Its syntax is deliberately close to Python so teams can reuse existing habits, scripts, and ecosystems inside the Hyper environment with minimal friction.

Hyper is designed to deliver **C and C++ class memory control** and **hardware-aware execution** (CPU and GPU) so numerically heavy programs can run **orders of magnitude faster** than typical CPython — often tens to hundreds of times faster on hot paths once compiled.

The language targets the bottlenecks of **neural network training** and **large-scale data processing**: long-running compute kernels, tight memory use, and parallel work across cores and accelerators.

Architecture draws from **Rust-style safety** (memory safety as a first-class goal) and **modern parallelism** (multithreading and vectorized loops as the program model evolves).

## Why Hyper?

Hyper exists so teams can keep a **Python-shaped** workflow while getting **systems-level** speed and control. The points below are the design bets that drive the language and toolchain:

- **Python-compatible surface:** Readable, indentation-based syntax; a path toward running existing Python-oriented code and libraries in Hyper.
- **Maximum speed and efficiency:** Native AOT compilation (default Hyper-IR → LLVM IR → clang; Cranelift for `--emit-obj` / opt-in AOT), buffered I/O, and low runtime overhead — built to rival systems languages on performance-critical code.
- **Built for artificial intelligence:** First-class focus on training workloads, tensor-style numerics, and processing very large datasets without interpreter bottlenecks.
- **Security and modern architecture:** Memory-safe implementation strategy, clear error reporting, and parallel execution as the platform matures.

## Building from source

Clone, build, and run the toolchain from source — see [Building from source](doc/building.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

For a detailed explanation of the language's architecture and how to begin contributing, see the development guide.

## License

Hyper is primarily distributed under the terms of both the MIT license and the Apache License (Version 2.0).

See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.
