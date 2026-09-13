# Reference parameters (`ref`)

A `ref` parameter is a **mutable binding** into the callee. For struct instances, field storage is shared with the caller: updates inside the function are visible afterward.

```hyper
struct Box:
    let pub mut n: i64

    pub fn __init__(ref self, n: i64):
        self.n = n

fn bump(ref b: Box):
    b.n = b.n + 1

let mut x = Box(1)
bump(x)
print(x.n)
```

Methods that mutate `self` conventionally take `ref self`. Constructors (`__init__`) also use `ref self` when assigning fields.

## Notes

- Passing a struct into a `ref` parameter shares the instance’s fields; it is not a defensive copy.
- List, array, and dict payloads are shared the same way: mutations through a `ref` alias are visible, and overwriting a slot does not free a nested list or dict another name still holds.
- The argument at the call site should be a mutable location when the callee writes through `ref`.

## Example

Runnable sample: [`examples/function/reference.hyp`](../../examples/function/reference.hyp)
