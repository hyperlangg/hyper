# Parallel for (`@parallel`)

The `@parallel` decorator marks a `for` loop for **multi-threaded** execution across CPU cores. Each index may run concurrently, so the body should be safe without conflicting shared mutable writes.

```hyper
@parallel
for i in range(0, n):
    print(i)
```

## Compile path today

When the loop body only uses the induction variable (and call callees such as `print`), Hyper **outlines** the body into a worker and runs it on an OS thread pool (`hyper_rt_parallel_for` in the AOT C runtime). Per-index work is real parallelism — print order may vary across runs.

If the body reads or mutates outer locals, the loop still lowers **sequentially** (same per-index results as a normal `for`) until capture support lands.

`break` and `continue` are **rejected** inside a `@parallel` (or `@parallel @vectorize`) body — early exit has no single well-defined meaning across split iterations.

## Notes

- Place `@parallel` immediately above the `for`.
- Prefer independent per-index work (no races on shared mutables).
- See also [`@vectorize`](vectorized-for.md) and the combined form.
