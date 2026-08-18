//! A macro for stating unsafe assumptions in Rust.
//!
//! Using this macro, one can supply assumptions to the compiler for use in optimization.
//! These assumptions are checked in `debug_assertion` configurations, and are unchecked
//! (but still present) otherwise.
//!
//! This is an inherently unsafe operation. It lives in the space between regular `assert!`
//! and pure `unsafe` accesses - it relies heavily on an optimizing compiler's ability to
//! track unreachable paths to eliminate unnecessary asserts.
//!
//! # Examples:
//! ```
//! # fn get_index() -> usize { 0 }
//! # fn main() {
//! use assume::assume;
//!
//! let v = vec![1, 2, 3];
//!
//! // Some computed index that, per invariants, is always in bounds.
//! let i = get_index();
//!
//! assume!(
//!     unsafe: i < v.len(),
//!     "index {} is beyond vec length {}",
//!     i,
//!     v.len(),
//! );
//! let element = v[i];  // Bounds check optimized out per assumption.
//! # }
//! ```
//! ```
//! # use std::collections::HashMap;
//! # fn populate_items() -> HashMap<u32, String> {
//! #     let mut result = HashMap::default();
//! #     result.insert(0, "hello".to_string());
//! #     result
//! # }
//! # fn main() {
//! use assume::assume;
//!
//! let items: HashMap<u32, String> = populate_items();
//!
//! // Some item that, per invariants, always exists.
//! let item_zero_opt: Option<&String> = items.get(&0);
//!
//! assume!(
//!     unsafe: item_zero_opt.is_some(),
//!     "item zero missing from items map",
//! );
//! let item_zero = item_zero_opt.unwrap();  // Panic check optimized out per assumption.
//! # }
//! ```
//! ```
//! # fn main() {
//! use assume::assume;
//!
//! enum Choices {
//!     This,
//!     That,
//!     Other,
//! }
//! # fn get_choice() -> Choices { Choices::This }
//!
//! // Some choice that, per invariants, is never Other.
//! let choice = get_choice();
//!
//! match choice {
//!     Choices::This => { /* ... */ },
//!     Choices::That => { /* ... */ },
//!     Choices::Other => {
//!         // This case optimized out entirely, no panic emitted.
//!         assume!(
//!             unsafe: @unreachable,
//!             "choice was other",
//!         );
//!     },
//! }
//! # }
//! ```
//! ```
//! # fn main() {
//! use assume::assume;
//!
//! #[inline(always)]
//! fn compute_value() -> usize {
//!     let result = compute_value_internal();
//!
//!     // Can also be used to provide hints to the caller,
//!     // after the optimizer inlines this assumption.
//!     assume!(
//!         unsafe: result < 12,
//!         "result is invalid: {}",
//!         result,
//!     );
//!     result
//! }
//!
//! fn compute_value_internal() -> usize {
//!     /* ... */
//!     # 0
//! }
//!
//! fn process_data(data: &[f64; 100]) {
//!     // Bounds check elided per implementation's assumption.
//!     let value = data[compute_value()];
//! }
//! # }
//! ```
//!
//! # Gotchas
//! - Unlike `debug_assert!` et. al., the condition of an `assume!` is always present -
//!   it's the panic that is removed. Complicated assumptions involving function calls
//!   and side effects are unlikely to be helpful; the condition ought to be trivial and
//!   involve only immediately available facts.
//!
//! - As stated, this relies on the optimizer to propagate the assumption. Differences in
//!   optimization level or mood of the compiler may cause it to fail to elide assertions
//!   in the final output. If you simply *must* have no checking and do not want to rely
//!   on optimizations, then a `debug_assert!` + `unsafe` access is the way to go.
//!
//! - Avoid using `assume!(unsafe: false)` to indicate unreachable code. Although this works,
//!   the return type is `()` and not `!`. This can result in warnings or errors if e.g. other
//!   branches evaluate to a type other than `()`. Use `assume!(unsafe: @unreachable)` instead.
//!
#![doc(html_root_url = "https://docs.rs/assume/0.5.0")]
#![no_std]

