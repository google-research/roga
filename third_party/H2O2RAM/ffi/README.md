# FFI bindings for the H2O2RAM oblivious map

Everything needed to use the oblivious map from Rust lives in this directory.

```
ffi/
  h2o2ram_ffi.h     C ABI
  h2o2ram_ffi.cpp   shim over ORAM::ObliviousMap
  CMakeLists.txt    builds libh2o2ram_ffi.a
  rust/             the h2o2ram crate, drives the CMake build from build.rs
```

## Usage

```rust
use h2o2ram::{ObliviousMap, VAL_SIZE};

let mut map = ObliviousMap::new()?;
map.insert(b"some key", &[7u8; VAL_SIZE])?;
assert_eq!(map.get(b"some key")?, Some([7u8; VAL_SIZE]));
assert_eq!(map.get(b"absent")?, None);
```

`cargo build` from `ffi/rust` handles the C++ build. Requires TBB, OpenMP,
OpenSSL and NLopt, the same dependencies as the main project.

## Compile time parameters

Set as environment variables at build time. They are baked into both the C++
library and the Rust consts, and `ObliviousMap::new` asserts the two agree.

Variable           | Default | Meaning
------------------ | ------- | ------------------------------------------------
`H2O2RAM_KEY_MAX`  | 32      | maximum key length in bytes, keys may be shorter
`H2O2RAM_VAL_SIZE` | 32      | exact value length in bytes
`H2O2RAM_CAPACITY` | 65536   | max entries per map, rounded up to `128 * 2^k`

`H2O2RAM_KEY_MAX + H2O2RAM_VAL_SIZE` must be a multiple of 8, otherwise the ORAM
block picks up padding and carries uninitialised bytes.

```sh
H2O2RAM_KEY_MAX=64 H2O2RAM_VAL_SIZE=64 H2O2RAM_CAPACITY=1048576 cargo build
```

## Things worth knowing

**Only insert and lookup.** Deletion is not exposed. `ObliviousRAM::erase`
extracts an element without decrementing the size and does not behave as a
delete, so offering it would be misleading.

**Capacity is hard.** A map holds at most `capacity()` distinct keys. Inserting
a new key past that returns `Error::Full`; overwriting an existing key always
works and consumes no capacity. The limit is enforced here because the
underlying structure silently loses data once its block count is exceeded.

**First run on a machine autotunes.** The hash planner looks for a
`hash_map.bin<BlockSize>` tuning file next to the running executable. When it is
absent it benchmarks the alternatives and writes one, which takes on the order
of tens of seconds. This is per machine and per block size, and the block size
changes whenever the key or value size changes.

## Controlling cores

```rust
let mut map = ObliviousMap::with_threads(8)?;   // 0 = whole machine
```

The cap applies to construction and every later operation. Both runtimes the
library parallelises through are covered: TBB, which backs
`std::execution::par_unseq`, via a per-map `task_arena`, and OpenMP, whose
thread count is a per-thread setting applied and restored around each call.
Neither responds to a single env var on its own, and oneTBB has no thread count
env var at all, which is why this is a parameter.

It is a ceiling, not a target. Measured on a 72-core machine, 40k inserts:

requested        | cores used
---------------- | ----------
1                | 1.0
4                | 2.1
16               | 4.8
0 (unrestricted) | 15.9

## Concurrency

**One map** cannot be used concurrently. Every operation mutates ORAM state,
lookups included, so the type is `Send` but not `Sync` and `&mut self` on both
methods rejects it at compile time. Wrap in a `Mutex` to share.

**Separate maps** are independent in steady state. Construction is not: it
reaches `determine_hash`, which mutates an unsynchronised static plan cache and
the global `DepthCounter`, so the shim serialises `h2o2ram_map_new` behind a
process-wide mutex. Concurrent construction is correct, it just does not
overlap.

## Notes on the implementation

Three properties of the underlying library shape the shim.

*Insert is a read-modify-write.* `ObliviousRAM::insert` appends a second block
under the same id instead of replacing, and after a rebuild a lookup can return
either. `operator[]` extracts the existing element first, so only one copy
exists. It also makes insert and lookup indistinguishable in the access pattern.

*Slots are tagged with their index.* A miss returns an uninitialised block, and
that memory is recycled, so it is usually a verbatim copy of another key's slot.
The stored index separates recycled data, carrying a different index, from a
genuine collision, carrying the same one.

*Unwanted blocks are handed back.* A miss materialises a block that would
otherwise occupy a capacity slot forever, so the shim marks it with the
library's own dead-block marker for the ORAM to reclaim.

Slot contents and return codes are selected with `CMOV`, so control flow never
depends on whether a key was present.
