## Installing & building

### GitHub Releases

Standalone installers and complete platform packages are published to
[ZigHuang/dex Releases](https://github.com/ZigHuang/dex/releases). The installers
download packages from GitHub by default and verify their SHA-256 checksums.

Install or update to the latest stable release on Linux or macOS:

```bash
curl --http1.1 -fsSL --retry 5 --retry-all-errors --connect-timeout 10 \
  https://github.com/ZigHuang/dex/releases/latest/download/install.sh \
  -o /tmp/dex-install.sh && sh /tmp/dex-install.sh
```

The command selects your operating system and architecture automatically. No
version is required: the installer resolves GitHub's latest stable release on
each run. Use `--release` or `DEX_RELEASE` only when selecting a specific version.

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
Source changes to a published release require a new version. Missing platform
packages may be added from the same original tag; update the checksum manifest
when adding them and preserve the existing package archives.

### GitHub API installation

If GitHub's script download fails with a connection reset or curl error 56,
retrieve the installer through the repository API:

```bash
curl --http1.1 -fsSL --retry 5 --retry-all-errors --connect-timeout 10 \
  -H 'Accept: application/vnd.github.raw+json' \
  'https://api.github.com/repos/ZigHuang/dex/contents/scripts/install/install.sh?ref=main' \
  -o /tmp/dex-install.sh && sh /tmp/dex-install.sh
```

This downloads the installer from `main`; the installer still selects the latest
stable GitHub Release. Both commands execute the script only after curl completes
successfully.

Package downloads show progress in a terminal and retry interrupted connections.
A curl timeout printed during a retry does not mean the installer has exited;
the retry message shows the next attempt and the number of bytes already received.
Slow transfers may run for more than five minutes. Connections that stop sending
data are retried from the received offset, without curl discarding the partial file.
Partial archives are saved under the installation's `downloads` directory and
reused when the installer is run again for the same asset and SHA-256 digest.
When the GitHub download URL fails, the installer resumes through the GitHub
Release asset API and verifies the same SHA-256 digest before installation.

### System requirements

| Requirement                 | Details                                                         |
| --------------------------- | --------------------------------------------------------------- |
| Operating systems           | macOS 12+, Ubuntu 20.04+/Debian 10+, or Windows 11 **via WSL2** |
| Git (optional, recommended) | 2.23+ for built-in PR helpers                                   |
| RAM                         | 4-GB minimum (8-GB recommended)                                 |

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
