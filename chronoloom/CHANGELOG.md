# Unreleased

## Added

- `Extent`, the instants a sequence was observed over, as normalized spans.
  Every `TimeIntervalSequence` and `TimePointSequence` carries one, defaulting
  to everything, so existing behaviour is unchanged. `observed()`,
  `is_observed(t)`, `with_observed(extent)` and `observe(&extent)` read, set and
  narrow it, clipping the data to it. Inserting an event outside the extent
  grows it. Equality includes the extent; `covers_same` does not.
- The set operations and `transform` of `TimeIntervalSequence` follow
  three-valued logic: a result is known wherever it can be decided from what is
  known, and its extent says where.
- `TimeIntervalSequence::complement`, the observed instants that are not
  active, and `Attribute::Complement` for its label `¬A`.
- `slice(range)` on `TimePointSequence`, `TimeIntervalSequence`, `Sequence` and
  `SequenceSystem`: the data and extent inside the range, with labels,
  metadata and the output a derived member came from kept. A reversed range gives an empty, unobserved result.
- `Key`, the identity of a member of a `SequenceSystem`, kept apart from its
  attribute (its description). Two members may share an attribute under
  different keys, and `SequenceSystem::relabel` replaces a member's attribute
  in place, keeping its key and data. `SequenceSystem::observe` narrows every
  member's extent at once.
- Opaque `Metadata` (string maps) on a `SequenceSystem` and on each member:
  `metadata`, `metadata_mut`, `member_metadata` and `member_metadata_mut`.
  Inserting starts a member with empty metadata, replacing or removing drops it,
  and equality includes it.
- Live members of a `SequenceSystem`. `insert_live` files a member with a
  frontier, before which its data is final, and recording an event before it is
  `RecordError::Late`, with nothing recorded. `advance` moves the frontier and
  marks what it passed over as observed, `advance_unobserved` moves it without
  observing (except the instants of data already recorded there), and
  `advance_all` advances every live member. `frontier_of` and `frontier` read
  it, and equality ignores it. `RecordError::NotLive` reports advancing a batch
  member. `SequenceSystem::drain_finalised` hands over the final part of every
  live member by its own frontier, the events before it and the spans ending
  before it, whole, as a system of batch members with the same keys and
  metadata; the system keeps the rest, so a live member holds only its open
  spans and its non-final events.
- The `processing` module, re-exported at the crate root. A `Formula` is data
  that describes a state (`State`, `Between`, `While`, `Occurs`, `Not`, `And`,
  `Or`, `Shift`), read from a `SequenceSystem` through `Pattern`s, a key and a
  `Condition` on the values of that member, and evaluated three-valued with an
  extent. A `Shift` never fails, keeping bounds at the ends of time and
  saturating the others. `Between` takes an `OpenEnd` (`Extent` or `Head`) for a
  span that no end event closes. `Formula::hold(pattern, ticks)` is `Occurs`
  held for `ticks` ticks, as a `Shift` rather than a variant, and panics unless
  `ticks` is at least one. `Formula::conjunctive_normal_form` rewrites a formula
  as an `And` of `Or`s of literals denoting the same timeline. `Events`
  (`Edges`, `Matching`) are the formulas whose result is points, with `Edge`
  (`Start`, `End`).
- `Processor`, one recipe: `Processor::new(output, formula)` files an `Output`
  under the key of the one output it produces, evaluated by a `StreamSystem`.
  A formula may name the key of another processor's output, and a formula
  reading its own output is `ProcessError::Cycle { keys }`, reported by
  `Processor::check`.
- Step values, with no new sequence shape: a step is a point sequence of its
  changes read with sample and hold. `Formula::while_within(pattern, timeout)`
  is a `While` whose sample stops holding `timeout` ticks after it was taken,
  unknown from then until the next sample (it panics unless `timeout` is
  positive). `Events::changes(key)` is the points of a member of any value type
  with each repeat dropped, keeping the first sample of each run of the extent.
