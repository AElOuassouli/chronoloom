# chronoloom

Set algebra over time points and intervals — a small, dependency-free Rust
library.

Intervals are **half-open**: `start` is included, `end` is excluded. Two
intervals that merely touch (`[0, 5)` and `[5, 9)`) therefore share no instant.

## Primitives

Two event shapes make up the vocabulary, both generic over the value they carry
— a measurement, a label, a set of tags, or nothing at all (`()`):

- `TimePointEvent` anchors a value to a single instant, with no duration.
- `TimeIntervalEvent` attaches a value to a span of time. Construction is
  validated, so `end` is always strictly after `start` and a span is never
  empty.

```rust
use chronoloom::{TimeIntervalEvent, TimePointEvent};

let reading = TimePointEvent::new(1_700_000_000, 21.5_f64);
assert_eq!(reading.timestamp(), 1_700_000_000);

let phase = TimeIntervalEvent::new(0, 60, "warm-up").unwrap();
assert_eq!(phase.duration(), 60);

assert!(TimeIntervalEvent::span(5, 5).is_err());
```

## Sequences

`TimePointSequence` collects point events and keeps them in time order however
they arrive. Events sit contiguously in one `Vec`, sorted by timestamp, so
lookups and windows binary-search that maintained order rather than scanning,
and a window comes back as a real slice. Adding an event that belongs at the
end — the usual case for events arriving in time order — costs no shifting at
all. Several events may share an instant, where they keep the order they were
added.

```rust
use chronoloom::{TimePointEvent, TimePointSequence};

let readings = TimePointSequence::from_events("readings", vec![
    TimePointEvent::new(30, 3.0),
    TimePointEvent::new(10, 1.0),
    TimePointEvent::new(20, 2.0),
]);

// Ordered however the events arrived.
let seen: Vec<i64> = readings.iter().map(|e| e.timestamp()).collect();
assert_eq!(seen, [10, 20, 30]);

// Any Rust range works, and a window is a slice of the sequence itself.
let window: &[TimePointEvent<f64>] = readings.range(10..30);
assert_eq!(window.len(), 2);

// The closest event in either direction, when the exact instant is missing.
assert_eq!(readings.nearest(28).map(|e| e.timestamp()), Some(30));
```

`before` and `after` answer the same question one-sidedly, and both bounds are
inclusive — an event landing exactly on the instant is the answer.

`TimeIntervalSequence` is the other shape: **one state over time**, as the spans
during which it was active. Every span means the same thing, so there is no
per-span value. Overlapping and touching spans merge as they arrive, keeping the
timeline a canonical, disjoint description of which instants are covered.

```rust
use chronoloom::{TimeIntervalEvent, TimeIntervalSequence};

let mut uptime = TimeIntervalSequence::new("uptime");
uptime.insert(TimeIntervalEvent::span(0, 5).unwrap());
uptime.insert(TimeIntervalEvent::span(20, 30).unwrap());

// Touching [0, 5) leaves no instant between them, so they merge.
uptime.insert(TimeIntervalEvent::span(5, 9).unwrap());
assert_eq!(uptime[0].bounds(), (0, 9));

// A span reaching across the gap swallows both.
uptime.insert(TimeIntervalEvent::span(7, 25).unwrap());
assert_eq!(uptime.len(), 1);
assert!(uptime.contains(12));
```

Because the timeline is normalized, `len` counts the spans left after merging —
not the number inserted — and two timelines of the same state are equal exactly
when they cover the same instants, however they were built. `covers_same` asks
about the coverage alone, for timelines that describe themselves differently.

Two timelines combine with the usual set algebra. Both operands are borrowed and
left untouched; each operation returns a new sequence. Since both are already
ordered, each is a single pass over the two — linear, with no sorting.

```rust
use chronoloom::{TimeIntervalEvent, TimeIntervalSequence};

let up = TimeIntervalSequence::from_spans("up", vec![
    TimeIntervalEvent::span(0, 100).unwrap(),
]);
let maintenance = TimeIntervalSequence::from_spans("maintenance", vec![
    TimeIntervalEvent::span(10, 20).unwrap(),
    TimeIntervalEvent::span(30, 40).unwrap(),
]);

// Up, but not in maintenance.
let serving = up.difference(&maintenance);
let bounds: Vec<(i64, i64)> = serving.iter().map(|s| s.bounds()).collect();
assert_eq!(bounds, [(0, 10), (20, 30), (40, 100)]);

// Everything in maintenance happened while up — the same instants, under a
// description the operation derived.
let shared = up.intersection(&maintenance);
assert!(shared.covers_same(&maintenance));
assert_eq!(shared.label(), "up ∩ maintenance");

// Both operands are untouched and still usable.
assert!(up.union(&maintenance).covers_same(&up));
```

