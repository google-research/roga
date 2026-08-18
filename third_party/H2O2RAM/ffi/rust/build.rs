use std::env;
use std::fs;
use std::path::PathBuf;

/// Read once, pushed to both the C++ side (-D) and the Rust side (consts).
/// A mismatch would be a buffer overrun, so lib.rs asserts they agree.
fn param(name: &str, default: &str) -> usize {
    println!("cargo:rerun-if-env-changed={name}");
    let v = env::var(name).unwrap_or_else(|_| default.to_string());
    v.parse().unwrap_or_else(|_| panic!("{name} must be a positive integer, got {v:?}"))
}

fn main() {
    let key_max = param("H2O2RAM_KEY_MAX", "32");
    let val_size = param("H2O2RAM_VAL_SIZE", "32");
    let capacity = param("H2O2RAM_CAPACITY", "65536");

    if (key_max + val_size) % 8 != 0 {
        panic!(
            "H2O2RAM_KEY_MAX + H2O2RAM_VAL_SIZE must be a multiple of 8 \
             (got {key_max} + {val_size}); otherwise the ORAM block picks up \
             padding and every block carries uninitialised bytes"
        );
    }

    // The crate lives at ffi/rust, so the CMake project is one level up.
    let ffi_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("..")
        .canonicalize()
        .expect("ffi directory not found");
    let root = ffi_dir.join("..");

    // Named individually: ffi/ holds this crate's target/, and watching the
    // whole tree would make every build dirty the next one.
    for f in [
        ffi_dir.join("CMakeLists.txt"),
        ffi_dir.join("h2o2ram_ffi.cpp"),
        ffi_dir.join("h2o2ram_ffi.h"),
        root.join("include"),
        root.join("src"),
    ] {
        println!("cargo:rerun-if-changed={}", f.display());
    }

    let dst = cmake::Config::new(&ffi_dir)
        .define("H2O2RAM_KEY_MAX", key_max.to_string())
        .define("H2O2RAM_VAL_SIZE", val_size.to_string())
        .define("H2O2RAM_CAPACITY", capacity.to_string())
        .build();

    println!("cargo:rustc-link-search=native={}/lib", dst.display());
    println!("cargo:rustc-link-lib=static=h2o2ram_ffi");

    // C++ static lib: TBB, OpenMP, OpenSSL for the key hash, nlopt via
    // ohash_bucket.hpp.
    println!("cargo:rustc-link-lib=dylib=stdc++");
    println!("cargo:rustc-link-lib=dylib=tbb");
    println!("cargo:rustc-link-lib=dylib=crypto");
    println!("cargo:rustc-link-lib=dylib=nlopt");
    println!("cargo:rustc-link-lib=dylib=gomp");

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(
        out.join("consts.rs"),
        format!(
            "/// Maximum key length in bytes. Keys may be shorter.\n\
             pub const KEY_MAX: usize = {key_max};\n\
             /// Exact value length in bytes.\n\
             pub const VAL_SIZE: usize = {val_size};\n"
        ),
    )
    .unwrap();
}
