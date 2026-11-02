#!/usr/bin/env python3

from pathlib import Path
import hashlib
import io
import tarfile
import json
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from build_winget_package import prepare_winget_package
from dex_package.archive import write_archive, resolve_zstd_command
from dex_package.layout import build_package_dir
from dex_package.layout import validate_package_dir
from dex_package.targets import PACKAGE_VARIANTS
from dex_package.targets import PackageInputs
from dex_package.targets import TARGET_SPECS
from dex_package.targets import default_target


class PackageLayoutTest(unittest.TestCase):
    @unittest.skipIf(sys.platform == "win32", "MSVC symbols are separate PDB files")
    def test_strip_policy_preserves_inputs_and_runs_packaged_executables(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "input-executable"
            # System executables may already be stripped. Compile with debug
            # information so a no-op strip cannot pass this regression test.
            subprocess.run(
                ["cc", "-g", "-O0", "-x", "c", "-", "-o", str(source)],
                input=r"""#include <stdio.h>
int main(void) {
    fputs("package-ok", stdout);
    return 0;
}
""",
                text=True,
                check=True,
                capture_output=True,
            )
            original = source.read_bytes()
            target = default_target()
            for mode in ("auto", "all", "none"):
                with self.subTest(mode=mode):
                    package = root / mode
                    command = [
                        sys.executable,
                        str(
                            Path(__file__).resolve().parents[1] / "build_dex_package.py"
                        ),
                        "--target",
                        target,
                        "--cargo-profile",
                        "release",
                        "--package-dir",
                        str(package),
                        "--strip",
                        mode,
                    ]
                    flags = [
                        "--entrypoint-bin",
                        "--code-mode-host-bin",
                        "--exec-server-bin",
                        "--rg-bin",
                    ]
                    if "linux" in target:
                        flags.append("--bwrap-bin")
                    for flag in flags:
                        command.extend((flag, str(source)))
                    if mode != "all":
                        # Preserved prebuilt inputs must not even need a strip tool.
                        command.extend(("--strip-tool", str(root / "absent-strip")))
                    subprocess.run(command, check=True, capture_output=True)
                    for name in ("dex", "dex-code-mode-host", "dex-exec-server"):
                        executable = package / "bin" / name
                        self.assertEqual(
                            subprocess.check_output([str(executable)]),
                            b"package-ok",
                        )
                        if mode != "all":
                            self.assertEqual(executable.read_bytes(), original)
                        else:
                            self.assertLess(executable.stat().st_size, len(original))
                    self.assertEqual(source.read_bytes(), original)
                    self.assertEqual((package / "dex-path/rg").read_bytes(), original)
                    if "linux" in target:
                        self.assertEqual(
                            (package / "dex-resources/bwrap").read_bytes(), original
                        )

    def test_winget_preserves_signed_files_and_voice_hashes(self) -> None:
        for target in ("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temp:
                package = Path(temp)
                files = {
                    "bin/dex.exe": b"signed CLI",
                    "bin/dex-code-mode-host.exe": b"signed code mode host",
                    "bin/dex-exec-server.exe": b"signed executor",
                    "dex-resources/dex-command-runner.exe": b"signed runner",
                    "dex-resources/dex-windows-sandbox-setup.exe": b"signed setup",
                    "dex-resources/voice/bin/dex-voice-host.exe": b"signed voice host",
                    "dex-resources/voice/bin/gstreamer-1.0-0.dll": b"signed audio DLL",
                    "dex-resources/voice/NOTICE.md": b"license notices",
                    "dex-path/rg.exe": b"ripgrep",
                }
                for name, contents in files.items():
                    path = package / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(contents)
                metadata = {
                    "layoutVersion": 1,
                    "target": target,
                    "entrypoint": "bin/dex.exe",
                }
                (package / "dex-package.json").write_text(json.dumps(metadata))
                manifest = {
                    "schemaVersion": 1,
                    "sha256": {
                        name: hashlib.sha256(contents).hexdigest()
                        for name, contents in files.items()
                        if name == "bin/dex.exe"
                        or name.startswith("dex-resources/voice/")
                    },
                }
                manifest_path = package / "dex-resources/voice/manifest.json"
                manifest_path.write_text(json.dumps(manifest))
                prepare_winget_package(package)
                entrypoint = f"dex-{target}.exe"
                files[entrypoint] = files.pop("bin/dex.exe")
                files["dex-code-mode-host.exe"] = files.pop(
                    "bin/dex-code-mode-host.exe"
                )
                for helper in (
                    "dex-command-runner.exe",
                    "dex-windows-sandbox-setup.exe",
                ):
                    files[helper] = files[f"dex-resources/{helper}"]
                actual = {
                    str(path.relative_to(package)).replace("\\", "/"): path.read_bytes()
                    for path in package.rglob("*")
                    if path.is_file()
                }
                actual_metadata = json.loads(actual.pop("dex-package.json"))
                actual_manifest = json.loads(
                    actual.pop("dex-resources/voice/manifest.json")
                )
                self.assertEqual(actual, files)
                metadata["entrypoint"] = entrypoint
                self.assertEqual(actual_metadata, metadata)
                manifest["sha256"][entrypoint] = manifest["sha256"].pop("bin/dex.exe")
                self.assertEqual(actual_manifest, manifest)
                for name, digest in actual_manifest["sha256"].items():
                    self.assertEqual(hashlib.sha256(actual[name]).hexdigest(), digest)

    def test_macos_package_preserves_prebuilt_resource_binaries(self) -> None:
        for variant_name in ("dex", "dex-app-server"):
            for target in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
                with self.subTest(variant=variant_name, target=target):
                    with tempfile.TemporaryDirectory() as temp_dir:
                        root = Path(temp_dir)
                        package_dir = root / "package"
                        package_dir.mkdir()
                        rg_bin = touch_executable(root / "signed-rg")
                        rg_bin.write_bytes(b"signed ripgrep binary")
                        variant = PACKAGE_VARIANTS[variant_name]
                        spec = TARGET_SPECS[target]
                        inputs = PackageInputs(
                            entrypoint_bin=touch_executable(
                                root / variant.executable_stem
                            ),
                            code_mode_host_bin=touch_executable(
                                root / "dex-code-mode-host"
                            ),
                            exec_server_bin=rg_bin,
                            rg_bin=rg_bin,
                            bwrap_bin=None,
                            dex_command_runner_bin=None,
                            dex_windows_sandbox_setup_bin=None,
                        )

                        build_package_dir(package_dir, "1.2.3", variant, spec, inputs)
                        validate_package_dir(package_dir, variant, spec)
                        for suffix in (".tar.gz", ".tar.zst"):
                            archive_path = root / f"package{suffix}"
                            write_archive(package_dir, archive_path, force=False)
                            data = (
                                subprocess.check_output(
                                    [
                                        *resolve_zstd_command(),
                                        "-d",
                                        "--stdout",
                                        str(archive_path),
                                    ]
                                )
                                if suffix == ".tar.zst"
                                else archive_path.read_bytes()
                            )
                            with tarfile.open(
                                fileobj=io.BytesIO(data), mode="r:*"
                            ) as archive:
                                member = archive.getmember("bin/dex-exec-server")
                                self.assertTrue(member.mode & 0o111)
                                self.assertEqual(
                                    archive.extractfile(member).read(),
                                    rg_bin.read_bytes(),
                                )

                        self.assertEqual(
                            {
                                "rg": (package_dir / "dex-path" / "rg").read_bytes(),
                                "executor": (
                                    package_dir / "bin/dex-exec-server"
                                ).read_bytes(),
                            },
                            {
                                "rg": b"signed ripgrep binary",
                                "executor": b"signed ripgrep binary",
                            },
                        )

    def test_app_server_package_places_code_mode_host_beside_entrypoint(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            package_dir = root / "package"
            package_dir.mkdir()
            inputs = PackageInputs(
                entrypoint_bin=touch_executable(root / "dex-app-server"),
                code_mode_host_bin=touch_executable(root / "dex-code-mode-host"),
                exec_server_bin=touch_executable(root / "dex-exec-server"),
                rg_bin=touch_executable(root / "rg"),
                bwrap_bin=touch_executable(root / "bwrap"),
                dex_command_runner_bin=None,
                dex_windows_sandbox_setup_bin=None,
            )

            build_package_dir(
                package_dir,
                "1.2.3",
                PACKAGE_VARIANTS["dex-app-server"],
                TARGET_SPECS["x86_64-unknown-linux-musl"],
                inputs,
            )
            validate_package_dir(
                package_dir,
                PACKAGE_VARIANTS["dex-app-server"],
                TARGET_SPECS["x86_64-unknown-linux-musl"],
            )

            self.assertTrue((package_dir / "bin" / "dex-code-mode-host").is_file())


def touch_executable(path: Path) -> Path:
    path.touch(mode=0o755)
    return path


if __name__ == "__main__":
    unittest.main()