`symmetric_difference` is the XOR: the instants covered by exactly one of the
two timelines.

## What was observed

A timeline with no span after `t` reads as "inactive after `t`", whether the
data stopped, the sensor was offline, or it has not arrived yet. Every sequence
therefore carries an `Extent`, the instants it was observed over, which starts
as everything. Inside it, no span means inactive; outside it, nothing is known.
`contains` keeps asking whether an instant is active, and `is_observed` tells
inactive from unknown.

Operations keep a result wherever it can be decided: a union is active where
either side is known active, even where the other was not observed. `complement`
is the observed instants that are not active, `¬A`.

```rust
use chronoloom::{Extent, TimeIntervalEvent, TimeIntervalSequence};

let span = |start, end| TimeIntervalEvent::span(start, end).unwrap();
let seen = |start, end| Extent::from_spans(vec![span(start, end)]);

// Observed over [0, 10) only: what happens after that is unknown.
let up = TimeIntervalSequence::from_spans("up", vec![span(2, 5)])
    .with_observed(seen(0, 10));
assert!(!up.contains(7) && up.is_observed(7));
assert!(!up.contains(20) && !up.is_observed(20));

let down = up.complement();
assert!(down.contains(7) && !down.contains(3));

// Another sensor watched [20, 30) and saw activity there. The union is active
// there, and unknown in the gap between the two.
let other = TimeIntervalSequence::from_spans("other", vec![span(22, 25)])
    .with_observed(seen(20, 30));
let either = up.union(&other);
assert!(either.contains(23) && !either.is_observed(15));

// An event outside the extent grows it, since it proves the instant was seen.
let mut up = up;
up.insert(span(40, 45));
assert!(up.is_observed(42));
```

`with_observed` sets the extent and cuts the data to it, and
`SequenceSystem::observe` narrows every member at once. Equality includes the
extent; `covers_same` does not.

## Moving a timeline in time

`transform` shifts each bound of every span independently: `alpha` moves the
lower bound, `beta` the upper, so `A[alpha, beta]` turns every span `[s, e)`
into `[s + alpha, e + beta)`. Widening a timeline asks "was this active *near*
here"; narrowing it asks "was this active for a solid stretch". Spans that meet
after widening are merged, and spans narrowed away to nothing disappear.

```rust
use chronoloom::{TimeIntervalEvent, TimeIntervalSequence};

let alerts = TimeIntervalSequence::from_spans("alerts", vec![
    TimeIntervalEvent::span(0, 10).unwrap(),
    TimeIntervalEvent::span(14, 20).unwrap(),
]);

// Two ticks of slack on each side close the four-tick gap exactly.
let within_two = alerts.transform(-2, 2).unwrap();
let bounds: Vec<(i64, i64)> = within_two.iter().map(|s| s.bounds()).collect();
assert_eq!(bounds, [(-2, 22)]);

// Only stretches longer than six ticks have anything left after six ticks of
// lead-in, and the six-tick alert is gone entirely.
let sustained = alerts.transform(6, 0).unwrap();
let bounds: Vec<(i64, i64)> = sustained.iter().map(|s| s.bounds()).collect();
assert_eq!(bounds, [(6, 10)]);
```

Only the difference `alpha - beta` decides what happens structurally, so a call
either narrows spans and widens gaps, or the reverse — it can drop spans or
merge them, never both. It is a single pass, and the one operation that can fail:
a shift that pushes a bound outside the timestamp range returns
`IntervalError::BoundOverflow`.

## Operations on a pair of intervals

Two `TimeIntervalEvent`s can be intersected — the span where both are active —
or united — the spans covered by either, merged when nothing separates them.

```rust
use chronoloom::TimeIntervalEvent;

let a = TimeIntervalEvent::new(0, 5, "a").unwrap();
let b = TimeIntervalEvent::new(5, 9, "b").unwrap();

// Half-open, so touching intervals share no instant.
assert_eq!(a.intersection(&b), None);

// But together they cover exactly [0, 9), so they merge.
assert_eq!(a.union(&b), vec![TimeIntervalEvent::span(0, 9).unwrap()]);

// A real gap keeps them apart, always ordered by start.
let far = TimeIntervalEvent::new(20, 30, "c").unwrap();
assert_eq!(a.union(&far).len(), 2);
```

