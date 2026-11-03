#!/usr/bin/env python3

from concurrent.futures import ThreadPoolExecutor
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import select
import shlex
import shutil
import subprocess
import tarfile
import tempfile
import textwrap
from threading import Thread
import time
import unittest


INSTALL_SCRIPT = Path(__file__).with_name("install.sh")
VERSION = "0.142.5"
MISMATCH_VERSION = "0.145.0"


class InstallShTest(unittest.TestCase):
    def test_codex_config_reuse_preserves_files_and_separates_daemon_home(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            codex_home = root / "home/.codex"
            codex_home.mkdir(parents=True)
            config = 'model_provider = "custom"\n'
            auth = '{"OPENAI_API_KEY":"test-credential"}\n'
            (codex_home / "config.toml").write_text(config)
            (codex_home / "auth.json").write_text(auth)
            codex_binary = codex_home / "packages/app-server-daemon/current/bin/codex"
            codex_binary.parent.mkdir(parents=True)
            codex_binary.write_text("existing Codex daemon")
            dex_home = root / "dex-home"
            dex_home.mkdir()
            (dex_home / "config.toml").write_text("original Dex config")
            (dex_home / "auth.json").write_text("original Dex auth")
            (dex_home / "auth.json").chmod(0o600)
            archive, checksum, metadata = create_package_release(root, record_daemon_home=True)
            result, _ = run_installer_in(
                root, "latest", metadata_json=metadata, archive_path=archive,
                checksum_path=checksum, force_macos=True, use_codex_config=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            for name in ("config.toml", "auth.json"):
                self.assertEqual((dex_home / name).resolve(), codex_home / name)
            backup = next(dex_home.glob("codex-config-backup.*"))
            self.assertEqual((backup / "config.toml").read_text(), "original Dex config")
            self.assertEqual((backup / "auth.json").read_text(), "original Dex auth")
            self.assertEqual(backup.stat().st_mode & 0o777, 0o700)
            self.assertEqual((backup / "auth.json").stat().st_mode & 0o777, 0o600)
            self.assertEqual((codex_home / "config.toml").read_text(), config)
            self.assertEqual((codex_home / "auth.json").read_text(), auth)
            self.assertEqual(codex_binary.read_text(), "existing Codex daemon")
            self.assertEqual((root / "daemon-home.log").read_text().strip(), str(dex_home))

    def test_codex_config_reuse_is_idempotent(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = root / "home/.codex"
            source.mkdir(parents=True)
            (source / "config.toml").write_text('model_provider = "custom"\n')
            dex_home = root / "dex-home"
            dex_home.mkdir()
            (dex_home / "config.toml").write_text("original config")
            archive, checksum, metadata = create_package_release(root, record_daemon_home=True)
            options = dict(
                metadata_json=metadata, archive_path=archive, checksum_path=checksum,
                force_macos=True, use_codex_config=True,
            )
            first, initial_requests = run_installer_in(root, "latest", **options)
            self.assertEqual(first.returncode, 0, first.stderr)
            link_stat = (dex_home / "config.toml").lstat()
            backups = list(dex_home.glob("codex-config-backup.*"))
            (source / "config.toml").write_text('model_provider = "updated"\n')
            second, requests = run_installer_in(root, "latest", **options)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertEqual((dex_home / "config.toml").lstat().st_ino, link_stat.st_ino)
            self.assertEqual(list(dex_home.glob("codex-config-backup.*")), backups)
            self.assertEqual((dex_home / "config.toml").read_text(), 'model_provider = "updated"\n')
            self.assertEqual(
                requests[len(initial_requests):],
                ["https://api.github.com/repos/ZigHuang/dex/releases/latest"],
            )

    def test_codex_config_reuse_requires_existing_codex_configuration(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            result, requests = run_installer_in(root, "latest", use_codex_config=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Codex config not found", result.stderr)
            self.assertEqual(requests, [])
            self.assertFalse((root / "install-bin/dex").exists())

    def test_codex_config_reuse_rejects_a_shared_data_directory(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = root / "home/.codex"
            source.mkdir(parents=True)
            (source / "config.toml").write_text("original Codex config")
            alias = root / "dex-home"
            alias.symlink_to(source, target_is_directory=True)
            result, requests = run_installer_in(root, "latest", use_codex_config=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("separate data directories", result.stderr)
            self.assertEqual(requests, [])
            self.assertEqual((source / "config.toml").read_text(), "original Codex config")
            self.assertFalse((source / "packages").exists())

    def test_codex_config_reuse_rejects_daemon_only_installation(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            result, requests = run_installer_in(
                Path(temp_dir), "latest", use_codex_config=True, daemon_only=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("requires a standalone CLI", result.stderr)
            self.assertEqual(requests, [])

    def test_metadata_fetch_failure_is_not_reported_as_missing_assets(self) -> None:
        result, requests = run_installer(VERSION, metadata_failure=True)

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(
            requests,
            [
                f"https://api.github.com/repos/ZigHuang/dex/releases/tags/rust-v{VERSION}"
            ],
        )
        self.assertIn(
            f"Could not fetch GitHub release metadata for Dex {VERSION}",
            result.stderr,
        )
        self.assertNotIn("Could not find Dex package", result.stderr)

    def test_exact_release_opt_out_uses_github_metadata_once(self) -> None:
        result, requests = run_installer(VERSION, use_mirror=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(
            requests,
            [
                "https://api.github.com/repos/ZigHuang/dex/releases/tags/"
                f"rust-v{VERSION}",
                "https://github.com/ZigHuang/dex/releases/download/"
                f"rust-v{VERSION}/dex-package_SHA256SUMS",
            ],
        )
        self.assertIn(f"Resolved version: {VERSION}", result.stdout)

    def test_alpha_hotfix_release_is_valid(self) -> None:
        version = "0.145.0-alpha.23.1"
        result, requests = run_installer(version)

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(
            requests,
            [
                "https://api.github.com/repos/ZigHuang/dex/releases/tags/"
                f"rust-v{version}",
                "https://github.com/ZigHuang/dex/releases/download/"
                f"rust-v{version}/dex-package_SHA256SUMS",
            ],
        )
        self.assertIn(f"Resolved version: {version}", result.stdout)

    def test_latest_release_reuses_version_metadata(self) -> None:
        result, requests = run_installer("latest")

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(
            requests,
            [
                "https://api.github.com/repos/ZigHuang/dex/releases/latest",
                "https://github.com/ZigHuang/dex/releases/download/"
                f"rust-v{VERSION}/dex-package_SHA256SUMS",
            ],
        )
        self.assertIn(f"Resolved version: {VERSION}", result.stdout)

    def test_compact_metadata_is_independent_of_field_order(self) -> None:
        result, requests = run_installer(
            "latest", metadata_json=release_metadata(compact=True, reorder=True)
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(
            requests,
            [
                "https://api.github.com/repos/ZigHuang/dex/releases/latest",
                "https://github.com/ZigHuang/dex/releases/download/"
                f"rust-v{VERSION}/dex-package_SHA256SUMS",
            ],
        )
        self.assertIn(f"Resolved version: {VERSION}", result.stdout)

    def test_json_like_strings_and_nested_fields_do_not_define_assets(self) -> None:
        result, requests = run_installer(
            VERSION, metadata_json=legacy_release_metadata_with_decoys()
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(requests), 2)
        self.assertIn("/dex-npm-", requests[1])
        self.assertNotIn("dex-package_SHA256SUMS", requests[1])

    def test_macos_install_exposes_code_mode_host_beside_dex(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, checksum_path, metadata_json = create_package_release(root)

            result, _requests = run_installer_in(
                root,
                VERSION,
                metadata_json=metadata_json,
                archive_path=archive_path,
                checksum_path=checksum_path,
                force_macos=True,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            install_bin = root / "install-bin"
            current = root / "dex-home" / "packages" / "standalone" / "current"
            dex_path = install_bin / "dex"
            host_path = install_bin / "dex-code-mode-host"
            self.assertEqual(os.readlink(dex_path), str(current / "bin" / "dex"))
            self.assertEqual(
                os.readlink(host_path),
                str(current / "bin" / "dex-code-mode-host"),
            )
            self.assertTrue(os.access(host_path, os.X_OK))

    def test_github_latest_installs_verified_package_by_default(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, checksum_path, metadata_json = create_package_release(
                root, include_exec_server=True
            )

            result, requests = run_installer_in(
                root,
                "latest",
                metadata_json=metadata_json,
                archive_path=archive_path,
                checksum_path=checksum_path,
                force_macos=True,
                use_mirror=None,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                requests,
                [
                    "https://api.github.com/repos/ZigHuang/dex/releases/latest",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package_SHA256SUMS",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package-aarch64-apple-darwin.tar.gz",
                ],
            )

            current = root / "dex-home/packages/standalone/current"
            for binary in ("dex", "dex-code-mode-host", "dex-exec-server"):
                self.assertTrue(os.access(current / "bin" / binary, os.X_OK))
            archive_request = next(
                line for line in (root / "curl-arguments.log").read_text().splitlines()
                if "/" + archive_path.name in line
            )
            downloaded_path = Path(shlex.split(archive_request)[-1])
            self.assertTrue(downloaded_path.name.startswith(archive_path.name + "."))
            self.assertNotEqual(downloaded_path.name, checksum_path.name)

    def test_ssl_timeout_retries_with_a_longer_connection_budget(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive, checksum, metadata = create_package_release(root)
            result, requests = run_installer_in(
                root, "latest", metadata_json=metadata, archive_path=archive,
                checksum_path=checksum, force_macos=True,
                network_scenario="archive_ssl_timeout_once",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(sum(url.endswith(archive.name) for url in requests), 2)
            self.assertIn("exit 28", result.stderr)
            self.assertIn("attempt 2/5", result.stderr)
            arguments = (root / "curl-arguments.log").read_text()
            self.assertIn("--connect-timeout 30", arguments)
            self.assertIn("--ipv4", arguments)
            self.assertNotIn("--retry ", arguments)
            self.assertIn("--max-time 0 --speed-limit 1024 --speed-time 30", arguments)

    def test_real_curl_timeout_resumes_from_the_received_offset(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive, checksum, metadata = create_package_release(root)
            requests = []

            class Handler(BaseHTTPRequestHandler):
                def log_message(self, *_args):
                    pass

                def do_GET(self):
                    range_header = self.headers.get("Range")
                    requests.append((self.path, range_header))
                    if self.path.endswith(archive.name):
                        body = archive.read_bytes()
                    elif self.path.endswith(checksum.name):
                        body = checksum.read_bytes()
                    else:
                        body = metadata.encode()
                    start = int(range_header.removeprefix("bytes=").split("-")[0]) if range_header else 0
                    self.send_response(206 if range_header else 200)
                    self.send_header("Content-Length", str(len(body) - start))
                    if range_header:
                        self.send_header("Content-Range", f"bytes {start}-{len(body)-1}/{len(body)}")
                    self.end_headers()
                    try:
                        if self.path.endswith(archive.name) and not range_header:
                            self.wfile.write(body[:128])
                            self.wfile.flush()
                            time.sleep(0.3)
                            self.wfile.write(body[128:])
                        else:
                            self.wfile.write(body[start:])
                    except (BrokenPipeError, ConnectionResetError):
                        pass

            server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
            worker = Thread(target=server.serve_forever, daemon=True)
            worker.start()
            real_curl = shutil.which("curl")
            self.assertIsNotNone(real_curl)
            wrapper = textwrap.dedent(f"""\
                #!/usr/bin/python3
                import os, pathlib, subprocess, sys, urllib.parse
                args = sys.argv[1:]
                archive = {archive.name!r}
                first_archive = False
                for index, arg in enumerate(args):
                    if arg.startswith('https://'):
                        path = urllib.parse.urlsplit(arg).path
                        args[index] = 'http://127.0.0.1:{server.server_port}' + path
                        marker = pathlib.Path(os.environ['DEX_TEST_RETRY_MARKER'])
                        if path.endswith(archive) and not marker.exists():
                            marker.touch()
                            first_archive = True
                if first_archive:
                    args += ['--max-time', '0.1']
                args += ['--noproxy', '127.0.0.1']
                sys.exit(subprocess.call([{real_curl!r}] + args))
                """)
            try:
                result, _ = run_installer_in(
                    root, "latest", metadata_json=metadata, archive_path=archive,
                    checksum_path=checksum, force_macos=True, curl_wrapper=wrapper,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                archive_requests = [header for path, header in requests if path.endswith(archive.name)]
                self.assertEqual(archive_requests, [None, "bytes=128-"])
                self.assertIn("resuming from 128 bytes", result.stderr)
                self.assertTrue((root / "install-bin/dex").is_file())
            finally:
                server.shutdown()
                server.server_close()
                worker.join()

    def test_api_fallback_preserves_a_partial_archive_after_timeouts(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive, checksum, metadata = create_package_release(root)
            release = json.loads(metadata)
            for item, asset_id in zip(release["assets"], (101, 102)):
                item["url"] = f"https://api.github.com/repos/ZigHuang/dex/releases/assets/{asset_id}"
            result, requests = run_installer_in(
                root, "latest", metadata_json=json.dumps(release), archive_path=archive,
                checksum_path=checksum, force_macos=True, network_scenario="archive_timeouts",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(sum(url.endswith(archive.name) for url in requests), 5)
            self.assertIn("https://api.github.com/repos/ZigHuang/dex/releases/assets/101?download=1", requests)

    def test_rerunning_installer_resumes_a_cached_partial_archive(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive, checksum, metadata = create_package_release(root)
            options = dict(
                metadata_json=metadata, archive_path=archive,
                checksum_path=checksum, force_macos=True,
            )
            interrupted, _ = run_installer_in(
                root, "latest", **options, network_scenario="archive_timeouts",
            )
            self.assertNotEqual(interrupted.returncode, 0)
            cache = root / "dex-home/packages/standalone/downloads"
            partial = next(cache.glob("*.part"))
            self.assertEqual(partial.stat().st_size, 128)
            self.assertFalse((root / "install-bin/dex").exists())
            resumed, _ = run_installer_in(
                root, "latest", **options, network_scenario="cached_archive",
            )
            self.assertEqual(resumed.returncode, 0, resumed.stderr)
            self.assertIn("Resuming cached download from 128 bytes", resumed.stdout)
            self.assertFalse(partial.exists())

    def test_interrupted_archive_is_resumed_and_verified(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive, checksum, metadata = create_package_release(root, include_exec_server=True)
            result, requests = run_installer_in(
                root, "latest", metadata_json=metadata, archive_path=archive,
                checksum_path=checksum, force_macos=True, network_scenario="archive_reset_once",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            package_url = f"https://github.com/ZigHuang/dex/releases/download/rust-v{VERSION}/{archive.name}"
            self.assertEqual(requests.count(package_url), 2)
            self.assertIn("--continue-at -", (root / "curl-arguments.log").read_text())
            self.assertTrue((root / "dex-home/packages/standalone/current/bin/dex-exec-server").is_file())

    def test_github_api_fallback_installs_after_browser_download_resets(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive, checksum, metadata = create_package_release(root, include_exec_server=True)
            release = json.loads(metadata)
            for item, asset_id in zip(release["assets"], (101, 102)):
                item["url"] = f"https://api.github.com/repos/ZigHuang/dex/releases/assets/{asset_id}"
            result, requests = run_installer_in(
                root, "latest", metadata_json=json.dumps(release), archive_path=archive,
                checksum_path=checksum, force_macos=True, network_scenario="github_reset",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("https://api.github.com/repos/ZigHuang/dex/releases/assets/101?download=1", requests)
            self.assertIn("https://api.github.com/repos/ZigHuang/dex/releases/assets/102?download=1", requests)
            self.assertIn("Accept: application/octet-stream", (root / "curl-arguments.log").read_text())
            self.assertTrue((root / "install-bin/dex").is_file())

    def test_partial_metadata_from_reset_is_discarded(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive, checksum, metadata = create_package_release(root)
            result, requests = run_installer_in(
                root, "latest", metadata_json=metadata, archive_path=archive,
                checksum_path=checksum, force_macos=True, network_scenario="metadata_reset_once",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(requests.count("https://api.github.com/repos/ZigHuang/dex/releases/latest"), 2)

    def test_corrupt_api_fallback_cannot_select_an_installation(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive, checksum, metadata = create_package_release(root)
            release = json.loads(metadata)
            for item, asset_id in zip(release["assets"], (101, 102)):
                item["url"] = f"https://api.github.com/repos/ZigHuang/dex/releases/assets/{asset_id}"
            result, _ = run_installer_in(
                root, "latest", metadata_json=json.dumps(release), archive_path=archive,
                checksum_path=checksum, force_macos=True, network_scenario="github_reset_api_corrupt",
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("checksum did not match expected digest", result.stderr)
            self.assertFalse((root / "dex-home/packages/standalone/current").is_symlink())
            self.assertFalse((root / "install-bin/dex").exists())

    def test_daemon_install_preserves_visible_cli_and_profile(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir).resolve()
            archive, checksum, metadata = create_package_release(root)
            options = dict(
                metadata_json=metadata,
                archive_path=archive,
                checksum_path=checksum,
                force_macos=True,
            )
            pinned, _ = run_installer_in(root, VERSION, **options)
            self.assertEqual(pinned.returncode, 0, pinned.stderr)
            visible = root / "install-bin/dex"
            original = visible.resolve()
            pending = visible.with_name(".dex.12345")
            pending.symlink_to(original)
            profile = root / "home/.profile"
            profile.write_text("unchanged profile\n")

            installed, _ = run_installer_in(root, "latest", daemon_only=True, **options)
            self.assertEqual(installed.returncode, 0, installed.stderr)
            daemon = root / "dex-home/packages/app-server-daemon"
            release_name = f"{VERSION}-aarch64-apple-darwin"
            self.assertEqual(
                (daemon / "current").resolve(), daemon / "releases" / release_name
            )
            self.assertEqual((daemon / "auto-update-version").read_text(), release_name)
            history = daemon / "selection-history"
            original_history = history.read_bytes()
            self.assertEqual(history.read_text().splitlines()[:2], [release_name, ""])
            repeated, _ = run_installer_in(root, "latest", daemon_only=True, **options)
            self.assertEqual(repeated.returncode, 0, repeated.stderr)
            self.assertEqual(history.read_bytes(), original_history)
            options["daemon_only"] = True
            local = daemon / "releases/local-development"
            local.mkdir()
            (daemon / "current").unlink()
            (daemon / "current").symlink_to(local)
            marker = daemon / "auto-update-version"
            marker.unlink()

            scheduled, _ = run_installer_in(
                root, "latest", update_guard_from_release=local.name, **options
            )
            self.assertEqual(scheduled.returncode, 0, scheduled.stderr)
            self.assertEqual((daemon / "current").resolve(), local)
            self.assertFalse(marker.exists())

            raced, _ = run_installer_in(
                root,
                "latest",
                update_guard_from_release="a-different-release",
                manual_update=True,
                **options,
            )
            self.assertNotEqual(raced.returncode, 0, raced.stderr)
            self.assertEqual((daemon / "current").resolve(), local)
            self.assertFalse(marker.exists())

            binary = daemon / "releases" / release_name / "bin/dex"
            contents = binary.read_text()
            binary.write_text(
                f'#!/bin/sh\n[ "$1" = "--version" ] || exit 2\necho dex-cli {VERSION}\n'
            )
            incompatible, _ = run_installer_in(
                root,
                "latest",
                update_guard_from_release=local.name,
                manual_update=True,
                **options,
            )
            self.assertNotEqual(incompatible.returncode, 0)
            self.assertIn("does not support daemon-owned packages", incompatible.stderr)
            self.assertEqual((daemon / "current").resolve(), local)
            self.assertFalse(marker.exists())
            binary.write_text(contents)

            restored, _ = run_installer_in(
                root,
                "latest",
                update_guard_from_release=local.name,
                manual_update=True,
                **options,
            )
            self.assertEqual(restored.returncode, 0, restored.stderr)
            self.assertEqual(
                (daemon / "current").resolve(), daemon / "releases" / release_name
            )
            self.assertEqual(marker.read_text(), release_name)
            self.assertEqual(visible.resolve(), original)
            self.assertTrue(pending.is_symlink())
            self.assertEqual(profile.read_text(), "unchanged profile\n")
            self.assertFalse(
                (root / "dex-home/packages/standalone/auto-update-version").exists()
            )

    def test_explicit_release_pins_even_the_current_latest_version(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive, checksum, metadata = create_package_release(root)
            options = dict(
                metadata_json=metadata,
                archive_path=archive,
                checksum_path=checksum,
                force_macos=True,
            )
            marker = root / "dex-home/packages/standalone/auto-update-version"
            latest, _ = run_installer_in(root, "latest", **options)
            self.assertEqual(latest.returncode, 0, latest.stderr)
            release_name = f"{VERSION}-aarch64-apple-darwin"
            self.assertEqual(marker.read_text(), release_name)

            pinned, _ = run_installer_in(root, VERSION, **options)
            self.assertEqual(pinned.returncode, 0, pinned.stderr)
            self.assertFalse(marker.exists())

            updater_record = root / "dex-home/app-server-daemon/app-server-updater.pid"
            updater_record.parent.mkdir(parents=True)
            updater_record.write_text(
                json.dumps(
                    {"pid": os.getpid(), "processStartTime": process_start_time()}
                )
            )
            old_updater, _ = run_installer_in(
                root, "latest", old_updater_parent_pid=os.getpid(), **options
            )
            self.assertEqual(old_updater.returncode, 0, old_updater.stderr)
            self.assertFalse(marker.exists())

            skipped, _ = run_installer_in(
                root, "latest", update_guard_from_release=release_name, **options
            )
            self.assertEqual(skipped.returncode, 0, skipped.stderr)
            self.assertFalse(marker.exists())

            updater_record.write_text(
                json.dumps({"pid": os.getpid(), "processStartTime": "stale"})
            )
            latest_again, _ = run_installer_in(
                root, "latest", old_updater_parent_pid=os.getpid(), **options
            )
            self.assertEqual(latest_again.returncode, 0, latest_again.stderr)
            self.assertEqual(marker.read_text(), release_name)

            managed = (
                root / f"dex-home/packages/standalone/releases/{release_name}/bin/dex"
            )
            managed.unlink()
            guarded, _ = run_installer_in(
                root,
                "latest",
                update_guard_from_release=release_name,
                **options,
            )
            self.assertEqual(guarded.returncode, 0, guarded.stderr)
            self.assertTrue(managed.exists())

    def test_macos_release_inspection_preserves_lsof_matches(self) -> None:
        # Exercise the macOS branch on every platform, including lsof's absolute path.
        function = (
            INSTALL_SCRIPT.read_text()
            .split("record_standalone_releases_in_use() {", 1)[1]
            .split("\n}\n", 1)[0]
        )
        function = "record_standalone_releases_in_use() {" + function + "\n}\n"
        function = function.replace("/usr/sbin/lsof ", "mock_lsof ")
        matches = "p123\nn/releases/active/bin/dex\n"
        for status, output, error, expected in (
            (0, matches, "", 0),
            (1, "", "", 0),
            (1, matches, "", 0),
            (0, matches, "inspection failed\n", 1),
            (1, matches, "inspection failed\n", 1),
            (2, "", "", 1),
            (2, matches, "", 1),
        ):
            with self.subTest(status=status, output=output, error=error):
                with tempfile.TemporaryDirectory() as temp_dir:
                    script = (
                        "set -eu\n"
                        "os=macos\n"
                        "releases=/releases\n"
                        f"tmp_dir={shlex.quote(temp_dir)}\n"
                        # Avoid depending on other processes on the test host.
                        'ps() { printf "%s dex\\n" "$(id -u)"; }\n'
                        "mock_lsof() {\n"
                        f"  printf '%s' {shlex.quote(output)}\n"
                        f"  printf '%s' {shlex.quote(error)} >&2\n"
                        f"  return {status}\n"
                        "}\n" + function + "record_standalone_releases_in_use\n"
                    )
                    result = subprocess.run(
                        ["sh"], input=script, text=True, capture_output=True
                    )
                    self.assertEqual(
                        (result.returncode, result.stdout, result.stderr),
                        (expected, "", ""),
                    )
                    self.assertEqual((Path(temp_dir) / "in-use").read_text(), output)

    def test_standalone_retains_previous_and_running_releases_across_installs(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            # macOS lsof can escape the non-ASCII bytes in the installation path.
            root = Path(temp_dir).resolve() / "André dex install"
            root.mkdir()
            system = os.uname()
            if system.sysname == "Linux":
                # The race requires pruning to proceed. A live but unreadable
                # Dex/helper (e.g. the agent's sandbox) must prevent that.
                for process in Path("/proc").glob("[0-9]*"):
                    try:
                        (process / "exe").readlink()
                    except OSError:
                        try:
                            name = (process / "comm").read_text().strip()
                            status = (process / "status").read_text()
                        except OSError:
                            continue  # The process may have exited.
                        if (
                            name.startswith("dex") or name in ("rg", "bwrap")
                        ) and not any(
                            line.startswith("State:") and "Z" in line
                            for line in status.splitlines()
                        ):
                            self.skipTest(
                                f"Cannot inspect live {name} process {process.name}"
                            )
            arch = "aarch64" if system.machine in ("arm64", "aarch64") else "x86_64"
            target = f"{arch}-{'apple-darwin' if system.sysname == 'Darwin' else 'unknown-linux-musl'}"
            packages = root / "dex-home/packages/standalone"
            releases = packages / "releases"
            versions = ("0.140.0", "0.141.0", VERSION)
            install_options = {}
            for version in versions:
                source = root / version
                source.mkdir()
                archive, checksum, metadata = create_package_release(
                    source,
                    metadata_version=version,
                    binary_version=version,
                    target=target,
                )
                install_options[version] = dict(
                    metadata_json=metadata, archive_path=archive, checksum_path=checksum
                )

            first, _ = run_installer_in(
                root, versions[0], **install_options[versions[0]]
            )
            self.assertEqual(first.returncode, 0, first.stderr)
            oldest = releases / f"{versions[0]}-{target}"
            helper = oldest / "bin/dex-code-mode-host"
            shutil.copy("/bin/sleep", helper)
            if system.sysname == "Darwin":
                # A copied macOS system binary cannot run with its platform signature.
                subprocess.run(
                    ["/usr/bin/codesign", "--force", "--sign", "-", str(helper)],
                    check=True,
                    capture_output=True,
                )
            # A helper can outlive its parent CLI; retain its whole release.
            with subprocess.Popen([str(helper), "300"]) as running:
                try:
                    second, _ = run_installer_in(
                        root, versions[1], **install_options[versions[1]]
                    )
                    self.assertEqual(second.returncode, 0, second.stderr)

                    unowned = releases / f"0.139.0-{target}"
                    unowned.mkdir()
                    (unowned / "dex-package.json").write_text("{}")
                    retired = releases / ".retired.old-attempt"
                    retired.mkdir()
                    (retired / "leave-alone").write_text("not retried")
                    external = root / "external"
                    external.mkdir()
                    (external / "user-file").write_text("untouched")
                    (releases / f"0.138.0-{target}").symlink_to(external)

                    # Failed installs must not advance history or prune anything.
                    history = packages / "selection-history"
                    before_failure = history.read_bytes()
                    bad_source = root / "wrong-version"
                    bad_source.mkdir()
                    archive, checksum, metadata = create_package_release(
                        bad_source, metadata_version="0.199.0", target=target
                    )
                    failed, _ = run_installer_in(
                        root,
                        "0.199.0",
                        metadata_json=metadata,
                        archive_path=archive,
                        checksum_path=checksum,
                    )
                    self.assertNotEqual(failed.returncode, 0, failed.stderr)
                    self.assertEqual(history.read_bytes(), before_failure)
                    self.assertTrue(oldest.is_dir())

                    latest, _ = run_installer_in(
                        root, "latest", **install_options[VERSION]
                    )
                    self.assertEqual(latest.returncode, 0, latest.stderr)
                    self.assertIsNone(running.poll())
                    self.assertTrue(oldest.is_dir(), latest.stderr)
                    previous = releases / f"{versions[1]}-{target}"
                    current = releases / f"{VERSION}-{target}"
                    self.assertEqual((packages / "current").resolve(), current)
                    self.assertTrue(previous.is_dir())
                    self.assertEqual(
                        history.read_text().splitlines()[:2],
                        [current.name, previous.name],
                    )
                    recorded = history.read_bytes()
                finally:
                    running.terminate()
                    running.wait()

            if system.sysname == "Darwin":
                # lsof sees no running helper now. ps can still see names for
                # a different user, whose open files lsof silently omits.
                fake_ps = root / "bin/ps"
                foreign_uid = 0 if os.geteuid() != 0 else 501
                try:
                    for executable in (oldest / "bin/dex", helper):
                        with self.subTest(foreign_executable=executable.name):
                            fake_ps.write_text(
                                "#!/bin/sh\n"
                                'case "$*" in\n'
                                "  '-axo uid=,comm=')\n"
                                f"    printf '%s\\n' {shlex.quote(f'{foreign_uid} {executable}')}\n"
                                "    ;;\n"
                                '  *) exec /bin/ps "$@" ;;\n'
                                "esac\n"
                            )
                            fake_ps.chmod(0o755)
                            retained, _ = run_installer_in(
                                root, "latest", **install_options[VERSION]
                            )
                            self.assertEqual(retained.returncode, 0, retained.stderr)
                            self.assertTrue(oldest.is_dir(), retained.stderr)
                            self.assertEqual(history.read_bytes(), recorded)
                finally:
                    fake_ps.unlink()

            # Start the helper after the first scan but immediately before the
            # retirement rename. The second scan must restore its release.
            fake_mv = root / "bin/mv"
            ready = root / "retirement-ready"
            resume = root / "retirement-resume"
            os.mkfifo(ready)
            os.mkfifo(resume)
            real_mv = shutil.which("mv")
            fake_mv.write_text(
                "#!/bin/sh\n"
                'case "$2" in\n'
                "  */.retired.*/release)\n"
                f'    printf "ready\\n" > {shlex.quote(str(ready))}\n'
                f"    read -r _ < {shlex.quote(str(resume))}\n"
                "    ;;\n"
                "esac\n"
                f'exec {shlex.quote(real_mv)} "$@"\n'
            )
            fake_mv.chmod(0o755)
            ready_fd = os.open(ready, os.O_RDWR | os.O_NONBLOCK)
            resume_fd = os.open(resume, os.O_RDWR | os.O_NONBLOCK)
            try:
                with ThreadPoolExecutor(max_workers=1) as executor:
                    installing = executor.submit(
                        run_installer_in, root, "latest", **install_options[VERSION]
                    )
                    try:
                        readable, _, _ = select.select([ready_fd], [], [], 30)
                        self.assertTrue(
                            readable, "installer did not attempt retirement"
                        )
                        self.assertEqual(os.read(ready_fd, 128), b"ready\n")
                        with subprocess.Popen([str(helper), "300"]) as late_helper:
                            try:
                                os.write(resume_fd, b"continue\n")
                                raced, _ = installing.result(timeout=30)
                                self.assertEqual(raced.returncode, 0, raced.stderr)
                                self.assertTrue(oldest.is_dir(), raced.stderr)
                                self.assertEqual(history.read_bytes(), recorded)
                            finally:
                                late_helper.terminate()
                                late_helper.wait()
                    finally:
                        # Release the intercepted mv even if an assertion fails.
                        os.write(resume_fd, b"continue\n")
            finally:
                os.close(ready_fd)
                os.close(resume_fd)
                fake_mv.unlink()

            if system.sysname == "Darwin":
                # An external process can hold just the release directory open.
                with subprocess.Popen(["/bin/cat"], stdin=subprocess.PIPE, cwd=oldest):
                    held, _ = run_installer_in(
                        root, "latest", **install_options[VERSION]
                    )
                    self.assertEqual(held.returncode, 0, held.stderr)
                    self.assertTrue(oldest.is_dir(), held.stderr)

            # A no-op latest install (also the dex update path) cleans up the
            # now-unused release without rotating previous to current.
            repeated, _ = run_installer_in(root, "latest", **install_options[VERSION])
            self.assertEqual(repeated.returncode, 0, repeated.stderr)
            self.assertFalse(oldest.exists(), repeated.stderr)
            self.assertTrue(previous.is_dir())
            self.assertEqual(history.read_bytes(), recorded)
            self.assertTrue(unowned.is_dir())
            self.assertEqual((retired / "leave-alone").read_text(), "not retried")
            self.assertEqual((external / "user-file").read_text(), "untouched")

            # Downgrades keep the actual previously selected version.
            pinned, _ = run_installer_in(
                root, versions[1], **install_options[versions[1]]
            )
            self.assertEqual(pinned.returncode, 0, pinned.stderr)
            self.assertEqual((packages / "current").resolve(), previous)
            self.assertEqual(
                history.read_text().splitlines()[:2], [previous.name, current.name]
            )
            self.assertTrue(current.is_dir())

    def test_uninspectable_legacy_updater_does_not_clear_pin(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive, checksum, metadata = create_package_release(root)
            options = dict(
                metadata_json=metadata,
                archive_path=archive,
                checksum_path=checksum,
                force_macos=True,
            )
            pinned, _ = run_installer_in(root, VERSION, **options)
            self.assertEqual(pinned.returncode, 0, pinned.stderr)
            updater_record = root / "dex-home/app-server-daemon/app-server-updater.pid"
            updater_record.parent.mkdir(parents=True)
            updater_record.write_text(
                json.dumps(
                    {"pid": os.getpid(), "processStartTime": process_start_time()}
                )
            )

            attempted, _ = run_installer_in(
                root,
                "latest",
                old_updater_parent_pid=os.getpid(),
                fail_ps=True,
                **options,
            )
            self.assertEqual(attempted.returncode, 0, attempted.stderr)
            self.assertFalse(
                (root / "dex-home/packages/standalone/auto-update-version").exists()
            )

    def test_releases_unusable_metadata_falls_back_to_github(self) -> None:
        unusable_metadata = {
            "html": "<html>proxy error</html>",
            "empty": "",
            "malformed_json": '{"tag_name":',
            "missing_tag": json.dumps({"assets": []}),
            "missing_assets": json.dumps(
                {"tag_name": f"rust-v{VERSION}", "assets": []}
            ),
            "invalid_checksum_digest": json.dumps(
                {
                    "tag_name": f"rust-v{VERSION}",
                    "assets": [
                        {
                            "name": "dex-package-aarch64-apple-darwin.tar.gz",
                            "digest": "sha256:" + "a" * 64,
                        },
                        {
                            "name": "dex-package_SHA256SUMS",
                            "digest": "sha256:" + "z" * 64,
                        },
                    ],
                }
            ),
            "invalid_version": json.dumps({"tag_name": "rust-vinvalid"}),
        }

        for name, releases_metadata_json in unusable_metadata.items():
            with self.subTest(metadata=name):
                with tempfile.TemporaryDirectory() as temp_dir:
                    root = Path(temp_dir)
                    archive_path, checksum_path, metadata_json = create_package_release(
                        root
                    )

                    result, requests = run_installer_in(
                        root,
                        "latest",
                        metadata_json=metadata_json,
                        releases_metadata_json=releases_metadata_json,
                        archive_path=archive_path,
                        checksum_path=checksum_path,
                        force_macos=True,
                        use_mirror=True,
                    )

                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(
                        requests,
                        [
                            "https://releases.openai.com/dex/channels/latest",
                            "https://api.github.com/repos/ZigHuang/dex/releases/latest",
                            "https://github.com/ZigHuang/dex/releases/download/"
                            f"rust-v{VERSION}/dex-package_SHA256SUMS",
                            "https://github.com/ZigHuang/dex/releases/download/"
                            f"rust-v{VERSION}/dex-package-aarch64-apple-darwin.tar.gz",
                        ],
                    )
                    self.assertIn("falling back to GitHub Releases", result.stderr)

    def test_releases_exact_metadata_version_mismatch_falls_back_to_github(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, checksum_path, metadata_json = create_package_release(root)
            releases_metadata = json.loads(metadata_json)
            releases_metadata["tag_name"] = f"rust-v{MISMATCH_VERSION}"

            result, requests = run_installer_in(
                root,
                VERSION,
                metadata_json=metadata_json,
                releases_metadata_json=json.dumps(releases_metadata),
                archive_path=archive_path,
                checksum_path=checksum_path,
                force_macos=True,
                use_mirror=True,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                requests,
                [
                    f"https://releases.openai.com/dex/releases/{VERSION}/release.json",
                    "https://api.github.com/repos/ZigHuang/dex/releases/tags/"
                    f"rust-v{VERSION}",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package_SHA256SUMS",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package-aarch64-apple-darwin.tar.gz",
                ],
            )
            self.assertIn("falling back to GitHub Releases", result.stderr)

    def test_releases_asset_download_falls_back_to_github(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, checksum_path, metadata_json = create_package_release(root)

            result, requests = run_installer_in(
                root,
                "latest",
                metadata_json=metadata_json,
                archive_path=archive_path,
                checksum_path=checksum_path,
                force_macos=True,
                use_mirror=True,
                releases_mode="asset_fallback",
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                requests,
                [
                    "https://releases.openai.com/dex/channels/latest",
                    f"https://releases.openai.com/dex/releases/{VERSION}/dex-package_SHA256SUMS",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package_SHA256SUMS",
                    f"https://releases.openai.com/dex/releases/{VERSION}/dex-package-aarch64-apple-darwin.tar.gz",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package-aarch64-apple-darwin.tar.gz",
                ],
            )
            self.assertIn("alternate GitHub download URL", result.stderr)

    def test_releases_corrupt_assets_fall_back_to_github(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, checksum_path, metadata_json = create_package_release(root)

            result, requests = run_installer_in(
                root,
                "latest",
                metadata_json=metadata_json,
                archive_path=archive_path,
                checksum_path=checksum_path,
                force_macos=True,
                use_mirror=True,
                releases_mode="corrupt_assets",
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                requests,
                [
                    "https://releases.openai.com/dex/channels/latest",
                    f"https://releases.openai.com/dex/releases/{VERSION}/dex-package_SHA256SUMS",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package_SHA256SUMS",
                    f"https://releases.openai.com/dex/releases/{VERSION}/dex-package-aarch64-apple-darwin.tar.gz",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package-aarch64-apple-darwin.tar.gz",
                ],
            )
            self.assertIn("checksum did not match expected digest", result.stderr)
            self.assertIn("alternate GitHub download URL", result.stderr)

    def test_releases_wrong_checksum_digest_uses_github_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, checksum_path, metadata_json = create_package_release(root)
            mirror_metadata = json.loads(metadata_json)
            for release_asset in mirror_metadata["assets"]:
                if release_asset["name"] == "dex-package_SHA256SUMS":
                    release_asset["digest"] = "sha256:" + "0" * 64

            result, requests = run_installer_in(
                root,
                "latest",
                metadata_json=metadata_json,
                releases_metadata_json=json.dumps(mirror_metadata),
                archive_path=archive_path,
                checksum_path=checksum_path,
                force_macos=True,
                use_mirror=True,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                requests,
                [
                    "https://releases.openai.com/dex/channels/latest",
                    f"https://releases.openai.com/dex/releases/{VERSION}/dex-package_SHA256SUMS",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package_SHA256SUMS",
                    "https://api.github.com/repos/ZigHuang/dex/releases/tags/"
                    f"rust-v{VERSION}",
                    f"https://releases.openai.com/dex/releases/{VERSION}/dex-package-aarch64-apple-darwin.tar.gz",
                ],
            )
            self.assertIn("checksum did not match expected digest", result.stderr)

    def test_releases_incomplete_checksum_manifest_falls_back_to_github(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, checksum_path, metadata_json = create_package_release(root)
            mirror_checksum_path = root / "mirror-SHA256SUMS"
            mirror_checksum_path.write_text(
                f"{'a' * 64}  dex-package-other-platform.tar.gz\n",
                encoding="utf-8",
            )
            mirror_metadata = json.loads(metadata_json)
            for release_asset in mirror_metadata["assets"]:
                if release_asset["name"] == "dex-package_SHA256SUMS":
                    release_asset["digest"] = (
                        "sha256:"
                        + hashlib.sha256(mirror_checksum_path.read_bytes()).hexdigest()
                    )

            result, requests = run_installer_in(
                root,
                "latest",
                metadata_json=metadata_json,
                releases_metadata_json=json.dumps(mirror_metadata),
                archive_path=archive_path,
                checksum_path=checksum_path,
                releases_checksum_path=mirror_checksum_path,
                force_macos=True,
                use_mirror=True,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                requests,
                [
                    "https://releases.openai.com/dex/channels/latest",
                    f"https://releases.openai.com/dex/releases/{VERSION}/dex-package_SHA256SUMS",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package_SHA256SUMS",
                    "https://api.github.com/repos/ZigHuang/dex/releases/tags/"
                    f"rust-v{VERSION}",
                    f"https://releases.openai.com/dex/releases/{VERSION}/dex-package-aarch64-apple-darwin.tar.gz",
                ],
            )
            self.assertIn("alternate GitHub download URL", result.stderr)

    def test_releases_corrupt_github_fallback_still_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, checksum_path, metadata_json = create_package_release(root)

            result, requests = run_installer_in(
                root,
                "latest",
                metadata_json=metadata_json,
                archive_path=archive_path,
                checksum_path=checksum_path,
                force_macos=True,
                use_mirror=True,
                releases_mode="corrupt_checksum_and_github",
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(
                requests,
                [
                    "https://releases.openai.com/dex/channels/latest",
                    f"https://releases.openai.com/dex/releases/{VERSION}/dex-package_SHA256SUMS",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-package_SHA256SUMS",
                    "https://api.github.com/repos/ZigHuang/dex/releases/tags/"
                    f"rust-v{VERSION}",
                ],
            )
            self.assertIn("checksum did not match expected digest", result.stderr)

    def test_releases_exact_rejects_wrong_binary_version(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, checksum_path, metadata_json = create_package_release(
                root,
                metadata_version=MISMATCH_VERSION,
            )

            result, requests = run_installer_in(
                root,
                MISMATCH_VERSION,
                metadata_json=metadata_json,
                archive_path=archive_path,
                checksum_path=checksum_path,
                force_macos=True,
                use_mirror=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(
                requests,
                [
                    f"https://releases.openai.com/dex/releases/{MISMATCH_VERSION}/release.json",
                    f"https://releases.openai.com/dex/releases/{MISMATCH_VERSION}/dex-package_SHA256SUMS",
                    f"https://releases.openai.com/dex/releases/{MISMATCH_VERSION}/dex-package-aarch64-apple-darwin.tar.gz",
                ],
            )
            self.assertIn(
                f"did not report expected version {MISMATCH_VERSION}",
                result.stderr,
            )
            self.assertNotIn("installed successfully", result.stdout)

    def test_releases_exact_legacy_fallback_reuses_offline_install(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, metadata_json = create_legacy_release(root)

            first_result, first_requests = run_installer_in(
                root,
                VERSION,
                metadata_json=metadata_json,
                legacy_archive_path=archive_path,
                force_macos=True,
                use_mirror=True,
                releases_mode="channel_failure",
            )

            self.assertEqual(first_result.returncode, 0, first_result.stderr)
            self.assertEqual(
                first_requests,
                [
                    f"https://releases.openai.com/dex/releases/{VERSION}/release.json",
                    "https://api.github.com/repos/ZigHuang/dex/releases/tags/"
                    f"rust-v{VERSION}",
                    "https://github.com/ZigHuang/dex/releases/download/"
                    f"rust-v{VERSION}/dex-npm-darwin-arm64-{VERSION}.tgz",
                ],
            )

            (root / "requests.log").unlink()
            second_result, second_requests = run_installer_in(
                root,
                VERSION,
                metadata_json=metadata_json,
                force_macos=True,
                use_mirror=True,
                releases_mode="channel_failure",
            )

            self.assertEqual(second_result.returncode, 0, second_result.stderr)
            self.assertEqual(
                second_requests,
                [
                    f"https://releases.openai.com/dex/releases/{VERSION}/release.json",
                    "https://api.github.com/repos/ZigHuang/dex/releases/tags/"
                    f"rust-v{VERSION}",
                ],
            )
            self.assertNotIn("Downloading Dex CLI", second_result.stdout)


def run_installer(
    release: str,
    *,
    metadata_failure: bool = False,
    metadata_json: str | None = None,
    use_mirror: bool | None = False,
) -> tuple[subprocess.CompletedProcess[str], list[str]]:
    with tempfile.TemporaryDirectory() as temp_dir:
        return run_installer_in(
            Path(temp_dir),
            release,
            metadata_failure=metadata_failure,
            metadata_json=metadata_json,
            use_mirror=use_mirror,
        )


def run_installer_in(
    root: Path,
    release: str,
    *,
    installer_path: Path = INSTALL_SCRIPT,
    metadata_failure: bool = False,
    metadata_json: str | None = None,
    releases_metadata_json: str | None = None,
    archive_path: Path | None = None,
    checksum_path: Path | None = None,
    releases_checksum_path: Path | None = None,
    legacy_archive_path: Path | None = None,
    force_macos: bool = False,
    use_mirror: bool | None = False,
    releases_mode: str = "",
    update_guard_from_release: str | None = None,
    old_updater_parent_pid: int | None = None,
    fail_ps: bool = False,
    daemon_only: bool = False,
    manual_update: bool = False,
    network_scenario: str = "",
    curl_wrapper: str | None = None,
    use_codex_config: bool = False,
) -> tuple[subprocess.CompletedProcess[str], list[str]]:
    bin_dir = root / "bin"
    bin_dir.mkdir(exist_ok=True)
    request_log = root / "requests.log"
    fake_curl = bin_dir / "curl"
    fake_curl.write_text(
        textwrap.dedent(
            """\
            #!/bin/sh
            url=""
            output=""
            previous=""
            for arg in "$@"; do
              case "$arg" in
                https://*) url="$arg" ;;
              esac
              if [ "$previous" = "-o" ]; then
                output="$arg"
              fi
              previous="$arg"
            done
            printf '%s\n' "$url" >>"$DEX_TEST_REQUEST_LOG"
            printf '%s\n' "$*" >>"$DEX_TEST_CURL_ARGUMENTS"

            if [ "$DEX_TEST_NETWORK_SCENARIO" = "github_reset" ] ||
              [ "$DEX_TEST_NETWORK_SCENARIO" = "github_reset_api_corrupt" ]; then
              case "$url" in
                https://github.com/*)
                  echo "curl: (56) Recv failure: Connection reset by peer" >&2
                  exit 56
                  ;;
              esac
            fi

            if [ "$DEX_TEST_NETWORK_SCENARIO" = "archive_reset_once" ] &&
              [ ! -f "$DEX_TEST_RETRY_MARKER" ]; then
              case "$url" in
                https://github.com/*/dex-package-*.tar.gz)
                  head -c 128 "$DEX_TEST_ARCHIVE_PATH" >"$output"
                  touch "$DEX_TEST_RETRY_MARKER"
                  exit 56
                  ;;
              esac
            fi

            if [ "$DEX_TEST_NETWORK_SCENARIO" = "archive_ssl_timeout_once" ] &&
              [ ! -f "$DEX_TEST_RETRY_MARKER" ]; then
              case "$url" in
                https://github.com/*/dex-package-*.tar.gz)
                  touch "$DEX_TEST_RETRY_MARKER"
                  echo "curl: (28) SSL connection timeout" >&2
                  exit 28
                  ;;
              esac
            fi

            if [ "$DEX_TEST_NETWORK_SCENARIO" = "archive_timeouts" ]; then
              case "$url" in
                https://github.com/*/dex-package-*.tar.gz)
                  head -c 128 "$DEX_TEST_ARCHIVE_PATH" >"$output"
                  exit 28
                  ;;
              esac
            fi

            case "$url" in
              https://api.github.com/repos/ZigHuang/dex/releases/assets/101*)
                if [ "$DEX_TEST_NETWORK_SCENARIO" = "archive_timeouts" ]; then
                  case "$*" in
                    *"--continue-at -"*) ;;
                    *) exit 91 ;;
                  esac
                  [ "$(wc -c < "$output")" -eq 128 ] || exit 91
                fi
                if [ "$DEX_TEST_NETWORK_SCENARIO" = "github_reset_api_corrupt" ]; then
                  printf 'corrupt archive\n' >"$output"
                else
                  cp "$DEX_TEST_ARCHIVE_PATH" "$output"
                fi
                ;;
              https://api.github.com/repos/ZigHuang/dex/releases/assets/102*)
                cp "$DEX_TEST_CHECKSUM_PATH" "$output"
                ;;
              https://api.github.com/*)
                if [ "$DEX_TEST_METADATA_FAILURE" = "1" ]; then
                  echo "curl: (22) The requested URL returned error: 403" >&2
                  exit 22
                fi
                if [ "$DEX_TEST_NETWORK_SCENARIO" = "metadata_reset_once" ] &&
                  [ ! -f "$DEX_TEST_RETRY_MARKER" ]; then
                  printf '{"tag_name":' >"$output"
                  touch "$DEX_TEST_RETRY_MARKER"
                  exit 56
                fi
                printf '%s\n' "$DEX_TEST_METADATA_JSON" >"$output"
                ;;
              https://releases.openai.com/dex/channels/latest|https://releases.openai.com/dex/releases/*/release.json)
                if [ "$DEX_TEST_RELEASES_MODE" = "channel_failure" ]; then
                  exit 22
                fi
                printf '%s\n' "$DEX_TEST_RELEASES_METADATA_JSON" >"$output"
                ;;
              https://releases.openai.com/dex/releases/*/dex-package_SHA256SUMS)
                if [ "$DEX_TEST_RELEASES_MODE" = "asset_fallback" ]; then
                  exit 22
                fi
                if [ "$DEX_TEST_RELEASES_MODE" = "corrupt_assets" ] ||
                  [ "$DEX_TEST_RELEASES_MODE" = "corrupt_checksum_and_github" ]; then
                  printf '<html>proxy error</html>\n' >"$output"
                  exit 0
                fi
                if [ -n "$DEX_TEST_RELEASES_CHECKSUM_PATH" ]; then
                  cp "$DEX_TEST_RELEASES_CHECKSUM_PATH" "$output"
                else
                  exit 22
                fi
                ;;
              https://releases.openai.com/dex/releases/*/dex-package-*.tar.gz)
                if [ "$DEX_TEST_RELEASES_MODE" = "asset_fallback" ]; then
                  exit 22
                fi
                if [ "$DEX_TEST_RELEASES_MODE" = "corrupt_assets" ]; then
                  printf '<html>proxy error</html>\n' >"$output"
                  exit 0
                fi
                if [ -n "$DEX_TEST_ARCHIVE_PATH" ]; then
                  cp "$DEX_TEST_ARCHIVE_PATH" "$output"
                else
                  exit 22
                fi
                ;;
              https://github.com/ZigHuang/dex/releases/download/*/dex-package_SHA256SUMS)
                if [ "$DEX_TEST_RELEASES_MODE" = "corrupt_checksum_and_github" ]; then
                  printf '<html>proxy error</html>\n' >"$output"
                  exit 0
                fi
                if [ -n "$DEX_TEST_CHECKSUM_PATH" ]; then
                  cp "$DEX_TEST_CHECKSUM_PATH" "$output"
                else
                  exit 22
                fi
                ;;
              https://github.com/ZigHuang/dex/releases/download/*/dex-package-*.tar.gz)
                if [ "$DEX_TEST_NETWORK_SCENARIO" = "cached_archive" ]; then
                  case "$*" in
                    *"--continue-at -"*) ;;
                    *) exit 91 ;;
                  esac
                  [ "$(wc -c < "$output")" -eq 128 ] || exit 91
                fi
                if [ -n "$DEX_TEST_ARCHIVE_PATH" ]; then
                  cp "$DEX_TEST_ARCHIVE_PATH" "$output"
                else
                  exit 22
                fi
                ;;
              https://github.com/ZigHuang/dex/releases/download/*/dex-npm-*.tgz)
                if [ -n "$DEX_TEST_LEGACY_ARCHIVE_PATH" ]; then
                  cp "$DEX_TEST_LEGACY_ARCHIVE_PATH" "$output"
                else
                  exit 22
                fi
                ;;
              *)
                exit 22
                ;;
            esac
            """
        ),
        encoding="utf-8",
    )
    if curl_wrapper is not None:
        fake_curl.write_text(curl_wrapper, encoding="utf-8")
    fake_curl.chmod(0o755)
    if force_macos:
        fake_uname = bin_dir / "uname"
        fake_uname.write_text(
            "#!/bin/sh\n"
            'case "$1" in\n'
            "  -s) printf 'Darwin\\n' ;;\n"
            "  -m) printf 'arm64\\n' ;;\n"
            "esac\n",
            encoding="utf-8",
        )
        fake_uname.chmod(0o755)
    if old_updater_parent_pid is not None:
        fake_ps = bin_dir / "ps"
        fake_ps.write_text(
            "#!/bin/sh\nexit 1\n"
            if fail_ps
            else "#!/bin/sh\n"
            'case "$*" in\n'
            '  *lstart*) printf "S %s\\n" "$DEX_TEST_PARENT_START" ;;\n'
            '  *) printf "%s\\n" "$DEX_TEST_PARENT_PID" ;;\n'
            "esac\n",
            encoding="utf-8",
        )
        fake_ps.chmod(0o755)

    home = root / "home"
    home.mkdir(exist_ok=True)
    env = os.environ.copy()
    env.update(
        {
            "DEX_HOME": str(root / "dex-home"),
            "DEX_INSTALL_DIR": str(root / "install-bin"),
            "DEX_NON_INTERACTIVE": "1",
            "DEX_RELEASE": release,
            "DEX_INSTALL_DAEMON_ONLY": "1" if daemon_only else "0",
            "DEX_TEST_ARCHIVE_PATH": str(archive_path or ""),
            "DEX_TEST_CHECKSUM_PATH": str(checksum_path or ""),
            "DEX_TEST_RELEASES_CHECKSUM_PATH": str(
                releases_checksum_path or checksum_path or ""
            ),
            "DEX_TEST_LEGACY_ARCHIVE_PATH": str(legacy_archive_path or ""),
            "DEX_TEST_METADATA_FAILURE": "1" if metadata_failure else "0",
            "DEX_TEST_METADATA_JSON": (
                metadata_json if metadata_json is not None else release_metadata()
            ),
            "DEX_TEST_RELEASES_METADATA_JSON": (
                releases_metadata_json
                if releases_metadata_json is not None
                else metadata_json
                if metadata_json is not None
                else release_metadata()
            ),
            "DEX_TEST_RELEASES_MODE": releases_mode,
            "DEX_TEST_REQUEST_LOG": str(request_log),
            "DEX_TEST_NETWORK_SCENARIO": network_scenario,
            "DEX_TEST_RETRY_MARKER": str(root / "retry-marker"),
            "DEX_TEST_CURL_ARGUMENTS": str(root / "curl-arguments.log"),
            "DEX_TEST_DAEMON_HOME_LOG": str(root / "daemon-home.log"),
            "HOME": str(home),
            "PATH": f"{bin_dir}:/usr/bin:/bin",
            "SHELL": "/bin/sh",
        }
    )
    env["DEX_INSTALL_IF_CURRENT"] = "1" if manual_update else "0"
    if update_guard_from_release is None:
        env.pop("DEX_INSTALL_IF_LATEST", None)
        env.pop("DEX_UPDATE_FROM_RELEASE", None)
    else:
        env["DEX_INSTALL_IF_LATEST"] = "0" if manual_update else "1"
        env["DEX_UPDATE_FROM_RELEASE"] = update_guard_from_release
    if old_updater_parent_pid is not None:
        env["DEX_TEST_PARENT_PID"] = str(old_updater_parent_pid)
        env["DEX_TEST_PARENT_START"] = process_start_time()
    if use_mirror is None:
        env.pop("DEX_INSTALLER_USE_RELEASES_OPENAI_COM", None)
    else:
        env["DEX_INSTALLER_USE_RELEASES_OPENAI_COM"] = "TRUE" if use_mirror else "false"
    result = subprocess.run(
        ["/bin/sh", str(installer_path)] + (["--use-codex-config"] if use_codex_config else []),
        capture_output=True,
        check=False,
        env=env,
        text=True,
    )
    requests = (
        request_log.read_text(encoding="utf-8").splitlines()
        if request_log.exists()
        else []
    )
    return result, requests


def process_start_time() -> str:
    details = subprocess.check_output(
        ["ps", "-p", str(os.getpid()), "-o", "stat=", "-o", "lstart="], text=True
    ).strip()
    return details.split(maxsplit=1)[1]


def create_package_release(
    root: Path,
    *,
    metadata_version: str = VERSION,
    binary_version: str = VERSION,
    target: str = "aarch64-apple-darwin",
    include_exec_server: bool = False,
    record_daemon_home: bool = False,
) -> tuple[Path, Path, str]:
    package_dir = root / "package"
    (package_dir / "bin").mkdir(parents=True)
    (package_dir / "dex-path").mkdir()
    metadata = {"version": binary_version, "target": target, "entrypoint": "bin/dex"}
    if include_exec_server:
        metadata["execServer"] = "bin/dex-exec-server"
        write_executable(package_dir / "bin/dex-exec-server", "#!/bin/sh\nexit 0\n")
    (package_dir / "dex-package.json").write_text(
        json.dumps(metadata),
        encoding="utf-8",
    )
    write_executable(
        package_dir / "bin" / "dex",
        "#!/bin/sh\n"
        + ('if [ "$*" = "app-server daemon start" ]; then\n'
           '  printf "%s\\n" "$DEX_HOME" > "$DEX_TEST_DAEMON_HOME_LOG"\n'
           '  exit 0\n'
           'fi\n' if record_daemon_home else "")
        + f"printf 'dex-cli {binary_version}\\n'\n",
    )
    write_executable(
        package_dir / "bin" / "dex-code-mode-host",
        "#!/bin/sh\nexit 0\n",
    )
    write_executable(package_dir / "dex-path" / "rg", "#!/bin/sh\nexit 0\n")
    if "linux" in target:
        (package_dir / "dex-resources").mkdir()
        write_executable(package_dir / "dex-resources/bwrap", "#!/bin/sh\nexit 0\n")

    asset = f"dex-package-{target}.tar.gz"
    archive_path = root / asset
    with tarfile.open(archive_path, "w:gz") as archive:
        for path in package_dir.iterdir():
            archive.add(path, arcname=path.name)

    archive_digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    checksum_path = root / "dex-package_SHA256SUMS"
    checksum_path.write_text(f"{archive_digest}  {asset}\n", encoding="utf-8")
    checksum_digest = hashlib.sha256(checksum_path.read_bytes()).hexdigest()
    metadata_json = json.dumps(
        {
            "assets": [
                {"name": asset, "digest": f"sha256:{archive_digest}"},
                {
                    "name": "dex-package_SHA256SUMS",
                    "digest": f"sha256:{checksum_digest}",
                },
            ],
            "tag_name": f"rust-v{metadata_version}",
        },
        indent=2,
    )
    return archive_path, checksum_path, metadata_json


def create_legacy_release(root: Path) -> tuple[Path, str]:
    package_dir = root / "legacy-package"
    vendor_dir = package_dir / "package" / "vendor" / "aarch64-apple-darwin"
    (vendor_dir / "dex").mkdir(parents=True)
    (vendor_dir / "path").mkdir()
    write_executable(
        vendor_dir / "dex" / "dex",
        f"#!/bin/sh\nprintf 'dex-cli {VERSION}\\n'\n",
    )
    write_executable(vendor_dir / "path" / "rg", "#!/bin/sh\nexit 0\n")

    asset = f"dex-npm-darwin-arm64-{VERSION}.tgz"
    archive_path = root / asset
    with tarfile.open(archive_path, "w:gz") as archive:
        archive.add(package_dir / "package", arcname="package")

    archive_digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    metadata_json = json.dumps(
        {
            "assets": [{"name": asset, "digest": f"sha256:{archive_digest}"}],
            "tag_name": f"rust-v{VERSION}",
        },
        indent=2,
    )
    return archive_path, metadata_json


def write_executable(path: Path, contents: str) -> None:
    path.write_text(contents, encoding="utf-8")
    path.chmod(0o755)


def release_metadata(*, compact: bool = False, reorder: bool = False) -> str:
    assets = [
        asset_metadata(
            f"dex-package-{target}.tar.gz",
            f"sha256:{'a' * 64}",
            reorder=reorder,
        )
        for target in (
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "aarch64-unknown-linux-musl",
            "x86_64-unknown-linux-musl",
        )
    ]
    assets.append(
        asset_metadata(
            "dex-package_SHA256SUMS",
            f"sha256:{'b' * 64}",
            reorder=reorder,
        )
    )
    separators = (",", ":") if compact else None
    return json.dumps(
        {"assets": assets, "body": "braces: { } [ ]", "tag_name": f"rust-v{VERSION}"},
        indent=None if compact else 2,
        separators=separators,
    )


def asset_metadata(name: str, digest: str, *, reorder: bool) -> dict[str, str]:
    if reorder:
        return {"digest": digest, "name": name}
    return {"name": name, "digest": digest}


def legacy_release_metadata_with_decoys() -> str:
    fake_digest = f"sha256:{'0' * 64}"
    assets = [
        {
            "metadata": {
                "name": "dex-package-x86_64-unknown-linux-musl.tar.gz",
                "digest": fake_digest,
            },
            "digest": f"sha256:{'c' * 64}",
            "name": f"dex-npm-{target}-{VERSION}.tgz",
        }
        for target in ("darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64")
    ]
    return json.dumps(
        {
            "body": (
                f'fake: {{"name":"dex-package_SHA256SUMS","digest":"{fake_digest}"}}'
            ),
            "assets": assets,
            "tag_name": f"rust-v{VERSION}",
        },
        separators=(",", ":"),
    )


if __name__ == "__main__":
    unittest.main()
