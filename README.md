## ROGA - resizable oblivious group-by aggregations

NOTE: This is not an officially supported Google product. This project is not eligible for the [Google Open Source Software Vulnerability Rewards Program](https://bughunters.google.com/open-source-security).

Resizable oblivious histogram with PRF-derived keyed routing and multi-copy
sharding for parallel throughput.

This crate provides two core types:

-   **`ObliviousHistogram`**: a single-tree Path OSAM histogram. Keys are hashed
    using AES-128 and routed directly to tree leaves. Counts are accumulated
    with `wrapping_add`. The tree can grow on demand via a
    differentially-private auto-resize mechanism. Fully parameterized by tag
    size `T` and payload size `P`.

-   **`ShardedObliviousHistogram`**: a parallel multi-copy sharded layer that
    distributes batches of updates across independently-keyed OSAM trees using
    oblivious sort + distribute. Provides near-linear throughput scaling with
    the number of shards/cores. Fully parameterized by tag size `T` and payload
    size `P`.

Both types expose a uniform `increment` / `increment_batch` / `flush` /
`read_total` interface via inherent methods.

The auto-resize mechanism is based on the *Resizable Oblivious Histograms* draft
(see `papers/Resizable_Oblivious_Histograms/`).

### Status

This implementation has not been audited. Use at your own risk.

Memory is assumed to be encrypted by the surrounding enclave; this crate does
not perform encryption-on-write.

### Quick example

```rust
use oram::{ObliviousHistogram, AutoResizeConfig};
use oram::DEFAULT_STASH_OVERFLOW_SIZE;

let mut rng = rand::rngs::OsRng;
// Z=4 (bucket size), T=16 (tag size in bytes), P=0 (payload size in bytes)
let mut hist = ObliviousHistogram::<4, 16, 0>::new(
    16, // capacity in blocks
    &mut rng,
    DEFAULT_STASH_OVERFLOW_SIZE,
)?;

hist.enable_auto_resize(AutoResizeConfig {
    t_capacity: 32,
    eps: 1.0,
    delta: 1e-6,
    alpha: 0.05,
    r: 1,
    seed: 7,
});

for word in ["apple", "banana", "apple", "cherry"] {
    hist.increment(word.as_bytes());
}
assert_eq!(hist.read_total(b"apple"), 2);
```

### Sharded example

```rust
use oram::ShardedObliviousHistogram;
use oram::DEFAULT_STASH_OVERFLOW_SIZE;

let mut rng = rand::rngs::OsRng;
let shard_count = 4;
let batch_size = 1024;
// Compute suggested shard quota for 80-bit security
let per_shard_quota = ShardedObliviousHistogram::<16, 16, 0>::suggested_per_shard_quota(
    batch_size,
    shard_count,
    80,
);

// Z=16 (bucket size), T=16 (tag size in bytes), P=0 (payload size in bytes)
let mut sharded = ShardedObliviousHistogram::<16, 16, 0>::new(
    shard_count,
    1 << 20, // total capacity in blocks
    batch_size,
    per_shard_quota,
    &mut rng,
    DEFAULT_STASH_OVERFLOW_SIZE,
)?;

sharded.increment(b"some-key");
sharded.increment_batch(&[b"key-a", b"key-b", b"key-c"]);
sharded.flush();
```

### Benchmarks

The benchmark suite comparison is defined under `benches/oram_benchmarks.rs`.
Specific experiments can be run by passing the experiment name as an argument.
Results are printed to stdout and saved as CSV files in the `target/` directory.

Instructions TBD

### Code layout

Within `src/`:

-   `lib.rs`: public surface (`ObliviousHistogram`, `ShardedObliviousHistogram`,
    `OramError`).
-   `metrics.rs`: profiling metrics for execution timing.
-   `oblivious/`: core oblivious primitives (constant-time select, sorting,
    compaction, reduction, crypto).
-   `oblivious_histogram/`: single-tree Path OSAM implementation:
    -   `mod.rs`: `ObliviousHistogram` struct, construction, accessors, and
        export.
    -   `ops.rs`: operation pipeline (insert / read / flush / eviction).
    -   `resize.rs`: resize decision (DP Laplace noise) and mechanism (grow /
        fill).
    -   `tree.rs`: complete-binary-tree index math.
    -   `bucket.rs`: bucket structure.
    -   `stash.rs`: stash structure.
    -   `routing.rs`: PRF routing.
-   `sharded_oblivious_histogram/`: parallel ORAM sharding:
    -   `mod.rs`: `ShardedObliviousHistogram` struct and flush pipeline.
    -   `router.rs`: oblivious routing of updates.

### Resources

The library descends from Meta's Path ORAM implementation
([github.com/facebook/oram](https://github.com/facebook/oram)) and inherits its
core stash/bucket primitives. The address-indexed Path ORAM and its recursive
position map have been removed; only the PRF-keyed OSAM remains.

Background reading:

-   [Path ORAM (Stefanov et al., 2013)](https://eprint.iacr.org/2013/280.pdf)
-   [Oblix (Mishra et al., 2018)](https://people.eecs.berkeley.edu/~raluca/oblix.pdf)

### License

Dual-licensed under either the [MIT license](./LICENSE-MIT) or the
[Apache License, Version 2.0](./LICENSE-APACHE), at your option.
