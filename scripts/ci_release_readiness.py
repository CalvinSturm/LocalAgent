#!/usr/bin/env python3
from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CARGO_TOML = ROOT / "Cargo.toml"
CHANGELOG = ROOT / "CHANGELOG.md"
RELEASE_INDEX = ROOT / "docs" / "release-notes" / "README.md"
RELEASE_DIR = ROOT / "docs" / "release-notes"

SCHEMA_CHANGE_RE = re.compile(
    r"^[+-](?![+-]{3})(?=.*(?:SCHEMA_VERSION|schema_version|openagent\.[\w.-]+\.v\d+)).*",
    re.MULTILINE,
)
SCHEMA_NOTE_RE = re.compile(r"(?im)^\s{0,3}(##|###)\s+Schema Notes\b")


def fail(msg: str) -> None:
    print(f"[release-readiness] ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError:
        fail(f"missing required file: {path.relative_to(ROOT)}")


def cargo_version() -> str:
    content = read_text(CARGO_TOML)
    m = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', content)
    if not m:
        fail("could not parse package version from Cargo.toml")
    return m.group(1)


def check_version_linkage(version: str) -> None:
    tag = f"v{version}"
    release_filename = f"RELEASE_NOTES_{tag}.md"
    release_path = RELEASE_DIR / release_filename

    changelog = read_text(CHANGELOG)
    if f"## {tag}" not in changelog:
        fail(f"`CHANGELOG.md` is missing a release heading for `{tag}`")

    index_text = read_text(RELEASE_INDEX)
    index_link = f"[{tag}]({release_filename})"
    if index_link not in index_text:
        fail(
            "`docs/release-notes/README.md` is missing the current version link "
            f"`{index_link}`"
        )

    if not release_path.exists():
        fail(f"missing versioned release notes file: `{release_path.relative_to(ROOT)}`")


def git_output(args: list[str]) -> str:
    proc = subprocess.run(
        args,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.strip() or "git command failed")
    return proc.stdout


def git_ref_exists(ref: str) -> bool:
    proc = subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", ref],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return proc.returncode == 0


def diff_range() -> str | None:
    event_name = os.environ.get("GITHUB_EVENT_NAME", "")
    if event_name == "pull_request":
        base = os.environ.get("GITHUB_BASE_SHA")
        head = os.environ.get("GITHUB_SHA")
        if base and head and git_ref_exists(base) and git_ref_exists(head):
            return f"{base}..{head}"
    elif event_name == "push":
        before = os.environ.get("GITHUB_EVENT_BEFORE")
        head = os.environ.get("GITHUB_SHA")
        if before and head and before != ("0" * 40) and git_ref_exists(before) and git_ref_exists(head):
            return f"{before}..{head}"

    if git_ref_exists("HEAD~1"):
        return "HEAD~1..HEAD"
    return None


def schema_changed() -> bool:
    rng = diff_range()
    if not rng:
        print("[release-readiness] INFO: no diff range available; skipping schema-change gate")
        return False

    try:
        diff = git_output(["git", "diff", "--unified=0", rng, "--", "src", "tests", "docs"])
    except RuntimeError as e:
        print(f"[release-readiness] INFO: failed to inspect git diff ({e}); skipping schema-change gate")
        return False

    return bool(SCHEMA_CHANGE_RE.search(diff))


def check_schema_note(version: str) -> None:
    tag = f"v{version}"
    release_path = RELEASE_DIR / f"RELEASE_NOTES_{tag}.md"
    changelog = read_text(CHANGELOG)
    release_notes = read_text(release_path)

    if SCHEMA_NOTE_RE.search(changelog) or SCHEMA_NOTE_RE.search(release_notes):
        return

    fail(
        "schema-like constants changed, but no `Schema Notes` section was found in "
        f"`CHANGELOG.md` or `{release_path.relative_to(ROOT)}` for `{tag}`. "
        "Add a `### Schema Notes` (or `## Schema Notes`) section describing compatibility impact."
    )


def main() -> None:
    version = cargo_version()
    check_version_linkage(version)
    if schema_changed():
        check_schema_note(version)
    print(f"[release-readiness] OK (version v{version})")


if __name__ == "__main__":
    main()
