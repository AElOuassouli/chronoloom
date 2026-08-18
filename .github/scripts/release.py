#!/usr/bin/env python3
"""Bump a library's version, close its changelog, and commit + tag the release.

Driven by the `release-fix` / `release-minor` / `release-major` make targets;
see `make help`. One library per run -- the two version independently, and a
tag only ever releases one of them.

What it touches, for `release-minor chronoloompy` at 0.1.0:

    chronoloompy/pyproject.toml   version = "0.2.0"
    chronoloompy/uv.lock          refreshed (it embeds the project version, and
                                  CI runs with UV_LOCKED=1, so a stale lock is a
                                  build failure rather than a silent re-lock)
    chronoloompy/CHANGELOG.md     "# Unreleased" -> "# 0.2.0 - 2026-08-18"
    git                           commit + annotated tag chronoloompy-v0.2.0

Nothing is pushed. The tag is what triggers publishing, so pushing stays a
deliberate act -- see the next-steps output.
"""

from __future__ import annotations

import argparse
import datetime
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

# find_section is the same parser the CD workflow gates releases with, so a
# changelog this script writes is one that workflow will accept.
from changelog import find_section

REPO_ROOT = Path(__file__).resolve().parents[2]
RELEASE_BRANCH = "main"
UNRELEASED = "Unreleased"


@dataclass(frozen=True)
class Library:
    """One publishable library and the files that carry its version."""

    name: str
    manifest: Path
    # The table whose `version = "..."` is the release version. Both manifests
    # contain several such keys (dependencies, other tables), so the section
    # header is what disambiguates.
    manifest_table: str
    lock_command: list[str] | None

    @property
    def changelog(self) -> Path:
        return REPO_ROOT / self.name / "CHANGELOG.md"

    def tag(self, version: str) -> str:
        return f"{self.name}-v{version}"


LIBRARIES = {
    "chronoloom": Library(
        name="chronoloom",
        manifest=REPO_ROOT / "chronoloom" / "Cargo.toml",
        manifest_table="package",
        # Rewrites Cargo.lock's entry for the crate. --offline because bumping a
        # version must not silently pick up new dependency versions too.
        lock_command=[
            "cargo",
            "metadata",
            "--manifest-path",
            "chronoloom/Cargo.toml",
            "--format-version",
            "1",
            "--offline",
        ],
    ),
    "chronoloompy": Library(
        name="chronoloompy",
        manifest=REPO_ROOT / "chronoloompy" / "pyproject.toml",
        manifest_table="project",
        lock_command=["uv", "lock", "--project", "chronoloompy", "--offline"],
    ),
}

PARTS = ("major", "minor", "fix")


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def git(*args: str, capture: bool = True) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=REPO_ROOT,
        capture_output=capture,
        text=True,
        check=True,
    )
    return (result.stdout or "").strip()


def version_pattern(table: str) -> re.Pattern[str]:
    """Match the `version = "..."` belonging to `[table]`, and nothing else."""
    return re.compile(
        rf'(?ms)^\[{re.escape(table)}\]\s*$.*?^version\s*=\s*"(?P<version>[^"]+)"',
    )


def read_version(library: Library) -> str:
    text = library.manifest.read_text(encoding="utf-8")
    match = version_pattern(library.manifest_table).search(text)
    if match is None:
        fail(f"no [{library.manifest_table}] version found in {library.manifest}")
    return match.group("version")  # type: ignore[union-attr]


def bump(version: str, part: str) -> str:
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        fail(f"cannot bump non-semver version {version!r}")
    major, minor, patch = (int(piece) for piece in version.split("."))
    if part == "major":
        return f"{major + 1}.0.0"
    if part == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def write_version(library: Library, new_version: str) -> None:
    """Replace only the version literal, leaving the rest of the file byte-identical."""
    text = library.manifest.read_text(encoding="utf-8")
    match = version_pattern(library.manifest_table).search(text)
    assert match is not None  # read_version already validated this
    start, end = match.span("version")
    library.manifest.write_text(
        text[:start] + new_version + text[end:], encoding="utf-8"
    )


def close_changelog(library: Library, new_version: str, today: str) -> None:
    """Rename the `# Unreleased` heading to this version, dated."""
    text = library.changelog.read_text(encoding="utf-8")
    heading = re.compile(rf"(?m)^#\s*{UNRELEASED}\s*$")
    if heading.search(text) is None:
        fail(
            f"{library.changelog} has no '# {UNRELEASED}' section. "
            f"Document the changes before releasing."
        )
    library.changelog.write_text(
        heading.sub(f"# {new_version} - {today}", text, count=1), encoding="utf-8"
    )


def preflight(library: Library, new_version: str) -> None:
    """Refuse anything the CD workflow would later reject, or that loses work."""
    branch = git("rev-parse", "--abbrev-ref", "HEAD")
    if branch != RELEASE_BRANCH:
        fail(
            f"on branch {branch!r}, releases must be cut from {RELEASE_BRANCH!r} -- "
            f"the CD workflow rejects a tag whose commit is not on {RELEASE_BRANCH}"
        )
    if git("status", "--porcelain"):
        fail("working tree is not clean; commit or stash first")

    tag = library.tag(new_version)
    if git("tag", "--list", tag):
        fail(f"tag {tag} already exists")

    # An empty Unreleased section means there is nothing to announce, and would
    # produce empty GitHub Release notes.
    body = find_section(library.changelog, UNRELEASED)
    if not body:
        fail(f"{library.changelog} has no content under '# {UNRELEASED}'")


def refresh_lock(library: Library) -> None:
    if library.lock_command is None:
        return
    try:
        subprocess.run(
            library.lock_command,
            cwd=REPO_ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError:
        fail(f"{library.lock_command[0]} not found; cannot refresh the lockfile")
    except subprocess.CalledProcessError as error:
        fail(f"failed to refresh the lockfile:\n{error.stderr}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("library", choices=sorted(LIBRARIES))
    parser.add_argument("part", choices=PARTS)
    args = parser.parse_args()

    library = LIBRARIES[args.library]
    old_version = read_version(library)
    new_version = bump(old_version, args.part)
    preflight(library, new_version)

    # UTC rather than local time: the date lands in a changelog and a git tag,
    # both of which are read by people in other timezones and by CI.
    today = datetime.datetime.now(tz=datetime.UTC).date().isoformat()
    write_version(library, new_version)
    close_changelog(library, new_version, today)
    refresh_lock(library)

    tag = library.tag(new_version)
    # Stage only this library's files plus its lockfile; preflight guaranteed the
    # tree was otherwise clean, so `git add -A` would be equivalent but broader.
    git("add", "--all", library.name)
    git("commit", "-m", f"release: {library.name} {new_version}")

    notes = find_section(library.changelog, new_version) or []
    git(
        "tag", "-a", tag, "-m", "\n".join([f"{library.name} {new_version}", "", *notes])
    )

    print(f"{library.name}: {old_version} -> {new_version}")
    print(f"committed and tagged {tag}\n")
    print("Nothing has been pushed. To release:")
    print(f"    git push origin {RELEASE_BRANCH} {tag}")
    print("\nTo undo:")
    print(f"    git tag -d {tag} && git reset --hard HEAD~1")
    return 0


if __name__ == "__main__":
    sys.exit(main())
