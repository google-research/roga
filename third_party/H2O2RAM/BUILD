load("//third_party/bazel_rules/rules_cc/cc:cc_binary.bzl", "cc_binary")
load("//third_party/bazel_rules/rules_cc/cc:cc_library.bzl", "cc_library")
load("//third_party/bazel_rules/rules_rust/rust:defs.bzl", "rust_library")

package(default_visibility = ["//visibility:public"])

cc_library(
    name = "h2o2ram_c",
    srcs = [
        "ffi/h2o2ram_ffi.cpp",
        "src/depthCounter.cpp",
        "src/oblivious_operations.cpp",
        "src/timer.cpp",
    ],
    hdrs = ["ffi/h2o2ram_ffi.h"],
    copts = [
        "-O3",
        "-std=c++20",
        "-fno-strict-aliasing",
        "-fopenmp",
        "-maes",
        "-mpclmul",
        "-msse4.1",
        "-mavx2",
        "-DPARALLEL",
        "-fexperimental-library",
    ],
    defines = [
        "H2O2RAM_KEY_MAX=16",
        "H2O2RAM_VAL_SIZE=8",
        "MIN_CAPACITY=524288",
    ],
    features = [
        "-use_header_modules",
    ],
    includes = [
        "ffi",
        "include",
    ],
    textual_hdrs = glob([
        "ffi/**/*.h",
        "include/**/*.h",
        "include/**/*.hpp",
    ]),
    deps = [
        "//third_party/llvm_openmp",
        "//third_party/nlopt",
        "//third_party/openssl:crypto",
        "//third_party/tbb",
    ],
)

rust_library(
    name = "h2o2ram_oram",
    srcs = [
        "ffi/rust/src/consts.rs",
        "ffi/rust/src/lib.rs",
    ],
    edition = "2021",
    deps = [
        ":h2o2ram_c",
    ],
)

alias(
    name = "h2o2ram",
    actual = ":h2o2ram_oram",
)

cc_binary(
    name = "stock_quick_bench",
    srcs = ["stock_quick_bench.cpp"],
    copts = [
        "-O3",
        "-std=c++20",
        "-fno-strict-aliasing",
        "-fopenmp",
        "-maes",
        "-mpclmul",
        "-msse4.1",
        "-mavx2",
        "-DPARALLEL",
        "-fexperimental-library",
    ],
    features = [
        "-use_header_modules",
    ],
    deps = [
        ":h2o2ram_c",
    ],
)

cc_binary(
    name = "stock_benchmark_oram128",
    srcs = ["stock_benchmark_oram128.cpp"],
    copts = [
        "-O3",
        "-std=c++20",
        "-fno-strict-aliasing",
        "-fopenmp",
        "-maes",
        "-mpclmul",
        "-msse4.1",
        "-mavx2",
        "-DPARALLEL",
        "-fexperimental-library",
    ],
    features = [
        "-use_header_modules",
    ],
    deps = [
        ":h2o2ram_c",
        "//third_party/benchmark",
    ],
)

cc_binary(
    name = "stock_thread_sweep",
    srcs = ["stock_thread_sweep.cpp"],
    copts = [
        "-O3",
        "-std=c++20",
        "-fno-strict-aliasing",
        "-fopenmp",
        "-maes",
        "-mpclmul",
        "-msse4.1",
        "-mavx2",
        "-DPARALLEL",
        "-fexperimental-library",
    ],
    features = [
        "-use_header_modules",
    ],
    deps = [
        ":h2o2ram_c",
        "//third_party/llvm_openmp",
        "//third_party/tbb",
    ],
)

cc_binary(
    name = "run_stock_hash_planner",
    srcs = ["run_stock_hash_planner.cpp"],
    copts = [
        "-O3",
        "-std=c++20",
        "-fno-strict-aliasing",
        "-fopenmp",
        "-maes",
        "-mpclmul",
        "-msse4.1",
        "-mavx2",
        "-DPARALLEL",
        "-fexperimental-library",
    ],
    features = [
        "-use_header_modules",
    ],
    deps = [
        ":h2o2ram_c",
        "//third_party/llvm_openmp",
        "//third_party/tbb",
    ],
)
