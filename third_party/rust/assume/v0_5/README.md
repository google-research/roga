# assume

A macro for stating unsafe assumptions in Rust.

Using this macro, one can supply assumptions to the compiler for use in
optimization. These assumptions are checked in `debug_assertion` configurations,
and are unchecked (but still present) otherwise.

This is an inherently unsafe operation. It lives in the space between regular
`assert!` and pure `unsafe` accesses - it relies heavily on an optimizing
compiler's ability to track unreachable paths to eliminate unnecessary asserts.

```toml
[dependencies]
assume = "0.5"
```

## Examples

```rust
use assume::assume;

let v = vec![1, 2, 3];

// Some computed index that, per invariants, is always in bounds.
let i = get_index();

assume!(
    unsafe: i < v.len(),
    "index {} is beyond vec length {}",
    i,
    v.len(),
);
let element = v[i];  // Bounds check optimized out per assumption.
```

```rust
use assume::assume;

let items: HashMap<u32, String> = populate_items();

// Some item that, per invariants, always exists.
let item_zero_opt: Option<&String> = items.get(&0);

assume!(
    unsafe: item_zero_opt.is_some(),
    "item zero missing from items map",
);
let item_zero = item_zero_opt.unwrap();  // Panic check optimized out per assumption.
```

```rust
use assume::assume;

enum Choices {
    This,
    That,
    Other,
}

// Some choice that, per invariants, is never Other.
let choice = get_choice();

match choice {
    Choices::This => { /* ... */ },
    Choices::That => { /* ... */ },
    Choices::Other => {
        // This case optimized out entirely, no panic emitted.
        assume!(
            unsafe: @unreachable,
            "choice was other",
        );
    },
}
```

```rust
use assume::assume;

#[inline(always)]
fn compute_value() -> usize {
    let result = compute_value_internal();

    // Can also be used to provide hints to the caller,
    // after the optimizer inlines this assumption.
    assume!(
        unsafe: result < 12,
        "result is invalid: {}",
        result,
    );
    result
}

fn compute_value_internal() -> usize {
    /* ... */
}

fn process_data(data: &[f64; 100]) {
    // Bounds check elided per implementation's assumption.
    let value = data[compute_value()];
}

```

## Motivation

Programs often have invariants that cannot be expressed in the type system. Rust
is safe by default, and in such cases asserts are made at runtime to verify
these invariants. A common example of this is bounds checking for slices.

Consider the following (somewhat convoluted) example:

```rust
pub struct ValuesWithEvens {
    values: Vec<u32>,  // Some integers.
    evens: Vec<usize>, // Indices of even integers in `values`.
}

impl ValuesWithEvens {
    pub fn new(values: Vec<u32>) -> Self {
        let evens = values
            .iter()
            .enumerate()
            .filter_map(
                |(index, value)| {
                    if value % 2 == 0 {
                        Some(index)
                    } else {
                        None
                    }
                }
            )
            .collect();

        Self { values, evens }
    }

    pub fn pop_even(&mut self) -> Option<u32> {
        let index = self.evens.pop()?;

        // We know this index is valid by construction,
        // but a bounds check is performed anyway.
        let value = self.values[index];

        Some(value)
    }
}

fn main() {
    let mut vwe = ValuesWithEvens::new(vec![1, 2, 3, 4]);

    println!("{:?}", vwe.pop_even());
}
```

By construction, indices contained within `evens` are always valid indices into
`values`. However, this cannot be expressed in the type system and there is a
bounds check on the line:

```rust
let value = self.values[index];
```

This ensures a bug in the program does not result in an out of bounds access.
For example, if another method were introduced that modified `values` but forgot
to update `evens`, it could invalidate the indices - this would not result in
undefined behavior thanks to bounds checking.

However, if this is a hot-spot in the program we may need to remove this check.
Rust offers `unsafe` access:

