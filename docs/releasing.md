# Releasing

The two libraries are versioned and released independently. A tag releases
exactly one of them, chosen by its prefix:

| Tag | Workflow | Publishes to |
| --- | --- | --- |
| `chronoloom-v<version>` | [`cd-chronoloom.yml`](../.github/workflows/cd-chronoloom.yml) | crates.io |
| `chronoloompy-v<version>` | [`cd-chronoloompy.yml`](../.github/workflows/cd-chronoloompy.yml) | PyPI (wheels and sdist) |

An unprefixed `v*` tag does nothing.

## Cutting a release

From a clean checkout of `main`, run one of:

```sh
make chronoloom-release-fix      # or -minor / -major
make chronoloompy-release-fix    # or -minor / -major
```

(equivalently `make -C <library> release-fix` from inside a library). The
script, [`.github/scripts/release.py`](../.github/scripts/release.py):

1. bumps the version in the library's manifest (`chronoloom/Cargo.toml` or
   `chronoloompy/pyproject.toml`);
2. dates the changelog's `# Unreleased` heading, turning it into
   `# <version> - <date>`;
3. refreshes the lockfile (`chronoloom/Cargo.lock` or `chronoloompy/uv.lock`),
   since both embed the version and CI runs with `UV_LOCKED=1`;
4. commits and creates an annotated tag.

Nothing is pushed: the script prints the `git push` to run, and the command to
undo the release. It refuses to start, before touching any file, unless it runs
on `main` with a clean tree and a non-empty `# Unreleased` section.

## What the workflow checks

Before publishing, each workflow refuses the release if:

- the tagged commit is not on `main`;
- the tag does not match the version in the manifest;
- the changelog has no section for that version.

Publishing then pauses for a manual approval in the `crates-io` or `pypi` GitHub
environment. Both registries use Trusted Publishing (OIDC): no registry token is
stored in the repository secrets. Once published, the workflow creates the
GitHub Release with the changelog section as its notes.

Every change to `chronoloom` also runs `cargo publish --dry-run` in CI, because a
published crate version is permanent and cannot be reused.

## Exposing a core change to Python

`chronoloompy/rust` depends on the released `chronoloom` crate, not on the
`chronoloom/` directory. A change to the core therefore reaches Python in two
releases:

1. land the change in `chronoloom` and release it (`chronoloom-v*`);
2. bump the `chronoloom` requirement in
   [`chronoloompy/rust/Cargo.toml`](../chronoloompy/rust/Cargo.toml), adapt the
   binding, and release `chronoloompy` (`chronoloompy-v*`).

Until step 2, `make test` at the root still passes, because each library is
tested against what it actually builds against.
