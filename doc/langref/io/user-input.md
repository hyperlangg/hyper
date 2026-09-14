# User input

`input` reads a line from standard input. An optional prompt string is written before the read.

```hyper
let name = input("Enter your name: ")
print(name)
```

The returned value is a string (without the trailing newline). Convert with `int(...)`, `float(...)`, or your own parsing when you need another type — and use `raises` / `handle` if parsing can fail.

## Notes

- `input` lowers on the AOT compile path (`run`, `compile`, and `--emit-exe`).
- Interactive prompts need a real terminal or piped stdin; CI samples feed input where required.
- For file-based input, prefer `open` / `with` rather than reading everything through `input`.

## Example

Runnable sample: [`examples/io/user-input.hyp`](../../examples/io/user-input.hyp)