Both work on the time dimension only: they accept intervals carrying different
kinds of value, consume neither, and return valueless spans. Reattach a value
with `map` if the result needs to mean something.

## Sequence systems

A `SequenceSystem` holds the sequences describing one subject side by side, the
way a sensor system holds its sensors: point sequences of any value type next to
timelines, one sequence per key. Events are recorded into it as they
arrive — one at a time, or a `Batch` at a time, all or nothing. The
Datasets section below adds metadata and `slice`.

```rust
use chronoloom::{Batch, SequenceSystem, TimeIntervalEvent, TimeIntervalSequence};
use chronoloom::{TimePointEvent, TimePointSequence};

// Declare each sensor once, under the key that identifies it.
let mut machine = SequenceSystem::new();
machine.insert("temperature", TimePointSequence::<f64>::new("temperature"));
machine.insert("log", TimePointSequence::<String>::new("log"));
machine.insert("up", TimeIntervalSequence::new("up"));

machine.record_point("temperature", TimePointEvent::new(10, 21.5_f64)).unwrap();

let mut tick = Batch::new();
tick.points("temperature", [TimePointEvent::new(11, 21.7_f64)])
    .points("log", [TimePointEvent::new(11, String::from("fan on"))])
    .spans("up", [TimeIntervalEvent::span(0, 20).unwrap()]);
machine.record_batch(tick).unwrap();

assert_eq!(machine.points::<f64>("temperature").map(|s| s.len()), Some(2));

// Recording never creates a sequence, so a misspelt key is an error.
assert!(machine.record_point("temprature", TimePointEvent::new(12, 0.0_f64)).is_err());

// The key is the identity; the attribute is only a description, and can change.
machine.relabel("up", "uptime").unwrap();
assert_eq!(machine.intervals("up").map(|s| s.label()), Some(String::from("uptime")));
```

## Processors

Sensors report transitions as point events, or values that cross a threshold.
Turning them into a timeline does not happen on the sequence types, which hold
data, extent and the set algebra and nothing else. It goes through a
`Processor`, a set of formulas, each filed under the key of the member it
produces. A `Formula` is data: it can be compared, printed, evaluated against
any system and later stored. It reads points through a `Pattern`, a key and a
`Condition` on the values (`Equals`, `Greater`, `Within`, `In`, `And`, `Not` and
so on) over the value type of that member.

`Between` is active from a point matching `start` until the next point matching
`end`. A start while active and an end while inactive do nothing, so a stream
with orphan ends is no error, and an end at the instant a span opened lasts one
tick.

```rust
use chronoloom::{Condition, Formula, Pattern, Processor, SequenceSystem};
use chronoloom::{TimePointEvent, TimePointSequence};

let mut room = SequenceSystem::new();
room.insert("motion", TimePointSequence::from_events("motion", vec![
    TimePointEvent::new(10, "motion_start"),
    TimePointEvent::new(25, "motion_end"),
]));

let present = Formula::between(
    Pattern::new("motion", Condition::Equals("motion_start")),
    Pattern::new("motion", Condition::Equals("motion_end")),
);
let processor = Processor::new().with("present", present);

// `run` makes a system of the outputs alone; `apply` adds them to `room`.
let derived = processor.run(&room).unwrap();
assert_eq!(derived.intervals("present").unwrap()[0].bounds(), (10, 25));
processor.apply(&mut room).unwrap();
assert!(room.contains("present"));
```

`While` holds the last sample: a threshold is `Greater`, and a range is `Within`,
which is half-open like a span. The formula is unknown before the first sample
of a run of the extent, and the last sample holds to the end of the run.

```rust
use chronoloom::{Condition, Formula, Pattern, Processor, SequenceSystem};
use chronoloom::{TimePointEvent, TimePointSequence};

let mut room = SequenceSystem::new();
room.insert("temperature", TimePointSequence::from_events("temperature", vec![
    TimePointEvent::new(0, 3.0),
    TimePointEvent::new(10, 7.0),
    TimePointEvent::new(20, 9.0),
    TimePointEvent::new(30, 4.0),
]));

let hot = Formula::while_matching(Pattern::new("temperature", Condition::Greater(5.0)));
let mild = Formula::while_matching(Pattern::new(
    "temperature",
    Condition::Within { low: 5.0, high: 8.0 },
));
let derived = Processor::new()
    .with("hot", hot)
    .with("mild", mild)
    .run(&room)
    .unwrap();

assert_eq!(derived.intervals("hot").unwrap()[0].bounds(), (10, 30));
assert_eq!(derived.intervals("mild").unwrap()[0].bounds(), (10, 20));
// Nothing is known before the first sample.
assert!(!derived.intervals("hot").unwrap().is_observed(-1));
```

