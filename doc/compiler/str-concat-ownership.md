# String concat ownership (issue #46) — change brief

This note is a **file-by-file, function-by-function** record of the compile-path leak fix for `s = s + "x"` and f-string `StrConcat` temps. Hyper syntax, parser, AST, IR, and lowering are **unchanged**. Send this document to another LLM as the patch summary.

Issue: [f-string / StrConcat intermediates leak on the compile path #46](https://github.com/muhammadyusufpov/hyper/issues/46).

## Unchanged files (read to understand the pipeline)

| File | Why it matters |
|------|----------------|
| `compiler/lowering.rs` | `x = x + "s"` still becomes `Load x` / `ConstStr "s"` / `Binary Add` / `Store x`. F-strings still left-fold `StrConcat`. No consume flags in IR. |
| `compiler/ir.rs` | `IrInstr::Binary`, `StrConcat`, `ValueToStr`, `ConstStr`, `Load`, `Store` — same shapes as before. |
| `src/parser.rs`, `src/ast.rs` | Language surface unchanged. |

Example IR (`hyper compile --emit-ir`):

```text
v5 = load s
v6 = const.str "x"
v7 = add v5 v6          ; codegen turns this into hyper_rt_str_concat
store s v7              ; still pointer overwrite only; free happens inside concat when flagged
```

## Ownership rule (the whole patch)

1. **Interned `ConstStr`** (Cranelift data / `.rodata`) is immortal. Never `free`.
2. **Heap strings** from the runtime (concat, `value_to_str`, methods, `input`, file reads, …) are uniquely owned and **registered**.
3. **`hyper_rt_str_concat(left, right, consume_left, consume_right)`** always allocates a new owned result. It frees an operand only if the matching flag is nonzero **and** the pointer is registered.
4. **Codegen** sets flags when: (a) the operand is an owned SSA temp whose last use is this concat (f-string fold), or (b) store-back `s = s + …` (`load s` then `store s, concat_result` with no reload in between).
5. **Do not** free operands of `t = a + b` — that is the naive fix that UAFs named values / interned literals.

Naive `free(left)` inside concat is **wrong**. This patch adds consume flags instead of always freeing.

## Files actually changed (13)

```
compiler/codegen.rs
compiler/runtime/mod.rs
compiler/runtime/hyper_rt.c
compiler/runtime/str.rs
compiler/runtime/io.rs
compiler/runtime/json.rs
compiler/runtime/file.rs
compiler/runtime/mmap.rs
compiler/runtime/hyper_rt_str.c
compiler/runtime/hyper_rt_io.c
compiler/runtime/hyper_rt_json.c
compiler/runtime/hyper_rt_file.c
compiler/runtime/hyper_rt_mmap.c
```

JIT uses the Rust runtime. AOT (`--emit-exe`) uses the C files. Both must share the 4-argument concat ABI.

---

## 1. `compiler/codegen.rs`

### `declare_runtime` — **changed**

**Removed:** 2-parameter signature for `hyper_rt_str_concat`.

**Added:** 4 `I64` params: `left`, `right`, `consume_left`, `consume_right`.

### New functions (all added)

| Function | Role |
|----------|------|
| `instr_uses` | SSA uses of one `IrInstr` (for last-use). |
| `call_returns_owned_str` | Runtime calls that malloc a new Hyper string (`upper`, `input`, `json_dumps`, file/mmap reads, …). |
| `concat_stored_back_to_name` | After a concat, is the next relevant use of `name` a `Store name, concat_dest`? Stops at `Load name` / `Jump` / `Branch` / `Return`. |
| `should_consume_concat_operand` | Last use is this concat **and** (owned temp **or** store-back load). |
| `concat_consume_plan` | Walks a function body; returns `Vec<(consume_left, consume_right)>` aligned with instruction indices. Marks owned temps: `StrConcat`, string `Binary Add`, `ValueToStr` of non-str, string-returning `Call`. `ConstStr` / `Load` are not owned temps. |

### `define_function` — **changed**

**Removed:** `for instr in body`.

**Added:** `let concat_consume = concat_consume_plan(body);` then `for (idx, instr) in body.iter().enumerate()`.

**Changed — string `IrOp::Add` arm:** call is `call(fref, &[l, r, consume_l, consume_r])` instead of `&[l, r]`. Flags from `concat_consume[idx]`.

**Changed — `IrInstr::StrConcat` arm:** same extra two args.

`Store` / `ConstStr` / `Load` codegen **not** changed. Interned literals still use `strings.define`. Store still does not free.

---

## 2. `compiler/runtime/mod.rs` (JIT)

### New

| Item | Role |
|------|------|
| `OWNED_STRS` | `LazyLock<Mutex<HashSet<usize>>>` of heap string pointers. |
| `heap_cstr` | `CString::into_raw` + register. All Hyper-visible heap strings should go through this. |
| `register_owned_str` | Insert pointer. |
| `owned_str_release` | Remove + `CString::from_raw` **only if registered**. Interned / unknown pointers are a no-op. |
| `owned_str_contains` | `#[cfg(test)]` only. |

### `free_rt_value` — **changed** (`KIND_STR` arm)

**Removed:** unconditional `CString::from_raw` on any nonzero string payload (would free interned `ConstStr` if a list slot held one).

**Added:** `owned_str_release(v.payload)`.

### `hyper_rt_coll_keys` — **changed**

**Removed:** `CString::new(key).into_raw()` without registering.

**Added:** `heap_cstr(key)`.

### `hyper_rt_value_to_str` — **changed**

**Removed:** `CString::new(s).into_raw()` unregistered.

**Added:** `heap_cstr(&s)`.

### `hyper_rt_str_concat` — **changed** (ABI break vs old JIT)

**Removed signature:** `hyper_rt_str_concat(left: i64, right: i64) -> i64`

**Added signature:** `hyper_rt_str_concat(left, right, consume_left, consume_right) -> i64`

**Removed body:** copy both sides into `String`s, `format!("{}{}", a, b)`, `into_raw`, never free operands.

**Added body:** borrow `&str` from both sides, `heap_cstr` the concatenation, then if `consume_* != 0` call `owned_str_release` on that operand.

### Tests in `mod tests` — **changed / added**

**Changed:** `str_payload` now uses `heap_cstr`.

**Added:**

- `concat_without_consume_keeps_live_operands` — flags `0,0`; left must still be readable (`t = a + b`).
- `concat_consume_left_drops_owned_temp_not_interned` — consume owned left; consume interned right is a no-op.
- `concat_store_back_loop_does_not_accumulate_owned_strings` — 10k consume-left concats; previous pointer leaves the owned set.

---

## 3. `compiler/runtime/hyper_rt.c` (AOT)

Must match the Rust ABI.

### New

| Item | Role |
|------|------|
| `g_owned_strs` / `g_owned_n` / `g_owned_cap` | Growable pointer table. |
| `hyper_rt_owned_str_register` | Append `malloc`'d Hyper string. |
| `hyper_rt_owned_str_release` | Swap-remove + `free` if present; else no-op. |
| `hyper_rt_str_dup` | `malloc` copy + register. Used by other `.c` files. |

### `free_rt_value` — **changed** (`KIND_STR`)

**Removed:** `if (payload) free(payload);`

**Added:** `hyper_rt_owned_str_release(...)`.

### `hyper_rt_coll_keys` — **changed**

**Removed:** raw `malloc` + `memcpy` of dict keys.

**Added:** `hyper_rt_str_dup(key)`.

### `hyper_rt_value_to_str` — **changed**

**Removed:** raw `malloc` for `KIND_STR` copy and for the final formatted buffer.

**Added:** `hyper_rt_str_dup`.

### `hyper_rt_str_concat` — **changed**

**Removed:** `int64_t hyper_rt_str_concat(int64_t left, int64_t right)` with malloc-only result.

**Added:** extra `consume_left`, `consume_right`; `hyper_rt_owned_str_register(out)`; conditional `hyper_rt_owned_str_release` on operands.

---

## 4. JIT helpers: `cstr_payload` → `heap_cstr`

Same one-line change in each file. Local `cstr_payload` still exists; it now forwards to `super::heap_cstr` so method/IO/JSON/file/mmap **returned** strings are registered.

Dropped unused `CString` imports where allocation moved.

| File | Function |
|------|----------|
| `compiler/runtime/str.rs` | `cstr_payload` |
| `compiler/runtime/io.rs` | `cstr_payload` (`hyper_rt_input`) |
| `compiler/runtime/json.rs` | `cstr_payload` |
| `compiler/runtime/file.rs` | `cstr_payload` (read/path/mode returns) |
| `compiler/runtime/mmap.rs` | `cstr_payload` (chunk returns) |

Nothing else in those files was redesigned.

---

## 5. AOT helpers: `rt_strdup` → registered dup

### `compiler/runtime/hyper_rt_str.c`

**Added:** `extern char *hyper_rt_str_dup(const char *s);`

**Changed:** local `rt_strdup` now `return hyper_rt_str_dup(s);` (string methods return owned Hyper strings).

### `compiler/runtime/hyper_rt_io.c`

**Added:** `extern char *hyper_rt_str_dup`.

**Changed:** `rt_strdup` → `hyper_rt_str_dup` (`hyper_rt_input` result).

### `compiler/runtime/hyper_rt_json.c`

Same as IO: `cstr_payload` / `rt_strdup` go through `hyper_rt_str_dup`.

### `compiler/runtime/hyper_rt_file.c`

**Added:** `extern char *hyper_rt_str_dup`, `extern void hyper_rt_owned_str_register`.

**Changed:** `rt_strndup` (read buffers) registers after malloc.

**Changed:** `hyper_rt_file_path` / `hyper_rt_file_mode` return `hyper_rt_str_dup(...)` instead of unregistered `rt_strdup`.

**Not changed:** internal `f->path` / `f->mode` still use private `rt_strdup` (freed on close, not via the owned-string set).

### `compiler/runtime/hyper_rt_mmap.c`

**Added:** extern `hyper_rt_str_dup` / `hyper_rt_owned_str_register`.

**Changed:** empty-chunk return uses `hyper_rt_str_dup("")`; successful `read_chunk` buffer is registered.

**Not changed:** `str_arg` still uses private `rt_strdup` for internal path copies.

---

## What was deliberately not added

- No IR consume flags, no new Hyper syntax, no `str_build` opcode.
- No free on every `Store` (would break `let t = s` pointer aliasing; out of scope vs full GC).
- No interpreter path (the interpreter was removed).

Overwriting a local with a **fresh** string that is not `s = s + …` still does not free the previous local (`last = f"n={i}!"` leaks the previous `last` only, not a concat chain). That is linear leftovers, not the quadratic prefix leak.

## Suggested questions for another LLM

1. Walk `s = s + "x"` from lowering IR through `concat_consume_plan` flags to `hyper_rt_str_concat`.
2. Explain why `t = a + b` must pass `(0, 0)`.
3. Explain why interned `"x"` can have `consume_right=1` and still be safe.
4. Contrast JIT `OWNED_STRS` vs AOT `g_owned_strs`.
5. Why `free_rt_value` must not `free` every `KIND_STR` pointer anymore.
