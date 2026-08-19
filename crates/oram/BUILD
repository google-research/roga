load("//third_party/bazel_rules/rules_python/python:py_binary.bzl", "py_binary")
load("//third_party/bazel_rules/rules_rust/rust:defs.bzl", "rust_binary", "rust_library", "rust_test")

package(default_visibility = ["//visibility:public"])

rust_library(
    name = "oram",
    srcs = glob(
        ["src/**/*.rs"],
        exclude = ["src/bin/**"],
    ),
    edition = "2021",
    rustc_flags = [
        "-C",
        "target-cpu=x86-64-v3",
        "-C",
        "opt-level=3",
    ],
    deps = [
        "//third_party/rust/aes/v0_8:aes",
        "//third_party/rust/cmov/v0_5:cmov",
        "//third_party/rust/rand/v0_10:rand",
        "//third_party/rust/rand_chacha/v0_10:rand_chacha",
        "//third_party/rust/rayon/v1:rayon",
    ],
)

# Profile-enabled build of the library: identical to :oram but with the `profile`
# feature on, so the in-library `timing_scope!` instrumentation is compiled in.
# Depend on this (not :oram) from anything that reads OramMetrics timings.
rust_library(
    name = "oram_profile",
    srcs = glob(
        ["src/**/*.rs"],
        exclude = ["src/bin/**"],
    ),
    crate_features = ["profile"],
    edition = "2021",
    rustc_flags = [
        "-C",
        "target-cpu=x86-64",
        "-C",
        "opt-level=3",
    ],
    deps = [
        "//third_party/rust/aes/v0_8:aes",
        "//third_party/rust/cmov/v0_5:cmov",
        "//third_party/rust/rand/v0_10:rand",
        "//third_party/rust/rand_chacha/v0_10:rand_chacha",
        "//third_party/rust/rayon/v1:rayon",
    ],
)

rust_test(
    name = "ct_test",
    srcs = ["tests/ct.rs"],
    edition = "2021",
    deps = [
        ":oram",
        "//third_party/rust/aes/v0_8:aes",
        "//third_party/rust/cmov/v0_5:cmov",
        "//third_party/rust/generic_array/v0_14:generic_array",
        "//third_party/rust/rand/v0_10:rand",
    ],
)

rust_test(
    name = "keyed_histogram_test",
    srcs = ["tests/keyed_histogram.rs"],
    edition = "2021",
    deps = [
        ":oram",
        "//third_party/rust/rand/v0_10:rand",
    ],
)

rust_test(
    name = "sharded_histogram_test",
    srcs = ["tests/sharded_histogram.rs"],
    edition = "2021",
    deps = [
        ":oram",
        "//third_party/rust/rand/v0_10:rand",
    ],
)

rust_test(
    name = "binomial_solver_test",
    crate = ":oram",
    edition = "2021",
)

py_binary(
    name = "update_overleaf_data",
    srcs = ["scripts/update_overleaf_data.py"],
)

rust_binary(
    name = "verification_harness",
    srcs = ["verify/harness.rs"],
    edition = "2021",
    rustc_flags = [
        "-C",
        "target-cpu=x86-64",
        "-C",
        "target-feature=-avx,-avx2",
        "-C",
        "opt-level=3",
        "-C",
        "overflow-checks=off",
        # For binsec: debug symbols and no stack protector
        "-C",
        "debuginfo=2",
        "-C",
        "link-arg=-fno-stack-protector",
    ],
    deps = [
        ":oram",
        "//third_party/rust/cmov/v0_5:cmov",
    ],
)
