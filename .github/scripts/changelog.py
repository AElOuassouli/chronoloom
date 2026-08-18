#!/usr/bin/env python3
"""Read one version's section out of a CHANGELOG.md.

Used by both release workflows: `check` gates a tag on the changelog actually
having been updated, `extract` supplies the GitHub Release notes.

Deliberately stdlib-only and standalone. The chronoloom workflow releases a Rust
crate and has no Python project to `uv run` this through, so it cannot depend on
chronoloompy's environment.

Section layout, as used by both changelogs:

    # 0.2.0 - 2026-08-18     <- H1 starts a version section
    ## Added                 <- H2 subsections belong to it
    - ...
    # 0.1.0 - 2026-07-01     <- next H1 ends it

A heading matches a version when its first token equals that version, with an
optional leading `v` (so `# v0.1.0` and `# 0.1.0` both match `0.1.0`). Anything
after the version -- a date, a separator -- is ignored.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# Exactly one '#', so the '## Added' subsections are not section boundaries.
H1 = re.compile(r"^#(?!#)\s*(.*?)\s*$")


def _heading_version(line: str) -> str | None:
    """Return the version a heading declares, or None if it is not an H1."""
    match = H1.match(line)
    if match is None:
        return None
    title = match.group(1)
    if not title:
        return None
    return title.split()[0].removeprefix("v")


def find_section(changelog: Path, version: str) -> list[str] | None:
    """Return the body lines of `version`'s section, or None if absent."""
    body: list[str] = []
    found = False
    for line in changelog.read_text(encoding="utf-8").splitlines():
        heading = _heading_version(line)
        if heading is not None:
            if found:  # next version section -- we are done
                break
            found = heading == version
            continue
        if found:
            body.append(line)
    if not found:
        return None
    # Trim the blank lines that padding between headings leaves behind.
    while body and not body[0].strip():
        body.pop(0)
    while body and not body[-1].strip():
        body.pop()
    return body


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["check", "extract"])
    parser.add_argument("changelog", type=Path)
    parser.add_argument("version")
    args = parser.parse_args()

    body = find_section(args.changelog, args.version)
    if body is None:
        print(
            f"::error::{args.changelog} has no section for version {args.version}. "
            f"Rename its '# Unreleased' heading to '# {args.version} - <date>' "
            f"before tagging.",
            file=sys.stderr,
        )
        return 1
    if not body:
        print(
            f"::error::{args.changelog} section for {args.version} is empty.",
            file=sys.stderr,
        )
        return 1
    if args.command == "extract":
        print("\n".join(body))
    return 0


if __name__ == "__main__":
    sys.exit(main())
