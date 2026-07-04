#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("ci_release_readiness.py")
SPEC = importlib.util.spec_from_file_location("ci_release_readiness", SCRIPT)
assert SPEC and SPEC.loader
ci_release_readiness = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci_release_readiness)


class ReleaseReadinessTests(unittest.TestCase):
    def test_version_from_release_tag_strips_v_prefix(self) -> None:
        self.assertEqual(
            ci_release_readiness.version_from_release_tag("v0.6.0-alpha.2"),
            "0.6.0-alpha.2",
        )

    def test_version_from_release_tag_accepts_build_metadata(self) -> None:
        self.assertEqual(
            ci_release_readiness.version_from_release_tag("v1.2.3+build.4"),
            "1.2.3+build.4",
        )

    def test_version_from_release_tag_rejects_non_version_tag(self) -> None:
        with self.assertRaises(ValueError):
            ci_release_readiness.version_from_release_tag("release-candidate")

    def test_parse_cargo_package_version_reads_package_section_only(self) -> None:
        content = """
[dependencies]
example = { version = "9.9.9" }

[package]
name = "localagent"
version = "0.6.0-alpha.2"
"""
        self.assertEqual(
            ci_release_readiness.parse_cargo_package_version(content),
            "0.6.0-alpha.2",
        )

    def test_release_tag_mismatch_message_reports_both_versions(self) -> None:
        message = ci_release_readiness.release_tag_mismatch_message(
            "v0.6.0-alpha.2",
            "0.5.0",
        )
        assert message is not None
        self.assertIn("0.6.0-alpha.2", message)
        self.assertIn("0.5.0", message)


if __name__ == "__main__":
    unittest.main()
