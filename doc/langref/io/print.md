# Printing

`print` writes values to standard output. It is variadic: you may pass one or more arguments.

```hyper
let greet = "Hello, World!"
print(greet)
```

User programs should use **`print()`** for normal output. Compiler and runtime diagnostics go to **stderr** with `SyntaxError` / `IndentationError` / `RuntimeError` formatting — there is no separate language-level logging API.

## Notes

- Values are converted for display on the compile path (strings, numbers, booleans, collections, and so on).
- Prefer clear `print` calls in examples and tools; keep error signaling to `raise` / the runtime.
- `print` lowers on the AOT compile path (`run`, `compile`, and `--emit-exe`).

## Example

Runnable sample: [`examples/io/print.hyp`](../../examples/io/print.hyp)