A span with no end is active as long as no end event is observed, and
`Between` carries an `OpenEnd` that says where it stops: `OpenEnd::Extent`, the
default, at the end of the extent, and `OpenEnd::Head` at the instant up to which
the inputs have been seen, with everything unknown from there on. `Occurs` is
active for one tick at each matching point. `Not`, `And`, `Or` and `Shift` mean
exactly what `complement`, `intersection`, `union` and `transform` mean,
three-valued extents included, except that a `Shift` never fails: a span that
reaches the end of time keeps reaching it, and other bounds saturate. `conjunctive_normal_form` rewrites a formula as an `And` of
`Or`s of literals that denotes the same timeline, and `Events::Edges` and
`Events::Matching` are the outputs that are points rather than a timeline.

## Datasets

A `SequenceSystem` with the pieces above describes a dataset: the recordings of
one subject over a stretch of time. Each piece answers one question.

- **Keys** say which sequence is which. The key is the identity; the label is
  only a description.
- **The observed extent** says what was seen. A dataset that stopped early is
  the system narrowed with `observe`: every member ends at the same instant, and
  after it `contains` and `is_observed` are both false.
- **Metadata** says what the data means to you: a time unit, an origin, a unit
  per sensor. `Metadata` is a string map, kept on the system and on each member.
  chronoloom stores it and never reads it.
- **A `Processor`** turns raw readings into timelines, so one dataset holds the
  readings and the states derived from them.
- **`slice`** cuts every member to a window of time, keeping keys, labels and
  metadata, and narrowing each extent to the window.

```rust
use chronoloom::{Condition, Extent, Formula, Pattern, Processor, SequenceSystem};
use chronoloom::{TimeIntervalEvent, TimePointEvent, TimePointSequence};

let motion = TimePointSequence::from_events("motion", vec![
    TimePointEvent::new(10, "motion_start"),
    TimePointEvent::new(40, "motion_end"),
]);

let mut room = SequenceSystem::new();
room.insert("motion", motion);
Processor::new()
    .with("presence", Formula::between(
        Pattern::new("motion", Condition::Equals("motion_start")),
        Pattern::new("motion", Condition::Equals("motion_end")),
    ))
    .apply(&mut room)
    .unwrap();
room.metadata_mut().insert("unit".into(), "seconds".into());
room.member_metadata_mut("motion")
    .unwrap()
    .insert("placement".into(), "door".into());

// The recording stopped at 50: nothing is known after it.
room.observe(&Extent::from_spans(vec![TimeIntervalEvent::span(i64::MIN, 50).unwrap()]));
assert!(!room.intervals("presence").unwrap().is_observed(60));

// A playback window keeps keys, labels and metadata.
let window = room.slice(20..45);
let presence = window.intervals("presence").unwrap();
assert_eq!(presence[0].bounds(), (20, 40));
assert!(presence.is_observed(44) && !presence.is_observed(45));
assert_eq!(window.points::<&str>("motion").map(|s| s.len()), Some(1));
assert_eq!(window.metadata().get("unit").map(String::as_str), Some("seconds"));
assert_eq!(
    window.member_metadata("motion").and_then(|m| m.get("placement")).map(String::as_str),
    Some("door"),
);
```

## Not here yet

Two directions come next, and neither exists today:

- **Online processing**: feeding events in as they arrive, with a frontier that
  says how far the data is final and operators that update their results
  incrementally.
- **Record values**: sequences whose events carry structured values, beside the
  single value type each sequence has now.

Beyond those, the wider algebra is still open: window and containment queries
over a sequence, coverage and gaps, and the same set algebra for
`TimePointSequence`. What exists today is the vocabulary, the two sequence
shapes, the pair-wise interval operations, the set algebra between two
timelines (complement and observed extent included), formulas and processors,
and systems that hold sequences of both shapes together as datasets.

Every code block in this file is compiled and run by `make test`, so what is
documented here is what is implemented.

## Python

Python bindings are published separately as
[`chronoloompy`](https://pypi.org/project/chronoloompy/), an API over this
crate that keeps data in Rust memory and hands Python zero-copy NumPy views of
it. It lives in
[`../chronoloompy`](../chronoloompy) in this repository.

## Development

```sh
make lint   # cargo fmt --check, clippy, and the "no pyo3" assertion
make test   # unit and doc tests
```

This crate must never depend on pyo3 — that is what makes it usable from any
Rust project. `make lint` enforces it.

## License

MIT — see [LICENSE](LICENSE).
