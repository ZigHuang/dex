import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts import format as repository_format


class BuildifierFormatterGroupTest(unittest.TestCase):
    def test_formats_existing_tracked_and_untracked_bazel_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            repository_root = Path(temporary_directory)
            (repository_root / "MODULE.bazel").touch()
            (repository_root / "new.bzl").touch()
            (repository_root / "README.md").touch()
            repository_files = (
                b"removed/BUILD.bazel\0new.bzl\0MODULE.bazel\0README.md\0"
            )
            with (
                patch.object(repository_format, "REPO_ROOT", repository_root),
                patch.object(
                    repository_format.subprocess,
                    "check_output",
                    return_value=repository_files,
                ),
            ):
                group = repository_format.buildifier_formatter_group(check=True)

            self.assertEqual(
                group.commands,
                (
                    repository_format.Command(
                        (
                            "dotslash",
                            str(repository_root / "tools" / "buildifier"),
                            "-mode=check",
                            "-lint=off",
                            "MODULE.bazel",
                            "new.bzl",
                        )
                    ),
                ),
            )


if __name__ == "__main__":
    unittest.main()
