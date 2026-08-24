# Unreleased

## Added

- `primitives`, the event vocabulary the rest of the library builds on. Both
  shapes are generic over the value they carry, so a payload can be a
  measurement, a label, a set of tags, or nothing at all (`()`).
- `primitives::TimePointEvent`, a value anchored to a single instant with no
  duration.
- `primitives::TimeIntervalEvent`, a value attached to a half-open span
  `[start, end)`. Construction is validated: `end` must be strictly after
  `start`, so empty and inverted spans are rejected and every interval covers
  at least one instant. `TimeIntervalEvent::span` builds the valueless case.
- `primitives::IntervalError`, the validation failure returned by those
  constructors, implementing `Display` and `std::error::Error`.
- `primitives::Timestamp`, an alias for `i64` naming the crate's time unit.
- The three types and the error are re-exported at the crate root.
- `TimeIntervalEvent::intersection`, the span where both intervals are active,
  or `None` where they never are. Intervals that merely touch share no instant
  and so do not intersect.
- `TimeIntervalEvent::union`, the spans covered by either interval: one when
  they combine, two — ordered by start — when a gap keeps them apart.
  Intervals that touch combine, since `[0, 5)` and `[5, 9)` together cover
  exactly `[0, 9)`. Both operations work on the time dimension only: they
  accept intervals carrying different kinds of value, consume neither, and
  return valueless spans.
- `sequences`, holding ordered collections of events.
- `sequences::TimePointSequence`, a collection of point events that is always
  in time order however the events arrive. Events sit contiguously, sorted by
  timestamp, so lookups binary-search that maintained order rather than
  scanning, and adding an event that belongs at the end — the usual case for
  events arriving in time order — costs no shifting at all. Several events may
  share an instant, where they keep the order they were added; `len` counts
  events and `instant_count` counts instants.
- Window and neighbour queries on that sequence: `range` over any Rust range,
  `first` / `last`, `before` / `after` (both bounds inclusive), and `nearest`,
  which returns the closest event in either direction and breaks a tie toward
  the earlier one.
- Slice and positional access: `as_slice`, `get` (by instant) and `range` both
  return slices of the sequence itself rather than copies, and `nth` plus an
  `Index<usize>` impl read by position in constant time.
- `FromIterator`, `Extend`, and `IntoIterator` for `TimePointSequence`, owned
  and borrowed. `FromIterator` sorts once instead of inserting one event at a
  time, and `Extend` reorders only when the additions actually broke the order.
- `TimePointSequence` is re-exported at the crate root.

## Removed

- The `algebra` module, and with it `algebra::intersection`. Intersection now
  lives on the primitive as `TimeIntervalEvent::intersection`, so the half-open
  rule has a single definition instead of one for tuples and one for events.
  Callers holding `(start, end)` pairs build a `TimeIntervalEvent` first.

# 0.2.0 - 2026-08-20

## Fixed

- Releasing now runs the full CI suite before publishing. `cd-chronoloom` calls
  `ci-chronoloom` as a reusable workflow and gates the crates.io publish on it;
  previously a tag published whatever it pointed at without running a single
  test. This replaces the standalone `cargo publish --dry-run` step, which the
  reused `package` job already performs.
- `ci-chronoloom` no longer runs on pushes to `main`. A pull request check
  already tests the merge result, so the post-merge run duplicated it.

## Added

- Initial standalone crate, extracted from the combined `timewarp` crate that
  previously served as both the Rust logic and its PyO3 binding.
- `algebra::intersection` for half-open intervals, with doc tests.
- `make release-fix` / `release-minor` / `release-major` (and
  `make chronoloom-release-*` from the repo root) bump the version, date the
  changelog, refresh `Cargo.lock`, then commit and tag. Nothing is pushed.
- Release automation: a `chronoloom-v*` tag publishes to crates.io via Trusted
  Publishing (OIDC, no stored token) after a manual approval, and cuts a GitHub
  Release from this changelog. The tag is rejected unless its commit is on
  `main`, its version matches `Cargo.toml`, and this file documents it.