- One timeline per entity, as processor outputs rather than a new sequence type.
  `Group::new(key, path)` names the member of `Value` records and the field that
  holds the entity. `Output::per_entity` files the formula of each entity under
  `<output key>/<entity>`, with the patterns over the member of the group
  restricted to the entity's records and the others shared;
  `Output::any_entity` is the union of those timelines; and
  `Output::count_entities` is a `TimePointSequence<i64>` of the number of
  entities active, as its changes. An entity exists, and is observed, from its
  first record. `StreamSystem::entity` gives the entity of a member, and
  `ProcessError::DuplicateMember`,
  `SeveralMembers` and `GroupedRead` report two members under one key, an
  `Output::evaluate` of a per-entity output, and a live processor asked to read
  a member of one.
- `LiveProcessor`, from `LiveProcessor::new(&processors, from)`: it runs the
  processors online, driven by its host, with no clock, input or output of its
  own. `on_point` and `on_span` record input events (a late one is
  `ProcessError::Late`), `advance(key, watermark)` returns the `Emission`s that became final, and
  `finish(horizon)` ends the stream (`ProcessError::Finished` after that). A
  state output emits `Emitted::Edge`s once their instant is below the frontier
  of the output, which follows the lag of each node (a set operation takes the
  smaller frontier, `Shift` adds `min(alpha, beta)`, a chained output has that
  of the output it reads). `Events::Matching`, `Events::Changes` and
  `Events::Project` emit `Emitted::Point`s with the value erased as `AnyValue`,
  and a per-entity output emits under its member keys, counts as points of
  `i64`. What is emitted equals the batch evaluation over the whole stream, below
  each frontier and in the end, whatever order and watermarks the inputs arrive
  with. `OpenEnd::Head` is the frontier of the inputs online, and unknown
  regions emit nothing. Each advance evaluates the outputs again over a
  retained window, cut to what the part not yet emitted needs; `frontier(output)`
  and `retained()` read the output frontier and the size of that window. The
  state of a per-entity output grows with the entities seen.
- Record values. `Value` (`Null`, `Bool`, `Integer`, `Float`, `Text`, `Record`)
  is a dynamic payload for a `TimePointSequence<Value>`, beside the sequences of
  one fixed value type: values of one kind compare naturally, an integer and a
  float compare as numbers, any other pair is unordered (so a `Condition<Value>`
  works unchanged), `get` reaches a field by a dotted path, and typed accessors,
  `From` conversions and a JSON-like `Display` build and read it. A `Schema`
  (`Any`, `Null`, `Bool`, `Integer`, `Float`, `Text`, `Record`, `Optional`; a
  record rejects unknown fields) is filed on the system with
  `SequenceSystem::set_schema` and read with `schema`; `check` says where a
  value differs through a `SchemaError`. Recording a value without the shape is
  `RecordError::SchemaMismatch`, in an update too, which stays all or nothing.
- `Events::Project { key, path, into: Kind }` reads a field of each record into
  a point sequence of the type the `Kind` names (`Bool`, `Integer`, `Float`,
  `Text`, `Value`), skipping records without the field.
  `ProcessError::ProjectionMismatch` and `ProcessError::SchemaMismatch` report a
  field of another kind and a derived member that no longer has its schema.
- `Pattern::field(key, path, condition)`, a condition on one field of the record
  `Value`s of a member, read by a dotted path. It works in `Between`, `While`,
  `Occurs`, `hold` and `Events::Matching`, batch and online. A record without
  the field does not match, a `Null` field is a value like another, and a
  member that does not hold `Value` points is `ProcessError::ValueTypeMismatch`.
- Declared inputs and full validation. `Input` says what a `Processor` expects
  of the member under a key (a timeline, points of a value type, edges, or
  records of a `Schema`), declared with `Processor::with_input` and read with
  `inputs()`; a processor without declarations behaves as before.
  `Processor::validate` lists every problem without a system, in a stable order,
  with `ProcessError::{InOutput, InputIsOutput, MissingField, FieldMismatch}`,
  and a `StreamSystem` and a `LiveProcessor` hold the streams to the
  declarations.