```rust
    pub fn pop_even(&mut self) -> Option<u32> {
        let index = self.evens.pop()?;

        let value = unsafe { *self.values.get_unchecked(index) };

        Some(value)
    }
```

This has no bounds check, but we've removed any scrutiny around the access. We
can improve this with a debug-only assertion:

```rust
    pub fn pop_even(&mut self) -> Option<u32> {
        let index = self.evens.pop()?;

        debug_assert!(index < self.evens.len());
        let value = unsafe { *self.values.get_unchecked(index) };

        Some(value)
    }
```

Can you spot the bug? We've asserted against the wrong vector! This should be:

```rust
debug_assert!(index < self.values.len());
//                         ^^^^^^
```

The decoupling of assertion to optimization is error-prone.

The `assume!` macro relies on the optimizer's ability to leverage stated
assumptions. An incorrect assumption leaves the bounds check alone, but a
correct assumption removes it:

```rust
    pub fn pop_even(&mut self) -> Option<u32> {
        let index = self.evens.pop()?;

        assume!(
            unsafe: index < self.values.len(),
            "even index {} beyond values vec length {}",
            index,
            self.values.len(),
        );
        let value = self.values[index];

        Some(value)
    }
```

The optimizer considers the bounds check dead code per the assumption, so it is
removed. Furthermore, this will assert our condition holds in `debug_assertion`
configurations such as in tests.

Assumptions can also be provided to the caller's context by the implementation:

```rust
    #[inline(always)]
    pub fn pop_even(&mut self) -> Option<u32> {
        let value = self.pop_even_internal()?;

        assume!(
            unsafe: value % 2 == 0,
            "popped value {} is not even",
            value,
        );
        value
    }

    fn pop_even_internal(&mut self) -> Option<u32> {
        /* ... */
    }
```

The caller now receives optimizations "for free". For example:

```rust
fn compute_something(vwe: &mut ValuesWithEvens) -> Option<f64> {
    let value = vwe.pop_even()?;

    perform_common_task(value)
}

fn perform_common_task(value: u32) -> Option<f64> {
    if value % 2 == 0 {
        /* ... */
    } else {
        // This branch is now considered dead code when the
        // function is called from the `compute_something` path.
    }
}
```

## When not to use

Do not use this macro.

Rely on `assert!` to check program invariants.

Rely on `unreachable!` to state that some code path should never be taken.

## When to use

Okay - once you:

-   Have profiling results indicating some invariant check is causing overhead.
-   Have no way of re-arranging the program to express this without overhead.
-   Are about to reach for an `unsafe` get operation.

Then you should consider `assume!` instead.

This is not a beginner-friendly macro; you must verify the desired optimizations
are taking place. You should also have a suite of tests that build with
`debug_assertion` enabled in order to catch violations of the invariant.

## Gotchas

-   Unlike `debug_assert!` et. al., the condition of an `assume!` is always
    present - it's the panic that is removed. Complicated assumptions involving
    function calls and side effects are unlikely to be helpful; the condition
    ought to be trivial and involve only immediately available facts.

-   As stated, this relies on the optimizer to propagate the assumption.
    Differences in optimization level or mood of the compiler may cause it to
    fail to elide assertions in the final output. If you simply *must* have no
    checking and do not want to rely on optimizations, then a `debug_assert!` +
    `unsafe` access is the way to go.

-   Avoid using `assume!(unsafe: false)` to indicate unreachable code. Although
    this works, the return type is `()` and not `!`. This can result in warnings
    or errors if e.g. other branches evaluate to a type other than `()`. Use
    `assume!(unsafe: @unreachable)` instead.

## See Also

The underlying mechanism for the macro is
[`std::hint::unreachable_unchecked`](https://doc.rust-lang.org/std/hint/fn.unreachable_unchecked.html).

## License

Licensed under either of

*   Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
    http://www.apache.org/licenses/LICENSE-2.0)
*   MIT license ([LICENSE-MIT](LICENSE-MIT) or
    http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
