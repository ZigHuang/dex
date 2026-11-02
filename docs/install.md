## Installing & building

### GitHub Releases

Standalone installers and complete platform packages are published to
[ZigHuang/dex Releases](https://github.com/ZigHuang/dex/releases). The installers
download packages from GitHub by default and verify their SHA-256 checksums.

The `Dex release` workflow uses GitHub-hosted runners to build Linux, macOS, and
Windows packages for x86_64 and arm64. Run it manually to build downloadable
Actions artifacts. To publish a release, set the Cargo workspace version to the
upstream version and push the matching tag:

```bash
git tag rust-v0.166.0
git push origin rust-v0.166.0
```

Tag builds upload all six complete packages, `dex-package_SHA256SUMS`,
`install.sh`, and `install.ps1` to GitHub Releases. Each package includes the
CLI, code-mode host, executor, and platform resources required for normal startup.

If an unpublished release fails because of its CI configuration, fix the
workflow on `main` and push a tag such as `rebuild-1-rust-v0.166.0`. This runs the
updated workflow against the original `rust-v0.166.0` source commit and stops
earlier attempts for that version. Use another rebuild number for further retries.
Published releases require a new version.

### System requirements

| Requirement                 | Details                                                         |
| --------------------------- | --------------------------------------------------------------- |
| Operating systems           | macOS 12+, Ubuntu 20.04+/Debian 10+, or Windows 11 **via WSL2** |
| Git (optional, recommended) | 2.23+ for built-in PR helpers                                   |
| RAM                         | 4-GB minimum (8-GB recommended)                                 |

### DotSlash

The GitHub Release also contains a [DotSlash](https://dotslash-cli.com/) file for the Dex CLI named `dex`. Using a DotSlash file makes it possible to make a lightweight commit to source control to ensure all contributors use the same version of an executable, regardless of what platform they use for development.

### Build from source

```bash
# Clone the repository and navigate to the root of the Cargo workspace.
git clone https://github.com/ZigHuang/dex.git
cd dex/dex-rs

# Install the Rust toolchain, if necessary.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add rustfmt
rustup component add clippy
# Install helper tools used by the workspace justfile:
cargo install --locked just
# DotSlash fetches pinned development tools such as buildifier on first use.
cargo install --locked dotslash
# Install nextest for the `just test` helper.
cargo install --locked cargo-nextest

# Build Dex.
cargo build

# Launch the TUI with a sample prompt.
cargo run --bin dex -- "explain this codebase to me"

# After making changes, use the root justfile helpers (they default to dex-rs):
just fmt
just fix -p <crate-you-touched>

# Run the relevant tests (project-specific is fastest), for example:
just test -p dex-tui
# `just test` runs the test suite via nextest:
just test
# Avoid `--all-features` for routine local runs because it increases build
# time and `target/` disk usage by compiling additional feature combinations.
```

## Tracing / verbose logging

Dex is written in Rust, so it honors the `RUST_LOG` environment variable to configure its logging behavior.

The TUI records diagnostics in bounded local stores by default. Set `log_dir` explicitly to enable a plaintext TUI log for a run:

```bash
dex -c log_dir=./.dex-log
tail -F ./.dex-log/dex-tui.log
```

The non-interactive mode (`dex exec`) defaults to `RUST_LOG=error`, but messages are printed inline, so there is no need to monitor a separate file.

See the Rust documentation on [`RUST_LOG`](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) for more information on the configuration options.
