#![allow(clippy::expect_used)]

use std::process::Command;

use divan::Bencher;

fn main() {
    divan::main();
}

/// Exercises the Bazel-backed end-to-end benchmark path with a cheap,
/// deterministic Dex invocation. Richer scenarios can add separate
/// benchmark binaries without making the shared harness depend on them.
#[divan::bench(sample_count = 20, sample_size = 1)]
fn dex_help(bencher: Bencher) {
    let dex = dex_utils_cargo_bin::cargo_bin("dex")
        .expect("dex binary should be available through Bazel runfiles");

    bencher.bench_local(move || {
        let output = Command::new(&dex)
            .arg("--help")
            .output()
            .expect("dex --help should run");
        assert!(output.status.success(), "dex --help should succeed");
    });
}
