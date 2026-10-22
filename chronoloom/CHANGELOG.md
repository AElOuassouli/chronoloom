# 0.3.0 - 2026-10-22

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
- Every `TimePointSequence` has an attribute, the quantity it records, so
  `new` and `from_events` take it first, and `attribute`, `set_attribute`,
  `label` and `Display` read and change it. The attribute is part of equality:
  two sequences holding the same events for different attributes are not
  equal. There is no sequence without one, so neither `Default` nor `FromIterator` is
  implemented — the same reason `TimeIntervalSequence` has neither.
- Window and neighbour queries on that sequence: `range` over any Rust range,
  `first` / `last`, `before` / `after` (both bounds inclusive), and `nearest`,
  which returns the closest event in either direction and breaks a tie toward
  the earlier one.
- Slice and positional access: `as_slice`, `get` (by instant) and `range` both
  return slices of the sequence itself rather than copies, and `nth` plus an
  `Index<usize>` impl read by position in constant time.
- `TimePointSequence::from_events` and `into_events`, for building a sequence
  from data that already exists and getting it back out. `from_events` sorts
  the `Vec` it is given in place rather than building a second one, and leaves
  already-ordered input alone; `into_events` hands back the `Vec` unchanged.
- `Extend` and `IntoIterator` for `TimePointSequence`, owned and borrowed.
  `Extend` reorders only when the additions actually broke the order.
- `sequences::TimeIntervalSequence`, the other sequence shape: one state over
  time, as the spans during which it was active. Every span means the same
  thing, so the sequence carries no per-span value — when state values are
  modelled they will belong to the sequence as a whole.
- That timeline is kept **normalized**: sorted by start, pairwise disjoint, and
  with no two spans left touching. Overlapping *and touching* spans merge as
  they arrive, since `[0, 5)` and `[5, 9)` together cover exactly `[0, 9)` —
  the same rule `TimeIntervalEvent::union` applies to a pair. Consequently
  `len` counts the spans remaining after merging rather than the number
  inserted, and two sequences are equal exactly when they cover the same
  instants, however they were built.
- `TimeIntervalSequence::from_spans` / `into_spans`, `insert` (merging),
  `remove` by position, `at` and `contains` for whether the state was active at
  an instant, plus `as_slice`, `nth`, `first` / `last`, `iter`, and the
  `FromIterator` / `Extend` / `IntoIterator` / `Index` impls, mirroring
  `TimePointSequence` wherever the two shapes agree.
- Both sequences are re-exported at the crate root.
- Set algebra between two `TimeIntervalSequence`s: `union` (either timeline),
  `intersection` (both), `difference` (this one but not the other), and
  `symmetric_difference` (exactly one — the XOR). Every operation borrows both
  operands, leaves them untouched, and returns a new sequence.
- Because both timelines are already normalized, each operation is a single
  pass over the two with no sorting — linear in their combined length. Results
  come back normalized by construction, so nothing is re-sorted either.
- `TimeIntervalSequence::active_duration`, the total time the timeline covers.
  Maintained as spans arrive and leave rather than computed on demand, so
  reading it is constant time. It counts covered *instants*, not inserted
  spans: overlapping inserts contribute their union once, and touching spans
  contribute the whole they merge into — normalization is what makes the total
  well defined. Unsigned and never saturating, unlike
  `TimeIntervalEvent::duration`, which must clamp to stay in `i64`: spans are
  disjoint and bounded by `i64`, so a total cannot exceed
  `i64::MAX - i64::MIN`, which `u64` represents exactly. Equality is unchanged
  and still compares coverage alone.
- `TimeIntervalEvent::merged`, the single span covering two intervals, or
  `None` when a gap separates them. The non-allocating counterpart to `union`,
  which now delegates to it — so whether two intervals combine, and into what,
  has one definition. Completes the trio with `intersection`: what two
  intervals share, what they cover together, and the unconditional answer.
- `TimeIntervalSequence::transform`, the temporal transformation `A[alpha,
  beta]`: `alpha` shifts every span's lower bound and `beta` its upper, turning
  `[s, e)` into `[s + alpha, e + beta)`. Because the two bounds move
  independently, widening a timeline can merge spans the shift brings together
  — including a gap closed to exactly zero — and narrowing it drops spans no
  wider than the shrinkage, so the result is re-normalized rather than merely
  re-bounded. Only the difference `alpha - beta` decides which of the two
  happens, so a call can drop spans or merge them, never both. Shifting every
  bound by the same amount preserves their order, which keeps it to a single
  pass with no sorting.
- `IntervalError::BoundOverflow`, reported when a `transform` shift pushes a
  bound outside the timestamp range. It is the one piece of arithmetic in the
  crate driven by caller-supplied numbers rather than by bounds that already
  exist, so it returns an error instead of panicking, and it transforms nothing
  when it does. The enum is `#[non_exhaustive]`, so the new variant does not
  break exhaustive matches.
- `sequence_system`, holding `SequenceSystem`: the sequences describing one
  subject, side by side, the way a sensor system holds its sensors — one
  sequence per attribute. Point sequences of any value type sit next to
  timelines, so `f64` readings, `String` log lines and an uptime state can
  belong to one system. Each sequence is filed under its own attribute, so the
  attribute it is found by is always the one it carries — a derived timeline
  is filed under its derivation, `up ∪ busy` for instance, and a transformed
  one under `up[1, 2]`. Sequences are kept sorted by attribute, and two systems
  are equal when they hold equal sequences for the same attributes.
- `SequenceSystem::insert` (returning whatever sequence the attribute held
  before), `remove`, `clear`, `get`, `contains`, `len`, `is_empty`,
  `attributes`, `iter`, and typed reads `points::<T>` and `intervals`.
  `FromIterator`, `Extend` and `IntoIterator` (owned and borrowed) are
  implemented; of two sequences for one attribute, the later is kept.
- Recording, for events arriving as a stream: `record_point` and `record_span`
  add one event to an existing sequence in place, and `record_batch` records
  a `Batch` — events for many attributes, with one value type per push — all
  or nothing. Every push in a batch is checked before anything is recorded,
  and each then goes in through its sequence's `Extend`, so a batch of
  in-order events costs no sorting. Recording never creates a sequence, so a
  misspelt attribute is an error rather than a new sequence, and the system
  never lends a sequence out mutably, so no sequence can change attribute
  while filed under its old one.
- `RecordError`, the failure recording returns: an unknown attribute, the
  other shape of sequence, or another value type, naming both types. It
  implements `Display` and `std::error::Error`, and is `#[non_exhaustive]`.
- `Sequence`, a member of either shape, with `attribute` (for a timeline, its
  transformation included), `label`, `len`, `is_empty`, `as_time_point::<T>`
  and `as_time_interval`.
- `AnyPointSequence`, a point sequence whose value type is known only at run
  time, recovered with `downcast_ref` or `downcast`, and `PointValue`, the
  bounds a value type needs to join a system — `Clone`, `Debug`, `PartialEq`,
  `Send`, `Sync` and `'static`, implemented automatically for every type that
  meets them. Systems are therefore `Send` and `Sync`.
- `SequenceSystem`, `Sequence`, `AnyPointSequence`, `Batch`, `RecordError` and
  `PointValue` are re-exported at the crate root.

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
