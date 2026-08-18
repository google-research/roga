//! Rust bindings for the H2O2RAM oblivious map.
//!
//! Insert and lookup only. Deletion is not exposed: `ObliviousRAM::erase`
//! extracts an element without decrementing the size, so it is not a delete.
//!
//! Keys vary up to [`KEY_MAX`], values are exactly [`VAL_SIZE`] bytes. Set
//! both, and `H2O2RAM_CAPACITY`, as env vars at build time.
//!
//! ```no_run
//! use h2o2ram::{ObliviousMap, VAL_SIZE};
//!
//! let mut map = ObliviousMap::new().unwrap();
//! map.insert(b"some key", &[7u8; VAL_SIZE]).unwrap();
//! assert_eq!(map.get(b"some key").unwrap(), Some([7u8; VAL_SIZE]));
//! assert_eq!(map.get(b"absent").unwrap(), None);
//! ```
//!
//! # Capacity
//!
//! At most [`ObliviousMap::capacity`] distinct keys, fixed at creation. Past
//! that a *new* key returns [`Error::Full`]; overwrites always succeed. Not
//! advisory: the structure silently loses data if exceeded, so this wrapper
//! counts and refuses.
//!
//! # Cost of construction
//!
//! [`ObliviousMap::new`] builds the whole table hierarchy up front, and
//! autotunes first if this machine has no tuning file for the block size,
//! writing `hash_map.bin<N>` next to the executable.

#![deny(missing_docs)]

use std::os::raw::{c_int, c_uint};

include!("consts.rs");

#[repr(C)]
struct RawMap {
    _private: [u8; 0],
}

extern "C" {
    fn h2o2ram_key_max() -> usize;
    fn h2o2ram_val_size() -> usize;
    fn h2o2ram_capacity() -> usize;
    fn h2o2ram_map_new(threads: c_uint) -> *mut RawMap;
    fn h2o2ram_map_free(map: *mut RawMap);
    fn h2o2ram_threads(map: *const RawMap) -> c_uint;
    fn h2o2ram_len(map: *const RawMap) -> usize;
    fn h2o2ram_insert(map: *mut RawMap, key: *const u8, key_len: usize, val: *const u8) -> c_int;
    fn h2o2ram_get(map: *mut RawMap, key: *const u8, key_len: usize, val_out: *mut u8) -> c_int;
}

const RC_OK: c_int = 0;
const RC_NOT_FOUND: c_int = 1;
const RC_FULL: c_int = 2;
const RC_COLLISION: c_int = 3;

/// Errors returned by map operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Key was empty or longer than [`KEY_MAX`].
    InvalidKey,
    /// The map is at capacity and the key is not already present.
    Full,
    /// Two distinct keys hit the same 63 bit index.
    ///
    /// The stored entry is kept and the new one rejected rather than one
    /// clobbering the other. ~5e-8 at 1e6 keys.
    Collision,
    /// The C++ side could not allocate the map.
    OutOfMemory,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Error::InvalidKey => "key must be between 1 and KEY_MAX bytes",
            Error::Full => "map is at capacity",
            Error::Collision => "key index collided with a different stored key",
            Error::OutOfMemory => "could not allocate the map",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for Error {}

/// A fixed capacity oblivious key/value map.
///
/// # Parallelism
///
/// Not `Sync`: every operation mutates ORAM state, lookups included. It is
/// `Send`, and `&mut self` on `insert` and `get` rejects concurrent use at
/// compile time. Wrap in a `Mutex` to share.
///
/// Separate maps run in parallel fine. Creating them in parallel is safe but
/// serialised, the C++ planner has unsynchronised globals.
///
/// One call is itself parallel across cores. Cap it with
/// [`ObliviousMap::with_threads`].
pub struct ObliviousMap {
    raw: *mut RawMap,
}

// Safety: owns all its state, no thread affinity. Deliberately not Sync.
unsafe impl Send for ObliviousMap {}

impl ObliviousMap {
    /// Empty map preallocated to [`ObliviousMap::capacity`] entries.
    ///
    /// Expensive, very much so on first run. See the module docs on autotuning.
    ///
    /// # Panics
    ///
    /// If the linked C++ lib was built with a different key or value size.
    /// That would be a buffer overrun, so it is checked, not trusted.
    pub fn new() -> Result<Self, Error> {
        Self::with_threads(0)
    }

    /// As [`ObliviousMap::new`], capped at `threads` cores for construction and
    /// every later operation. `0` means the whole machine.
    ///
    /// Covers both TBB and OpenMP. Use it when several maps, or the map and the
    /// rest of your program, would otherwise oversubscribe the machine.
    pub fn with_threads(threads: u32) -> Result<Self, Error> {
        let (c_key, c_val) = unsafe { (h2o2ram_key_max(), h2o2ram_val_size()) };
        assert_eq!(
            c_key, KEY_MAX,
            "linked h2o2ram_ffi was built with KEY_MAX={c_key} but this crate \
             expects {KEY_MAX}; rebuild with a matching H2O2RAM_KEY_MAX"
        );
        assert_eq!(
            c_val, VAL_SIZE,
            "linked h2o2ram_ffi was built with VAL_SIZE={c_val} but this crate \
             expects {VAL_SIZE}; rebuild with a matching H2O2RAM_VAL_SIZE"
        );

        let raw = unsafe { h2o2ram_map_new(threads as c_uint) };
        if raw.is_null() {
            return Err(Error::OutOfMemory);
        }
        Ok(Self { raw })
    }

    /// Cores this map may use, resolved to the real count when unrestricted.
    pub fn threads(&self) -> u32 {
        unsafe { h2o2ram_threads(self.raw) as u32 }
    }

    /// Max distinct keys, the requested capacity rounded up to `128 * 2^k`.
    pub fn capacity(&self) -> usize {
        unsafe { h2o2ram_capacity() }
    }

    /// Number of distinct keys currently stored.
    pub fn len(&self) -> usize {
        unsafe { h2o2ram_len(self.raw) }
    }

    /// Whether the map holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Inserts or overwrites `key`. Overwrites consume no capacity.
    ///
    /// [`Error::Full`] if the map is full and `key` is new,
    /// [`Error::InvalidKey`] if `key` is empty or longer than [`KEY_MAX`].
    pub fn insert(&mut self, key: &[u8], val: &[u8; VAL_SIZE]) -> Result<(), Error> {
        let rc = unsafe { h2o2ram_insert(self.raw, key.as_ptr(), key.len(), val.as_ptr()) };
        match rc {
            RC_OK => Ok(()),
            RC_FULL => Err(Error::Full),
            RC_COLLISION => Err(Error::Collision),
            _ => Err(Error::InvalidKey),
        }
    }

    /// Looks up `key`. `Ok(None)` for absent, which is not an error.
    ///
    /// Takes `&mut self` because a lookup mutates ORAM state.
    pub fn get(&mut self, key: &[u8]) -> Result<Option<[u8; VAL_SIZE]>, Error> {
        let mut out = [0u8; VAL_SIZE];
        let rc = unsafe { h2o2ram_get(self.raw, key.as_ptr(), key.len(), out.as_mut_ptr()) };
        match rc {
            RC_OK => Ok(Some(out)),
            RC_NOT_FOUND => Ok(None),
            _ => Err(Error::InvalidKey),
        }
    }
}

impl Drop for ObliviousMap {
    fn drop(&mut self) {
        unsafe { h2o2ram_map_free(self.raw) }
    }
}

impl std::fmt::Debug for ObliviousMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObliviousMap")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .finish()
    }
}