/// Assumes that the given condition is true.
///
/// This macro allows the expression of invariants in code. For example, one might `assume!`
/// that an index is in bounds prior to indexing into a slice - this would allow the optimizer
/// to remove the bounds checking entirely, under the promises of assume. In `debug_assertion`
/// configurations the expression is checked. Otherwise, it is unchecked (but present).
///
/// Use `@unreachable` as the condition to assume the code path cannot be reached.
///
/// Because this expresses unchecked information, the act of assuming is inherently unsafe.
/// The safe (i.e., runtime checked) alternative to this is the [`assert!`] macro. If the
/// condition is `@unreachable`, the safe alternative to this is the [`unreachable!`] macro.
///
/// See the module level documentation for more.
/// ```
#[macro_export]
macro_rules! assume {
    (unsafe: $cond:expr $(,)?) => {{
        $crate::__assume_impl!(
            $cond,
            $crate::__private::concat!(
                "assumption failed: ",
                $crate::__private::stringify!($cond)
            )
        )
    }};
    (unsafe: $cond:expr, $fmt:expr $(, $($args:tt)*)?) => {{
        $crate::__assume_impl!($cond, $fmt, $($($args)*)?)
    }};
    (unsafe: @unreachable $(,)?) => {{
        $crate::__assume_impl!(@unreachable, "assumption failed: unreachable")
    }};
    (unsafe: @unreachable, $fmt:expr $(, $($args:tt)*)?) => {{
        $crate::__assume_impl!(@unreachable, $fmt, $($($args)*)?)
    }};
    (unsafe: $($_:tt)*) => {{
        $crate::__private::compile_error!("assumption must be an expression or @unreachable");
    }};
    ($($_:tt)*) => {{
        $crate::__private::compile_error!("assumption must be prefixed with 'unsafe: '");
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __assume_impl {
    ($cond:expr, $fmt:expr $(, $($args:tt)*)?) => {{
        #[allow(unused_unsafe)]
        if unsafe { !$cond } {
            $crate::__assume_impl!(@unreachable, $fmt, $($($args)*)?)
        }
    }};
    (@unreachable, $fmt:expr $(, $($args:tt)*)?) => {{
        if $crate::__private::cfg!(debug_assertions) {
            // Panic cannot accept non-const format strings, which means we cannot
            // arbitrarily augment this message with more detail. Instead, we behave
            // like assert!: the default message is the code, but a provided format
            // string replaces this entirely if provided.
            //
            // This makes assume! as const as panic!/assert!.
            $crate::__private::panic!($fmt, $($($args)*)?);
        } else {
            unsafe {
                $crate::__private::unreachable_unchecked()
            }
        }
    }};
}

/// Used by macros.
#[doc(hidden)]
pub mod __private {
    pub use core::{cfg, compile_error, concat, hint::unreachable_unchecked, panic, stringify};
}

#[cfg(test)]
mod tests {
    /// Rogue macro.
    #[allow(unused_macros)]
    macro_rules! cfg {
        ($($tt:tt)*) => {
            return
        };
    }

    /// Rogue macro.
    #[allow(unused_macros)]
    macro_rules! concat {
        ($($tt:tt)*) => {
            return
        };
    }

    /// Rogue macro.
    #[allow(unused_macros)]
    macro_rules! panic {
        ($($tt:tt)*) => {
            return
        };
    }

    /// Rogue macro.
    #[allow(unused_macros)]
    macro_rules! stringify {
        ($($tt:tt)*) => {
            return
        };
    }

    // Rogue core module.
    mod core {}

    #[test]
    fn conditional_can_be_unsafe() {
        let values = [1, 2, 3];
        assume!(unsafe: *values.get_unchecked(0) > 0);
    }

    #[test]
    const fn fn_can_be_const() {
        assume!(unsafe: 1 > 0, "impossible");
    }

    #[test]
    fn no_unused_formatter_args_warnings() {
        // We can't actually write a test for this, but this test
        // will produce the warning if this suppression is broken.
        let unused = 0;
        assume!(unsafe: true, "this is unused: {}", unused);
    }

    #[test]
    #[should_panic(expected = "assumption failed: 2 > 3")]
    #[cfg(debug_assertions)]
    fn is_not_affected_by_call_site_environment() {
        assume!(unsafe: 2 > 3);
    }

    #[test]
    #[should_panic(expected = "oh no")]
    #[cfg(debug_assertions)]
    fn is_not_affected_by_call_site_environment_with_message() {
        assume!(unsafe: 2 > 3, "oh no");
    }

    #[test]
    #[should_panic(expected = "oh no, a problem")]
    #[cfg(debug_assertions)]
    fn is_not_affected_by_call_site_environment_with_format() {
        assume!(unsafe: 2 > 3, "oh no, a {}", "problem");
    }

    #[test]
    #[should_panic(expected = "assumption failed: unreachable")]
    #[cfg(debug_assertions)]
    fn is_not_affected_by_call_site_environment_unreachable() {
        assume!(unsafe: @unreachable);
    }

    #[test]
    #[should_panic(expected = "oh no")]
    #[cfg(debug_assertions)]
    fn is_not_affected_by_call_site_environment_unreachable_with_message() {
        assume!(unsafe: @unreachable, "oh no");
    }

    #[test]
    #[should_panic(expected = "oh no, a problem")]
    #[cfg(debug_assertions)]
    fn is_not_affected_by_call_site_environment_unreachable_with_format() {
        assume!(unsafe: @unreachable, "oh no, a {}", "problem");
    }
}
