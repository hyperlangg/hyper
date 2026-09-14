# Memory-mapped files

`open_mmap` maps a file for chunked reads without loading the entire contents into a single string first. Use it for large binaries where you only need slices.

```hyper
with open_mmap("huge_ai_model.bin") as mapped_file:
    let chunk = mapped_file.read_chunk(0, 1024)
```

`read_chunk(offset, size)` copies a byte range into a Hyper string. Offsets past EOF yield an empty string. The `with` form closes the mapping when the block ends.

## Compile path

Memory-mapped open and `read_chunk` lower on the AOT compile path (`run`, `compile`, and `--emit-exe`), alongside ordinary file I/O. Prefer `open` / `read` for small text files; reserve `open_mmap` for large, offset-oriented access.

## Notes

- Paths must exist and be readable for a successful map.
- Chunks are text/byte buffers as returned by the runtime; interpret them according to your file format.
- Combine with ordinary parsing only on the slices you need.

## Example

Runnable sample: [`examples/file_handling/mmap.hyp`](../../examples/file_handling/mmap.hyp)
