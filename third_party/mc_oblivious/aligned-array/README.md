# aligned-array

This is a fork of Jorge Aparicio's lovely
[aligned crate](https://github.com/japaric/aligned), and provides a way to
constrain a type to be over-aligned at some boundary.

The purpose of the fork is broadly to integrate better with
[generic-array](https://github.com/fizyk20/generic-array), and in particular, to
provide a general-purpose "aligned bytes" types, with safe, zero-overhead APIs
for viewing them as sequences of smaller numbers of bytes / at a smaller
alignment, or as native-endian integers. (This is not unlike how some rust SIMD
intrinsics can be "viewed" in several ways without copying:
https://doc.rust-lang.org/nightly/core/arch/x86/struct.__m256i.html)

For example, using the `GenericArray::Split` API, we can split a reference to a
slice of known size into two references to pieces, whose sizes are also known at
compile time. But, if we use `Split` on an `Aligned` `generic-array`, we lose
the information we had about the *alignment* of those two pieces.

For some applications, that alignment information is critical to be able to go
on to use the two pieces in a performant way, perhaps using SIMD instructions
later. It also may enable the compiler to generate faster code simply for moving
them around.

This crate offers implementations of `Split` API and others which return
`Aligned<GenericArray>` instead of `GenericArray`, without creating copies or
extra instructions in the release mode binary. Preserving alignment information
in this way allows users to go on to solve complex systems problems in a highly
performant way, without compromising on safety.

We also integrate a little bit with
[subtle](https://github.com/dalek-cryptography/subtle) in order that aligned
bytes implement `ConstantTimeEq`. That function can be implemented faster
sometimes when alignment information is available.

## Specific list of differences with `aligned` crate:

-   Extra alignment types: `A32`, `A64`.
-   Better conversions between `Aligned` objects: e.g. `Aligned<A8, T>`
    implements `AsRef<Aligned<A4, T>>`. This is permitted because objects
    aligned on a large boundary are also necessarily aligned on a smaller
    boundary.
-   `Aligned` derives `Eq, PartialEq, Hash, Ord, PartialOrd` when possible.
-   `Aligned` derives `subtle::ConstantTimeEq` when possible.
-   `Aligned` derives `FromIterator` and `IntoIterator` when possible.
-   `Aligned` derives `generic_array::sequence::GenericSequence` when possible.
-   `Aligned` implements `generic_array::sequence::Split` in such a way that
    preserves alignment information. In particular `Aligned<GenericArray<u8,
    N>>` can be split into `Aligned<GenericArray<u8, M>>` and
    `Aligned<GenericArray<u8, N - M>>` if `M` divides the alignment value. This
    is only done for the type `u8` right now but could be generalized.
-   Extra traits: `AsNeSlice` and `AsAlignedChunks`, and implementations for
    aligned arrays of bytes. These allow aligned sequences of bytes to be
    interpretted, with no overhead, as slices of native-endian integers, or
    slices of smaller aligned byte chunks.
-   `as-slice` dependency is removed. This crate is rarely useful in practice
    because `AsRef<[u8]>` and similar traits usually do the job just fine.
    `as-slice` also unfortunately depends on multiple versions of
    `generic-array` at once, bringing them all into the build plan, which is
    messy and makes some dependency mismatch problems harder to catch and solve
    in large projects. Making this crate optional doesn't fix that because Cargo
    adds them all to the `Cargo.lock` regardless of feature configuration. We
    would consider bringing back the `as-slice` stuff if the maintainers release
    a version of `as-slice` that depends on only the most recent version of
    `generic-array`.

## Unsafe code

This crate contains a few more unsafe casts than the original `aligned` crate
did. This is needed to implement the integrations with `generic-array`.

These casts are similar to the casts that `generic-array` performs itself
internally: they generally are based on the assumption that `GenericArray<T, N>`
is layout-compatible with `[T; N]`.

All casts like this

-   have their preconditions enforced by asserts, that llvm proves are true and
    strips out in release mode builds
-   have detailed correctness notes,
-   have been tested extensively in a large project.

This code was also covered as part of a formal audit by Trail of Bits.
