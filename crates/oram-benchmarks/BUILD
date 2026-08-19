# Force rebuild 2026-07-30
load("//third_party/bazel_rules/rules_rust/rust:defs.bzl", "rust_binary")

package(default_visibility = ["//visibility:public"])

rust_binary(
    name = "oram_benchmarks",
    srcs = [
        "benches/oram_benchmarks.rs",
    ] + glob(["benches/bench_support/**/*.rs"]),
    aliases = {
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/rust/oram/v0_2:oram": "facebook_oram",
    },
    crate_features = [
        "facebook-baseline",
        "h2o2ram-baseline",
        "obliviouslabs-baseline",
        "mc-oblivious-baseline",
    ],
    edition = "2021",
    rustc_flags = [
        "-C",
        "target-cpu=x86-64-v3",
        "-C",
        "opt-level=3",
        "-D",
        "warnings",
    ],
    deps = [
        "//experimental/users/lukasgerlach/osam-histogramm/crates/oram",
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/H2O2RAM:h2o2ram_oram",
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/mc_oblivious:aligned_cmov",
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/mc_oblivious:mc_oblivious_map",
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/mc_oblivious:mc_oblivious_ram",
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/mc_oblivious:mc_oblivious_traits",
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/obliviouslabs_oram",
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/rust/oram/v0_2:oram",
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/rust/rand_distr/v0_6:rand_distr",
        "//third_party/rust/aes/v0_8:aes",
        "//third_party/rust/cmov/v0_5:cmov",
        "//third_party/rust/etherparse/v0_18:etherparse",
        "//third_party/rust/pcap/v2:pcap",
        "//third_party/rust/rand/v0_10:rand",
        "//third_party/rust/rand_core/v0_6:rand_core",
        "//third_party/rust/rayon/v1:rayon",
        "//third_party/rust/serde/v1:serde",
        "//third_party/rust/serde_json/v1:serde_json",
        "//third_party/rust/typenum/v1:typenum",
    ],
)

rust_binary(
    name = "case_study_sim",
    srcs = [
        "benches/case_study_sim.rs",
    ],
    edition = "2021",
    rustc_flags = [
        "-C",
        "target-cpu=x86-64-v3",
        "-C",
        "opt-level=3",
        "-D",
        "warnings",
    ],
    deps = [
        "//experimental/users/lukasgerlach/osam-histogramm/crates/oram",
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/rust/rand_distr/v0_6:rand_distr",
        "//third_party/rust/rand/v0_10:rand",
    ],
)

rust_binary(
    name = "mawi_replay",
    srcs = [
        "benches/mawi_replay.rs",
    ],
    edition = "2021",
    rustc_flags = [
        "-C",
        "target-cpu=x86-64-v3",
        "-C",
        "opt-level=3",
        "-D",
        "warnings",
    ],
    deps = [
        "//experimental/users/lukasgerlach/osam-histogramm/crates/oram",
        "//experimental/users/lukasgerlach/osam-histogramm/third_party/rust/rand_distr/v0_6:rand_distr",
        "//third_party/rust/cmov/v0_5:cmov",
        "//third_party/rust/etherparse/v0_18:etherparse",
        "//third_party/rust/libc/v0_2:libc",
        "//third_party/rust/pcap/v2:pcap",
        "//third_party/rust/rand/v0_10:rand",
    ],
)
