load("//third_party/bazel_rules/rules_rust/rust:defs.bzl", "rust_library")

package(default_visibility = ["//visibility:public"])

rust_library(
    name = "oram",
    srcs = glob(["src/**/*.rs"]),
    edition = "2021",
    deps = [
        "//third_party/rust/log/v0_4:log",
        "//third_party/rust/rand/v0_10:rand",
        "//third_party/rust/subtle/v2:subtle",
        "//third_party/rust/thiserror/v1:thiserror",
    ],
)
