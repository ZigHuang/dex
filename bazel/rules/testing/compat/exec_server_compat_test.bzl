"""Bazel coverage for app-server and exec-server release compatibility."""

load("//:defs.bzl", "workspace_root_test")

def exec_server_compat_test(
        name,
        comparison_binary = None,
        current_binary = "//dex-rs/cli:dex",
        release = None):
    """Tests both executor directions against another Dex build or release.

    Args:
        name: Name of the generated compatibility test target.
        comparison_binary: Built Dex executable to compare with the current build.
        current_binary: Built Dex executable representing the current version.
        release: External release repository exposing `:dex` and `:package`.
    """
    if (comparison_binary == None) == (release == None):
        fail("exactly one of comparison_binary and release must be set")

    comparison = comparison_binary if release == None else release + "//:dex"
    data = [] if release == None else [release + "//:package"]
    comparison_alias = name + "-comparison-binary"
    native.alias(
        name = comparison_alias,
        actual = comparison,
        tags = ["manual"],
        visibility = ["//visibility:private"],
    )

    workspace_root_test(
        name = name,
        args = ["--test-threads=1"],
        data = data,
        runfile_env = {
            "//dex-rs/bwrap:bwrap": "CARGO_BIN_EXE_bwrap",
            current_binary: "DEX_TEST_CURRENT_DEX",
            ":" + comparison_alias: "DEX_TEST_RELEASED_DEX",
        },
        tags = ["no-sandbox"],
        target_compatible_with = [
            "@platforms//cpu:x86_64",
            "@platforms//os:linux",
        ],
        test_bin = "//bazel/rules/testing/compat:exec-server-compat-test-bin",
        workspace_root_marker = "//dex-rs/utils/cargo-bin:repo_root.marker",
    )