- The optional `serde` feature (the only dependency, off by default, which
  leaves the default build with none): `Serialize` and `Deserialize` for
  sequences, extents, values, schemas, formulas, processors, systems and
  `LiveProcessor`, with the `chronoloom::serialisation` module documenting the
  format. A `TimePointSequence`, a `TimeIntervalSequence`, a `SequenceSystem`, a
  `Processor` and a `LiveProcessor` are documents that start with `"format": 1`
  (`FORMAT_VERSION`), and a reader refuses any other number. Loading validates:
  spans and extents are normalized, a span that ends before it starts is
  refused, events are sorted, data outside the extent is refused (a point at
  `i64::MAX` and the data of a live member excepted), a schema must fit its
  member's data, lineage (the output a derived member came from), entity and
  frontier must agree, and a processor must pass `Processor::check`. A `Value` is plain JSON and keeps `5` and `5.0`
  apart. The point members of a system and the patterns of a processor are
  tagged with one of `f64`, `i64`, `bool`, `text`, `value` and `edge`; any other
  type is an error naming the key. The processor document has an `inputs` array,
  left out when empty. `make lint` and `make test` also run with
  `--all-features`, and `make lint` asserts that the default build has no
  dependencies.
- Checkpoints of a live processor, behind the `serde` feature: the document of a
  `LiveProcessor` holds its processor, the retained window of its inputs (with
  the placeholder inputs that no event has typed yet), where each output has
  emitted up to and is trusted from, the entities seen and the points of each
  count, and recomputes the rest on load. A restored processor emits, and
  reports `frontier` and `retained`, exactly as one that never stopped. A
  document is validated on load (inputs against the processor, views against
  their entities, cursors against frontiers); an input of a value type outside
  the closed set cannot be saved, and the error names its key.
  `LiveProcessor::processors` reads the processors a live one was built from.
- The optional `json-schema` feature (implies `serde`; adds `schemars` and
  `serde_json`): `serialisation::processor_json_schema()` returns the JSON
  Schema of the processor document.
- `StreamSystem`, in module `stream_system`: the streams of a subject and a
  list of `processor::Processor`s, each one recipe (the inputs it reads, a
  formula over them, one output), whose derived streams are read-only and
  follow every change. `update(Update)` records into the streams, all or
  nothing, and evaluates again only the processors that read what changed,
  directly or through another output, returning a `Report` of the derived
  streams that changed. `insert_stream`, `remove_stream`, `add_processor` and
  `remove_processor` refuse what would break that (`UpdateError`). The mode is
  a type parameter; batch mode is the first.

## Removed

- `sequence_system::Batch`, the recording buffer: it is now `stream_system::Update`
  (also at the crate root), and `SequenceSystem::record_batch` is
  `record_update`. `Batch` at the root now names the batch mode of a
  `StreamSystem`.
- Filing a member under its own label: `SequenceSystem::insert` now takes the key
  as its first argument, `insert(key, sequence)`. `FromIterator` and `Extend`
  take `(key, sequence)` pairs, and `Update::points` and `Update::spans` take a
  key.
- `SequenceSystem::attributes()`, replaced by `keys()`.
- The `btree_map` iterators of `SequenceSystem`: `iter()` and `into_iter()` now
  return the named `sequence_system::Iter` and `IntoIter`, and yield `Key`
  instead of `String`.
- The `attribute` field of every `RecordError` variant, replaced by `key`.

# 0.3.1 - 2026-10-22

## Added

- `TimePointEvent` and `TimeIntervalEvent` are `#[repr(C)]`, and their layout
  is now part of the public API: timestamp then value, and start, end, then
  value. A sequence's memory can therefore be read in place from outside Rust,
  which is what lets the Python bindings hand out NumPy views of a sequence
  rather than copies.

## Fixed

- The README no longer describes `chronoloompy` as a pydantic model layer; it
  is now an API over this crate.

## Removed

- The `[dependencies]` snippet from the README. It pinned a version by hand and
  went stale with each release; `cargo add chronoloom` picks the current one.

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
