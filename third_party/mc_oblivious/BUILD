load("//third_party/bazel_rules/rules_rust/rust:defs.bzl", "rust_library")

package(default_visibility = ["//visibility:public"])

rust_library(
    name = "aligned_array",
    srcs = glob(["aligned-array/src/**/*.rs"]),
    crate_features = ["subtle"],
    edition = "2018",
    deps = [
        "//third_party/rust/generic_array/v0_14:generic_array",
        "//third_party/rust/subtle/v2:subtle",
    ],
)

rust_library(
    name = "aligned_cmov",
    srcs = glob(["aligned-cmov/src/**/*.rs"]),
    edition = "2018",
    deps = [
        ":aligned_array",
        "//third_party/rust/generic_array/v0_14:generic_array",
        "//third_party/rust/subtle/v2:subtle",
        "//third_party/rust/typenum/v1:typenum",
    ],
)

rust_library(
    name = "balanced_tree_index",
    srcs = glob(["balanced-tree-index/src/**/*.rs"]),
    edition = "2018",
    deps = [
        ":aligned_cmov",
        "//third_party/rust/rand_core/v0_6:rand_core",
    ],
)

rust_library(
    name = "mc_oblivious_traits",
    srcs = glob(["mc-oblivious-traits/src/**/*.rs"]),
    edition = "2018",
    deps = [
        ":aligned_cmov",
        ":balanced_tree_index",
        "//third_party/rust/generic_array/v0_14:generic_array",
        "//third_party/rust/rand_core/v0_6:rand_core",
        "//third_party/rust/typenum/v1:typenum",
    ],
)

rust_library(
    name = "mc_oblivious_ram",
    srcs = glob(["mc-oblivious-ram/src/**/*.rs"]),
    edition = "2018",
    deps = [
        ":aligned_cmov",
        ":balanced_tree_index",
        ":mc_oblivious_traits",
        "//third_party/rust/generic_array/v0_14:generic_array",
        "//third_party/rust/rand_core/v0_6:rand_core",
        "//third_party/rust/typenum/v1:typenum",
    ],
)

rust_library(
    name = "mc_oblivious_map",
    srcs = glob(["mc-oblivious-map/src/**/*.rs"]),
    edition = "2018",
    deps = [
        ":aligned_cmov",
        ":mc_oblivious_ram",
        ":mc_oblivious_traits",
        "//third_party/rust/generic_array/v0_14:generic_array",
        "//third_party/rust/rand_core/v0_6:rand_core",
        "//third_party/rust/siphasher/v0_3:siphasher",
        "//third_party/rust/typenum/v1:typenum",
    ],
)
