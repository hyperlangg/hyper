# Summary

[Introduction](readme.md)

# Overview

- [Why Hyper](overview/why-hyper.md)

# Toolchain

- [Quickstart](quickstart.md)
- [Building from source](building.md)
- [Compiler-only toolchain](toolchain/dual-backend.md)

# Language reference

- [Language reference](langref/README.md)

## Variables

- [Immutable](langref/variable/immutable.md)
- [Mutable](langref/variable/mutable.md)

## Data types

- [Boolean](langref/data_type/boolean.md)
- [None](langref/data_type/none.md)
- [String](langref/data_type/string.md)
- [Signed integers](langref/data_type/numerical/signed-integers.md)
- [Unsigned integers](langref/data_type/numerical/unsigned-integers.md)
- [Floats](langref/data_type/numerical/floats.md)

## Collections

- [List](langref/collection/list.md)
- [Array](langref/collection/array.md)
- [Dictionary](langref/collection/dictionary.md)
- [Methods](langref/collection/methods.md)

## Operators

- [Arithmetic](langref/operator/arithmetic.md)
- [Assignment](langref/operator/assignment.md)
- [Boolean](langref/operator/boolean.md)
- [Comparison](langref/operator/comparison.md)

## Loops

- [For](langref/loop/for.md)
- [While](langref/loop/while.md)
- [String concat stress](langref/loop/str-concat-stress.md)
- [Vectorized for](langref/loop/advanced/vectorized-for.md)
- [Parallel for](langref/loop/advanced/parallel-for.md)
- [Parallel + vectorized for](langref/loop/advanced/parallel-vectorized-for.md)

## Conditionals

- [If / elif / else](langref/conditional/if-elif-else.md)
- [Ternary](langref/conditional/ternary.md)

## Functions

- [Simple](langref/function/simple.md)
- [Reference (`ref`)](langref/function/reference.md)
- [Strict types](langref/function/strict-type.md)

## Structs

- [Object creation](langref/struct/object_creation.md)
- [Composition](langref/struct/inheritance.md)
- [Traits](langref/struct/traits.md)

## Modules

- [Import](langref/module/import.md)
- [Sample module (`math`)](langref/module/math.md)

## Console I/O

- [Print](langref/io/print.md)
- [User input](langref/io/user-input.md)

## Files and JSON

- [Standard file I/O](langref/file_handling/standard.md)
- [JSON I/O](langref/file_handling/json_io.md)
- [Memory-mapped files](langref/file_handling/mmap.md)

## Errors

- [SyntaxError](langref/errors/syntax_error.md)
- [IndentationError](langref/errors/indentation_error.md)
- [RuntimeError](langref/errors/runtime_error.md)
- [Raise and handle](langref/errors/raise_handle.md)

# Compiler

- [Overview](compiler/overview.md)
- [Supported features](compiler/supported-features.md)
- [Known limitations](compiler/known-limitations.md)
- [String concat ownership (change brief)](compiler/str-concat-ownership.md)
- [Concat leak showcase (before / after RSS)](compiler/str-concat-stress-showcase.md)

# Standard library

- [File handling](standard-library/file-handling.md)
- [JSON module](standard-library/json-module.md)

# Errors

- [Error kinds](errors/overview.md)

# Contributing

- [Commit conventions](COMMIT_CONVENTION.md)

# Code samples

Hyper syntax examples (`.hyp` files only) live in [`examples/`](examples/) — they mirror [`langref/`](langref/) topics. Browse that directory in the repository for runnable samples.
