load("//third_party/bazel_rules/rules_cc/cc:cc_library.bzl", "cc_library")
load("//third_party/bazel_rules/rules_rust/bindgen:defs.bzl", "rust_bindgen")
load("//third_party/bazel_rules/rules_rust/rust:defs.bzl", "rust_library")

package(default_visibility = ["//visibility:public"])

cc_library(
    name = "omap_c",
    srcs = [
        "omap/common/encutils.cpp",
        "omap/common/tracing/tracer.cpp",
        "omap/external_memory/server/serverBackend.cpp",
        "omap/interface/interface.cpp",
        "omap/interface/omap_c.cpp",
        "omap/interface/par_omap_c.cpp",
    ],
    hdrs = [
        "omap/interface/common_interface.hpp",
        "omap/interface/omap_all_c.h",
        "omap/interface/omap_c.h",
        "omap/interface/omap_spec_interface.hpp",
        "omap/interface/par_omap_c.h",
        "omap/interface/par_omap_interface.hpp",
        "omap/interface/recoram_interface.hpp",
        "omap/interface/wrapper.h",
    ],
    copts = [
        "-O3",
        "-std=c++20",
        "-fopenmp",
        "-maes",
        "-mpclmul",
        "-msse4.1",
        "-mavx2",
    ],
    includes = [
        "omap",
    ],
    textual_hdrs = glob(
        [
            "omap/**/*.cfg",
            "omap/**/*.h",
            "omap/**/*.hpp",
            "omap/**/*.hxx",
        ],
        exclude = [
            "omap/interface/common_interface.hpp",
            "omap/interface/omap_c.h",
            "omap/interface/omap_spec_interface.hpp",
            "omap/interface/par_omap_interface.hpp",
            "omap/interface/recoram_interface.hpp",
        ],
    ),
    deps = [
        "//third_party/llvm_openmp",
        "//third_party/openssl:crypto",
    ],
)

rust_bindgen(
    name = "omap_bindings_gen",
    bindgen_flags = [
        "--no-layout-tests",
        "--with-derive-default",
        "--allowlist-type",
        "ParOMapBinding",
        "--allowlist-type",
        "Key16",
    ],
    cc_lib = ":omap_c",
    clang_flags = [
        "-x",
        "c++",
        "-std=c++20",
        "-D__BINDGEN__",
    ],
    header = "omap/interface/wrapper.h",
)

rust_library(
    name = "obliviouslabs_oram",
    srcs = [":omap_bindings_gen"],
    edition = "2021",
    deps = [
        ":omap_c",
    ],
)
