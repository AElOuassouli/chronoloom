# chronoloom

Set algebra over time points and intervals, in a small Rust library with no
dependencies unless you ask for the optional `serde` feature. Events become
timelines, timelines combine with set operations, and the same formulas run on
a finished recording or online, as the data arrives.

What it holds today: two event shapes and two sequence types, one for points
and one for timelines; the instants a sequence was observed over, so that
"inactive" and "unknown" differ; a `SequenceSystem` that files sequences under
keys as one dataset; a `Processor`, a set of formulas that turns points into
timelines and timelines into edges; a `LiveProcessor` that produces the same
outputs as the data arrives; dynamic record values with a schema; and one
timeline per entity of a record field. Behind features, systems, processors and
live processors save and load, and a processor has a JSON Schema.

Intervals are **half-open**: `start` is included, `end` is excluded. Two
intervals that merely touch (`[0, 5)` and `[5, 9)`) therefore share no instant.

The document goes from the parts to the whole: events and sequences, the
algebra on timelines, what was observed, systems that hold them together, the
processors that derive one from another, the same processors run online, and
saving and loading.

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

## Sequence systems

A `SequenceSystem` holds the sequences describing one subject side by side, the
way a sensor system holds its sensors: point sequences of any value type next to
timelines, one sequence per key. Events are recorded into it as they
arrive — one at a time, or an `Update` at a time, all or nothing. A member can
also be a stream (next section), and the Datasets section adds metadata and
`slice`.

```rust
use chronoloom::{SequenceSystem, TimeIntervalEvent, TimeIntervalSequence, Update};
use chronoloom::{TimePointEvent, TimePointSequence};

// Declare each sensor once, under the key that identifies it.
let mut machine = SequenceSystem::new();
machine.insert("temperature", TimePointSequence::<f64>::new("temperature"));
machine.insert("log", TimePointSequence::<String>::new("log"));
machine.insert("up", TimeIntervalSequence::new("up"));

machine.record_point("temperature", TimePointEvent::new(10, 21.5_f64)).unwrap();

let mut tick = Update::new();
tick.points("temperature", [TimePointEvent::new(11, 21.7_f64)])
    .points("log", [TimePointEvent::new(11, String::from("fan on"))])
    .spans("up", [TimeIntervalEvent::span(0, 20).unwrap()]);
machine.record_update(tick).unwrap();

assert_eq!(machine.points::<f64>("temperature").map(|s| s.len()), Some(2));

// Recording never creates a sequence, so a misspelt key is an error.
assert!(machine.record_point("temprature", TimePointEvent::new(12, 0.0_f64)).is_err());

// The key is the identity; the attribute is only a description, and can change.
machine.relabel("up", "uptime").unwrap();
assert_eq!(machine.intervals("up").map(|s| s.label()), Some(String::from("uptime")));
```

## Streaming into a system

A member inserted with `insert_live` is a stream. It has a frontier: data
before it is final, so an event stamped before it is late, and recording it is
`RecordError::Late`, with nothing changed. `advance` moves the frontier forward
and marks what it passed over as observed; `advance_unobserved` moves it
without observing, which leaves a gap for an outage (data already recorded
there still counts as seen). The frontier is the state of the stream, so it
lives on the system, not on the sequence, and equality ignores it.

```rust
use chronoloom::{RecordError, SequenceSystem, TimePointEvent, TimePointSequence};

let mut room = SequenceSystem::new();
room.insert_live("door", TimePointSequence::<&str>::new("door"), 0);

room.record_point("door", TimePointEvent::new(5, "open")).unwrap();
room.advance("door", 10).unwrap();
assert_eq!(room.frontier_of("door"), Some(10));

// Before the frontier is final: a late event is refused.
let late = room.record_point("door", TimePointEvent::new(3, "close"));
assert!(matches!(late, Err(RecordError::Late { .. })));

// What the frontier passed over is observed; beyond it, nothing is known yet.
let door = room.points::<&str>("door").unwrap();
assert!(door.is_observed(7) && !door.is_observed(12));
```

A live member grows for as long as the stream runs, so `drain_finalised` hands
its final part over. It takes, from each live member by its own frontier, the
events before the frontier and the spans that end before it, whole, and returns
them as a system of batch members with the same keys and metadata. A span that
reaches the frontier stays, since it may still grow, so a live member holds at
most its open spans plus its non-final events. The extent is split where the
data is: the drained part carries what was observed before the earliest instant
the member keeps, and the member keeps the rest, so the two put back together
are what a batch system of the same events holds. A member with nothing final is
still in the result, empty but with its extent; batch members, derived ones
included, are never drained.

```rust
use chronoloom::{SequenceSystem, TimeIntervalEvent, TimeIntervalSequence};

let mut room = SequenceSystem::new();
room.insert_live("occupied", TimeIntervalSequence::new("occupied"), 0);

room.record_span("occupied", TimeIntervalEvent::span(2, 5).unwrap()).unwrap();
room.record_span("occupied", TimeIntervalEvent::span(8, 30).unwrap()).unwrap();
room.advance("occupied", 20).unwrap();

// The first span is final; the second reaches the frontier and stays whole.
let finalised = room.drain_finalised();
assert_eq!(finalised.intervals("occupied").unwrap().len(), 1);
assert_eq!(room.intervals("occupied").unwrap().len(), 1);
assert_eq!(room.frontier_of("occupied"), Some(20));
```

## Processors

Sensors report transitions as point events, or values that cross a threshold.
Turning them into a timeline does not happen on the sequence types, which hold
data, extent and the set algebra and nothing else. It goes through a
`Processor`, one recipe: a formula filed under the key of the one output it
produces, evaluated by a `StreamSystem` (see Stream systems). A `Formula` is
data: it can be compared, printed, evaluated against any stream system and
later stored. `Processor::evaluate` gives the output over a system once,
without storing it, and `add_processor` keeps it up to date. It reads points
through a `Pattern`, a key and a `Condition` on the values (`Equals`, `Greater`, `Within`, `In`, `And`, `Not` and
so on) over the value type of that member.

`Between` is active from a point matching `start` until the next point matching
`end`. A start while active and an end while inactive do nothing, so a stream
with orphan ends is no error, and an end at the instant a span opened lasts one
tick.

```rust
use chronoloom::{Processor, StreamSystem};
use chronoloom::{Condition, Formula, Pattern};
use chronoloom::{TimePointEvent, TimePointSequence};

let mut room = StreamSystem::new();
room.insert_stream("motion", TimePointSequence::from_events("motion", vec![
    TimePointEvent::new(10, "motion_start"),
    TimePointEvent::new(25, "motion_end"),
])).unwrap();

let present = Formula::between(
    Pattern::new("motion", Condition::Equals("motion_start")),
    Pattern::new("motion", Condition::Equals("motion_end")),
);

// The system evaluates the processor and keeps its output up to date.
room.add_processor(Processor::new("present", present)).unwrap();
assert_eq!(room.intervals("present").unwrap()[0].bounds(), (10, 25));
assert!(room.is_derived("present"));
```

A processor can also be evaluated once, over a system, without adding it:

```rust
use chronoloom::{Condition, Formula, Pattern, Processor, StreamSystem};
use chronoloom::{TimePointEvent, TimePointSequence};

let mut room = StreamSystem::new();
room.insert_stream("motion", TimePointSequence::from_events("motion", vec![
    TimePointEvent::new(10, "motion_start"),
    TimePointEvent::new(25, "motion_end"),
])).unwrap();

let present = Formula::between(
    Pattern::new("motion", Condition::Equals("motion_start")),
    Pattern::new("motion", Condition::Equals("motion_end")),
);

let sequence = Processor::new("present", present).evaluate(&room).unwrap();
assert_eq!(sequence.as_time_interval().unwrap()[0].bounds(), (10, 25));
assert!(!room.contains("present"));
```

`While` holds the last sample: a threshold is `Greater`, and a range is `Within`,
which is half-open like a span. The formula is unknown before the first sample
of a run of the extent, and the last sample holds to the end of the run.

```rust
use chronoloom::{Processor, StreamSystem};
use chronoloom::{Condition, Formula, Pattern};
use chronoloom::{TimePointEvent, TimePointSequence};

let mut room = StreamSystem::new();
room.insert_stream("temperature", TimePointSequence::from_events("temperature", vec![
    TimePointEvent::new(0, 3.0),
    TimePointEvent::new(10, 7.0),
    TimePointEvent::new(20, 9.0),
    TimePointEvent::new(30, 4.0),
])).unwrap();

let hot = Formula::while_matching(Pattern::new("temperature", Condition::Greater(5.0)));
let mild = Formula::while_matching(Pattern::new(
    "temperature",
    Condition::Within { low: 5.0, high: 8.0 },
));
room.add_processor(Processor::new("hot", hot)).unwrap();
room.add_processor(Processor::new("mild", mild)).unwrap();

assert_eq!(room.intervals("hot").unwrap()[0].bounds(), (10, 30));
assert_eq!(room.intervals("mild").unwrap()[0].bounds(), (10, 20));
// Nothing is known before the first sample.
assert!(!room.intervals("hot").unwrap().is_observed(-1));
```

A span with no end is active as long as no end event is observed, and
`Between` carries an `OpenEnd` that says where it stops: `OpenEnd::Extent`, the
default, at the end of the extent, and `OpenEnd::Head` at the instant up to which
the inputs have been seen, with everything unknown from there on. `Occurs` is
active for one tick at each matching point, and `Formula::hold(pattern, ticks)`
is active for `ticks` ticks from each one, holds that overlap or touch merging
(it is a `Shift` of `Occurs`, not a variant of its own, and `ticks` must be at
least one: it panics otherwise). `Not`, `And`, `Or` and `Shift` mean exactly
what `complement`, `intersection`, `union` and `transform` mean, three-valued
extents included, except that a `Shift` never fails: a span that reaches the
end of time keeps reaching it, and other bounds saturate.
`conjunctive_normal_form` rewrites a formula as an `And` of `Or`s of literals
that denotes the same timeline, and `Events::Edges` and `Events::Matching` are
the outputs that are points rather than a timeline.

```rust
use chronoloom::{Processor, StreamSystem};
use chronoloom::{Condition, Formula, Pattern};
use chronoloom::{TimePointEvent, TimePointSequence};

let mut room = StreamSystem::new();
room.insert_stream("door", TimePointSequence::from_events("door", vec![
    TimePointEvent::new(10, "open"),
    TimePointEvent::new(12, "open"),
    TimePointEvent::new(30, "open"),
])).unwrap();

// Recently opened: three ticks from each opening, merged where they overlap.
let recent = Formula::hold(Pattern::new("door", Condition::Equals("open")), 3);
room.add_processor(Processor::new("recent", recent)).unwrap();
let spans: Vec<(i64, i64)> = room.intervals("recent").unwrap()
    .iter().map(|span| span.bounds()).collect();
assert_eq!(spans, [(10, 15), (30, 33)]);
```

A formula may name the key of the output of another processor, and then reads
that output. A processor reads only what the system already holds, so the
processors are added in the order they depend on each other, and a formula
reading its own output is `ProcessError::Cycle`, which `Processor::check`
reports without a system.

```rust
use chronoloom::{Processor, StreamSystem};
use chronoloom::{Condition, Formula, Pattern};
use chronoloom::{TimeIntervalEvent, TimeIntervalSequence, TimePointEvent, TimePointSequence};

let mut room = StreamSystem::new();
room.insert_stream("motion", TimePointSequence::from_events("motion", vec![
    TimePointEvent::new(10, "motion_start"),
    TimePointEvent::new(25, "motion_end"),
])).unwrap();
room.insert_stream("door", TimeIntervalSequence::from_spans("door", vec![
    TimeIntervalEvent::span(40, 50).unwrap(),
])).unwrap();

room.add_processor(Processor::new("present", Formula::between(
    Pattern::new("motion", Condition::Equals("motion_start")),
    Pattern::new("motion", Condition::Equals("motion_end")),
))).unwrap();
// Reads the output `present`, which the system holds by now.
room.add_processor(Processor::new(
    "occupied",
    Formula::state("present").or(Formula::state("door")),
)).unwrap();
assert_eq!(room.intervals("occupied").unwrap().len(), 2);

// A formula reading its own output is a cycle.
let looping = Processor::new("loop", Formula::state("loop"));
assert!(looping.check().is_err());
```

### Step values

A value that holds until it changes (an occupancy, a counter) is not a third
sequence shape: it is a point sequence of its changes read with `While`, as
above. `Formula::while_within(pattern, timeout)` is for a sensor that may go
silent: a sample stops holding `timeout` ticks after it was taken, and until the
next one the formula is unknown, not inactive. `Events::changes(key)` is the
canonical form of a step, the points of a member of any value type with each
repeat dropped, for a host that stores or streams only the changes. Both work
online, with no lag.

```rust
use chronoloom::{Processor, StreamSystem};
use chronoloom::{Condition, Events, Formula, Pattern};
use chronoloom::{TimePointEvent, TimePointSequence};

let mut room = StreamSystem::new();
room.insert_stream("count", TimePointSequence::from_events("count", vec![
    TimePointEvent::new(0, 2_i64),
    TimePointEvent::new(10, 9),
    TimePointEvent::new(20, 9),
    TimePointEvent::new(100, 1),
])).unwrap();

// Busy while the last count is above five, trusted for 30 ticks after it.
let busy = Formula::while_within(Pattern::new("count", Condition::Greater(5_i64)), 30);
room.add_processor(Processor::new("busy", busy)).unwrap();
room.add_processor(Processor::new("steps", Events::changes("count"))).unwrap();

let busy = room.intervals("busy").unwrap();
assert_eq!(busy[0].bounds(), (10, 50));
// From 50 to 100 the sensor was silent: unknown, not inactive.
assert!(!busy.contains(70) && !busy.is_observed(70));

// The repeat at 20 is dropped.
let steps = room.points::<i64>("steps").unwrap();
let changes: Vec<(i64, i64)> = steps.iter().map(|e| (e.timestamp(), *e.value())).collect();
assert_eq!(changes, [(0, 2), (10, 9), (100, 1)]);
```

### Declared inputs

A processor may say what it expects of the members it reads, with
`Processor::with_input(key, Input)`: a timeline, points of a value type, edges,
or records of a `Schema`. `Processor::validate` then lists every problem without
a system, in a stable order (the cycle first, then the reads of the formula),
and a `StreamSystem` or a `LiveProcessor` refuses a stream that contradicts a
declaration. A processor with no declarations behaves as before.

```rust
use chronoloom::{Processor, StreamSystem};
use chronoloom::{Formula, Input, Kind, TimePointSequence};

let processor = Processor::new("open", Formula::state("door").or(Formula::state("shut")))
    .with_input("door", Input::Timeline)
    .with_input("temperature", Input::Points(Kind::Float));

// `shut` is neither the output nor a declared input.
assert_eq!(processor.validate().unwrap_err().len(), 1);

// A system whose `temperature` holds integers contradicts the declaration.
let mut system = StreamSystem::new();
system.insert_stream("temperature", TimePointSequence::<i64>::new("temperature")).unwrap();
assert!(system.add_processor(processor).is_err());
```

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
- **A `Processor`** turns raw readings into timelines, so one dataset (a
  `StreamSystem`) holds the readings and the states derived from them.
- **`slice`** cuts every member to a window of time, keeping keys, labels and
  metadata, and narrowing each extent to the window.

```rust
use chronoloom::{Processor, StreamSystem};
use chronoloom::{Condition, Extent, Formula, Pattern};
use chronoloom::{TimeIntervalEvent, TimePointEvent, TimePointSequence};

let motion = TimePointSequence::from_events("motion", vec![
    TimePointEvent::new(10, "motion_start"),
    TimePointEvent::new(40, "motion_end"),
]);

let mut room = StreamSystem::new();
room.insert_stream("motion", motion).unwrap();
room.add_processor(Processor::new("presence", Formula::between(
    Pattern::new("motion", Condition::Equals("motion_start")),
    Pattern::new("motion", Condition::Equals("motion_end")),
))).unwrap();
room.metadata_mut().insert("unit".into(), "seconds".into());
room.member_metadata_mut("motion")
    .unwrap()
    .insert("placement".into(), "door".into());

// The recording stopped at 50: nothing is known after it.
room.observe(&Extent::from_spans(vec![TimeIntervalEvent::span(i64::MIN, 50).unwrap()]))
    .unwrap();
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

## Running a processor online

`LiveProcessor::new(&processors, from)` makes a `LiveProcessor`, the same outputs
produced as the inputs arrive. It is a state machine with no clock, input or output, that a host
drives: `on_point` and `on_span` record input events, `advance` moves the
frontier (watermark) of one input, and each call returns the `Emission`s that
became final. A state output emits `Edge`s, a `Start` once its instant is below
the frontier of the output, so a start reaches the host before its span closes;
`Events::Matching`, `Events::Changes` and `Events::Project` emit points with
their values erased as `AnyValue`.

Every operator has a lag: the frontier of an output is a function of the
frontiers of its inputs. Set operations take the smaller one, `Shift { alpha,
beta }` adds `min(alpha, beta)`, and a chained output has the frontier of the
output it reads. Everything before the frontier of an output is final, so the
edges emitted are those of the batch evaluation over the whole stream, whatever order
and watermarks the inputs arrive with. `finish(horizon)` ends the stream, and
what was emitted then equals the batch result with the extent of each input
ending at the horizon.

```rust
use chronoloom::{Processor};
use chronoloom::{Condition, Edge, Emitted, Formula, LiveProcessor, Pattern, TimePointEvent};

let busy = Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64)));
let mut live = LiveProcessor::new(&[Processor::new("busy", busy)], 0).unwrap();

live.on_point("count", TimePointEvent::new(7, 9_i64)).unwrap();

// The state became active at 7, and nothing before 10 can change.
let emitted = live.advance("count", 10).unwrap();
assert_eq!(emitted[0].item, Emitted::Edge(TimePointEvent::new(7, Edge::Start)));
assert_eq!(live.frontier("busy"), Some(10));

// Its end is not known yet; `finish` closes what is open at the horizon.
assert!(live.advance("count", 20).unwrap().is_empty());
let closing = live.finish(30).unwrap();
assert_eq!(closing[0].item, Emitted::Edge(TimePointEvent::new(30, Edge::End)));
```

The processor keeps a retained window of its inputs: what the part not yet
emitted can depend on (the reach of its shifts, the sample that holds a `While`,
the events of a `Between` since its open span began) and the events that are not
final. A late event is `ProcessError::Late`, and `retained` counts what is held.
Unknown regions emit nothing, and the extent of an output is reported only as
its `frontier`. With `OpenEnd::Head` the head is the frontier of the inputs.

The state of a live processor can be saved and restored, as a checkpoint (see
Saving and loading).

## Record values

A sensor message has several fields, and a processor written at run time must
reach them by name. `Value` is a dynamic payload (`Null`, `Bool`, `Integer`,
`Float`, `Text`, or a `Record` of named fields) that goes into a
`TimePointSequence<Value>` beside the sequences of one fixed value type, which
keep their own dense layout. Values of one kind compare naturally, an integer and
a float compare as numbers, any other pair is unordered, so an ordering
`Condition<Value>` matches the numbers it applies to and nothing else. `get`
reaches a field by a dotted path, `"agent.id"`.

A `Schema` says what shape the values of a member must have, and it belongs to
the system. A record requires its fields and rejects unknown ones, `Optional`
fields may be left out, and a `Float` accepts an integer. Once set, recording a
value without the shape is `RecordError::SchemaMismatch`, naming the path of the
offending field, and a batch stays all or nothing.

`Events::Project` reads one field of each record out into a point sequence of
its own, of the type a `Kind` names. A record without the field is skipped, and
a field of another kind is `ProcessError::ProjectionMismatch`. A live processor
supports it like `Matching`, so what it emits equals the batch result.

```rust
use chronoloom::{Processor, StreamSystem};
use chronoloom::{
    Condition, Emitted, Events, Formula, Kind, LiveProcessor, Pattern, RecordError, Schema,
    SequenceSystem, TimePointEvent, TimePointSequence, Value,
};

let message = |agent: i64, zone: &str| {
    Value::record([
        ("agent", Value::record([("id", Value::from(agent))])),
        ("zone", Value::from(zone)),
    ])
};

let mut system = SequenceSystem::new();
system.insert("sensor", TimePointSequence::<Value>::new("sensor"));
system
    .set_schema(
        "sensor",
        Schema::record([
            ("agent", Schema::record([("id", Schema::Integer)])),
            ("zone", Schema::Text),
            ("dwell", Schema::optional(Schema::Float)),
        ]),
    )
    .unwrap();

system.record_point("sensor", TimePointEvent::new(10, message(7, "gate"))).unwrap();
system.record_point("sensor", TimePointEvent::new(20, message(9, "yard"))).unwrap();

// A value without the shape is refused, and nothing is recorded.
let wrong = Value::record([("agent", Value::from(7)), ("zone", Value::from("gate"))]);
let refused = system.record_point("sensor", TimePointEvent::new(30, wrong));
assert!(matches!(
    refused,
    Err(RecordError::SchemaMismatch { error, .. }) if error.path == "agent"
));

// A field becomes a sequence of its own, and an ordinary processor reads it.
let mut result = StreamSystem::new();
result.insert_stream("sensor", system.points::<Value>("sensor").unwrap().clone()).unwrap();
result.set_schema("sensor", system.schema("sensor").unwrap().clone()).unwrap();
result
    .add_processor(Processor::new(
        "agent",
        Events::project("sensor", "agent.id", Kind::Integer),
    ))
    .unwrap();
result
    .add_processor(Processor::new(
        "far_agent",
        Formula::while_matching(Pattern::new("agent", Condition::Greater(8_i64))),
    ))
    .unwrap();

let agents = result.points::<i64>("agent").unwrap();
assert_eq!(agents.iter().map(|event| *event.value()).collect::<Vec<_>>(), [7, 9]);
assert!(result.intervals("far_agent").unwrap().contains(25));

// Online, the same projection emits each point once it is final.
let zone = Processor::new("zone", Events::project("sensor", "zone", Kind::Text));
let mut live = LiveProcessor::new(&[zone], 0).unwrap();
live.on_point("sensor", TimePointEvent::new(10, message(7, "gate"))).unwrap();
let emitted = live.advance("sensor", 20).unwrap();
let Emitted::Point(point) = &emitted[0].item else { panic!("a point") };
assert_eq!(point.value().downcast_ref::<String>().map(String::as_str), Some("gate"));
```

A `Pattern` can reach into a record too. `Pattern::field(key, path, condition)`
applies a `Condition<Value>` to one field of each record of a member, so the
formulas that read patterns (`Between`, `While`, `Occurs`, `hold`,
`Events::Matching`) filter and derive states from a field without a projection
first, in batch and online. A record without the field does not match, whatever
the condition, and is not an error; for `While` it is a sample that does not
satisfy the condition, so it ends a span, where a projection would skip it. A
field that is `Null` is a value like another, so `Equals(Value::Null)` matches
it and an ordering does not. A member that does not hold `Value` points is
`ProcessError::ValueTypeMismatch`. A condition on two fields is two patterns
combined by formulas, an `And` of `While`s:

```rust
use chronoloom::{Processor, StreamSystem};
use chronoloom::{Condition, Formula, Pattern, TimePointEvent, TimePointSequence, Value};

let reading = |count: i64, zone: &str| {
    Value::record([("count", Value::from(count)), ("zone", Value::from(zone))])
};

let mut system = StreamSystem::new();
system.insert_stream("motion", TimePointSequence::from_events("motion", vec![
    TimePointEvent::new(10, reading(7, "gate")),
    TimePointEvent::new(20, reading(9, "yard")),
    TimePointEvent::new(30, reading(2, "yard")),
    TimePointEvent::new(40, reading(8, "gate")),
    TimePointEvent::new(50, Value::record([("zone", Value::from("gate"))])),
])).unwrap();

let busy = Pattern::field("motion", "count", Condition::Greater(Value::from(5)));
let at_gate = Pattern::field("motion", "zone", Condition::Equals(Value::from("gate")));
assert_eq!(busy.to_string(), "motion.count > 5");

// Busy at the gate: both fields at once. The last record has no count.
system
    .add_processor(Processor::new(
        "busy_at_gate",
        Formula::And(vec![Formula::while_matching(busy), Formula::while_matching(at_gate)]),
    ))
    .unwrap();
let spans: Vec<(i64, i64)> = system
    .intervals("busy_at_gate")
    .unwrap()
    .iter()
    .map(|span| (span.start(), span.end()))
    .collect();
assert_eq!(spans, [(10, 20), (40, 50)]);
```

## One timeline per entity

Presence is one timeline per entity: an agent enters, an agent exits. Sessions
per user and uptime per host have the same shape. It needs no sequence of its
own: a `Group` names the member of `Value` records and the field that says which
entity a record is about (`Group::new("gate", "agent")`), and three outputs of a
processor are built on it. `Recipe::per_entity` evaluates a formula once per
entity, and files each result under `<output key>/<entity>`; `Recipe::any_entity`
is the union of those timelines, and `Recipe::count_entities` is how many are
active, as a `TimePointSequence<i64>` of its changes, to be read with `While` like
any step value.

Inside the formula, the patterns over the member of the group read only the
records of the entity, as if the member had been filtered by hand, and the
patterns over any other member are shared: a door is the same door for every
agent.

```rust
use chronoloom::{Processor, StreamSystem};
use chronoloom::{Condition, Formula, Group, Recipe, Pattern};
use chronoloom::{TimePointEvent, TimePointSequence, Value};

let crossing = |agent: &str, direction: &str| {
    Value::record([("agent", Value::from(agent)), ("direction", Value::from(direction))])
};
let mut gate = StreamSystem::new();
gate.insert_stream("gate", TimePointSequence::from_events("gate", vec![
    TimePointEvent::new(10, crossing("agent-7", "in")),
    TimePointEvent::new(12, crossing("agent-9", "in")),
    TimePointEvent::new(20, crossing("agent-7", "out")),
    TimePointEvent::new(30, crossing("agent-9", "out")),
])).unwrap();

// Present from an "in" of an agent until the next "out" of the same agent.
let direction = |text: &str| {
    Pattern::field("gate", "direction", Condition::Equals(Value::from(text)))
};
let present = Formula::between(direction("in"), direction("out"));
let by_agent = Group::new("gate", "agent");

gate.add_processor(Processor::new("presence", Recipe::per_entity(by_agent.clone(), present.clone())))
    .unwrap();
gate.add_processor(Processor::new("anyone", Recipe::any_entity(by_agent.clone(), present.clone())))
    .unwrap();
gate.add_processor(Processor::new("crowd", Recipe::count_entities(by_agent, present)))
    .unwrap();
// A processor reads the others by key, the members of `presence` included.
gate.add_processor(Processor::new(
    "seven_after_hours",
    Formula::state("presence/agent-7").shift(0, 60),
))
.unwrap();
let result = &gate;

assert_eq!(result.intervals("presence/agent-7").unwrap()[0].bounds(), (10, 20));
assert_eq!(result.intervals("presence/agent-9").unwrap()[0].bounds(), (12, 30));
assert_eq!(result.intervals("anyone").unwrap()[0].bounds(), (10, 30));
assert_eq!(result.intervals("seven_after_hours").unwrap()[0].bounds(), (10, 80));

// The count opens with a point at the start of its extent, then changes.
let crowd: Vec<(i64, i64)> = result
    .points::<i64>("crowd")
    .unwrap()
    .iter()
    .map(|change| (change.timestamp(), *change.value()))
    .collect();
assert_eq!(crowd, [(i64::MIN, 0), (10, 1), (12, 2), (20, 1), (30, 0)]);

// Each member is the timeline of one entity.
assert_eq!(gate.entity("presence/agent-9"), Some(&Value::from("agent-9")));
```

An entity is a distinct value of the field: a boolean, a number or text. A record
with no such field, or `Null`, a record or a `NaN` in it, belongs to no entity and
is part of none of the restricted members, so it neither opens nor closes
anything. The integer `5` and the float `5.0` are one entity. A member key writes
text as it is and anything else as `Value` prints it, a `/` in the text is kept,
and two entities that would share a key (the text `5` and the integer `5`), or an
entity and another output, are `ProcessError::DuplicateMember`. Another formula of
the processor may read a member by its key; the dependency order treats any key
under `<output key>/` as a read of the whole output, so a key of an entity that
does not exist is `ProcessError::UnknownKey` and a formula of the output reading
its own members is a `Cycle`.

An entity exists from its first record, and its timeline is observed from that
instant. Before it, `any_entity` and `count_entities` treat it as inactive and
known, so an entity that has not appeared adds no uncertainty to what the
entities that have appeared give, and a count never changes for instants it has
already given. Before any entity has appeared, the union and the count are known
only where the records were observed. This changes nothing for a formula that
is inactive before the first record of the entity, which a presence is.
`count_entities` has a point where each run of its extent opens, the count being
known there from every entity, and one at each later change, ending spans before
starting ones at one instant so that a hand-over counts once. Within a fully known
extent, the durations of the entities add up to the count times the duration it
held.

A `LiveProcessor` runs these outputs too. It makes an entity when the first record
of it is fed, and emits its edges under the member key, `presence/agent-7`; what
it emitted equals the batch result, and a union or a count is final up to the
frontier of the records, since an entity may still appear there. It cannot read
the members of a per-entity output (`ProcessError::GroupedRead`), since it only
learns them as records arrive. Each entity is kept for good, with the retained
window one formula would hold, so the state grows with the entities seen and an
advance costs in proportion.

## Stream systems

A `StreamSystem` puts the pieces above together: it holds the streams of a
subject and a list of processors, and keeps what the processors derive up to
date as data arrives. Each `Processor` has the inputs it
reads, which are the keys its formula reads (`with_input` declares what one must
be), and the one output it produces.

Derived streams are read-only and always current. An `Update`, a set of points
and spans by stream key recorded all or nothing, evaluates again the processors
that read what it changed, directly or through another processor's output, and
no others; the `Report` it returns names the derived streams that changed and
the outputs evaluated again. A processor reads only what the system already
holds, and what a processor reads cannot be removed, so the list is always in an
order the system can evaluate.

```rust
use chronoloom::{Processor, StreamSystem, Update};
use chronoloom::{Condition, Formula, Pattern, TimeIntervalEvent, TimeIntervalSequence};
use chronoloom::{TimePointEvent, TimePointSequence};

let mut room = StreamSystem::new();
room.insert_stream("motion", TimePointSequence::<&str>::new("motion")).unwrap();
room.insert_stream("door", TimeIntervalSequence::from_spans("door", vec![
    TimeIntervalEvent::span(40, 50).unwrap(),
])).unwrap();

room.add_processor(Processor::new("presence", Formula::between(
    Pattern::new("motion", Condition::Equals("start")),
    Pattern::new("motion", Condition::Equals("end")),
))).unwrap();
room.add_processor(Processor::new(
    "occupied",
    Formula::state("presence").or(Formula::state("door")),
)).unwrap();

let mut update = Update::new();
update.points("motion", [TimePointEvent::new(10, "start"), TimePointEvent::new(25, "end")]);
let report = room.update(update).unwrap();

assert_eq!(report.changed().len(), 2);
assert_eq!(room.intervals("occupied").unwrap().len(), 2);
assert!(room.is_derived("presence"));
```

The mode of a stream system is a type parameter, so what every mode shares is
written once. Batch mode, a dataset held whole, is here; a live mode, where an
update also moves frontiers and the processors run incrementally, is to come.

## Saving and loading

Behind the `serde` feature, which is off by default and adds the `serde`
dependency (`cargo add chronoloom --features serde`), every type that holds data
implements `Serialize` and `Deserialize`, for any self-describing format. The
examples use JSON, and need the feature, so they are marked `ignore` here and
run as doctests of the `serialisation` module. A `TimePointSequence`, a
`TimeIntervalSequence`, a `SequenceSystem`, a `Processor` and a `LiveProcessor`
are documents that start with a format version, `{ "format": 1, .. }`; what is
nested in them carries none (a checkpoint holds a processor and a system, which
are documents of their own), and a reader refuses any version it does not know.
A system is saved whole: members, metadata, frontiers of live members, the
output each derived member came from (its lineage), entities and schemas.

```rust,ignore
use chronoloom::{
    Condition, Formula, Pattern, Processor, SequenceSystem, TimePointEvent, TimePointSequence,
};

let mut system = SequenceSystem::new();
system.insert("count", TimePointSequence::from_events("count", vec![
    TimePointEvent::new(3, 4_i64),
    TimePointEvent::new(9, 8_i64),
]));
system.metadata_mut().insert("unit".to_owned(), "seconds".to_owned());

let saved = serde_json::to_string(&system).unwrap();
let loaded: SequenceSystem = serde_json::from_str(&saved).unwrap();
assert_eq!(loaded, system);

// A processor is a document of its own, and a loaded one has been checked.
let processor = Processor::new(
    "busy",
    Formula::occurs(Pattern::new("count", Condition::Greater(5_i64))),
);
let rules = serde_json::to_string(&processor).unwrap();
let rules: Processor = serde_json::from_str(&rules).unwrap();
assert_eq!(rules, processor);

// A reader refuses any format version it does not know.
let future = saved.replacen(r#""format":1"#, r#""format":2"#, 1);
assert!(serde_json::from_str::<SequenceSystem>(&future).is_err());
```

Loading validates. A document that loads gives values whose invariants hold, as
though they had been built in code: spans that overlap, touch or come unsorted
are normalized, and one that ends before it starts is refused; point events are
sorted, ties keeping their stored order; data outside the extent a sequence was
observed over is refused (a point stamped `i64::MAX` and the data of a live
member excepted, as documented); a schema must fit the data of its member; and a
processor must pass `Processor::check`, so a stored processor that loads is a
valid one. A refusal says what was wrong and names the member's key.

A `Value` is plain JSON, `null`, `true`, `5`, `5.0`, `"text"` and `{..}`, and an
integer and a float stay distinct on the way back although `5 == 5.0`. A float
that is not finite is refused when saving. The point members of a system and the
patterns of a processor are tagged with their value type, `"f64"`, `"i64"`,
`"bool"`, `"text"`, `"value"` or `"edge"`: a closed set, so saving a member of
any other type, or loading a document with another tag, is an error naming the
key. The version history and the shapes are in the `serialisation` module
documentation.

A `LiveProcessor` is saved as a checkpoint: its processor, the retained window of
its inputs, where each output has emitted up to, and the entities seen, which is
what cannot be recomputed from the processor, so its size is the state bound of
the live processor. A host stores it with its own input offsets, and a restored
processor emits, and reports `frontier` and `retained`, exactly as one that never
stopped. A checkpoint is validated on load like any document: its inputs must be
those its processor reads, and its views, cursors and frontiers must agree. A
processor holding an input of a value type outside the closed set above cannot
be checkpointed, and the error names the key.

```rust,ignore
use chronoloom::{Condition, Formula, LiveProcessor, Pattern, TimePointEvent};

let busy = Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64)));
let mut live = LiveProcessor::new(&[Processor::new("busy", busy)], 0).unwrap();
live.on_point("count", TimePointEvent::new(7, 9_i64)).unwrap();
live.advance("count", 10).unwrap();

// The host saves it with its own offsets, and a new process loads it.
let saved = serde_json::to_string(&live).unwrap();
let mut restored: LiveProcessor = serde_json::from_str(&saved).unwrap();
assert_eq!(restored.processors(), live.processors());
assert_eq!(restored.frontier("busy"), Some(10));
assert_eq!(restored.finish(30).unwrap(), live.finish(30).unwrap());
```

### A JSON Schema

The declarations of a processor (see Declared inputs) are saved in an `inputs`
array of its document. Behind the `json-schema` feature, which implies `serde`
and adds `schemars` and `serde_json`,
`chronoloom::serialisation::processor_json_schema()` returns the JSON Schema of
the processor document for editors and servers.

```rust,ignore
use chronoloom::serialisation::processor_json_schema;
use chronoloom::{Formula, Input, Kind, Processor};

let processor = Processor::new("open", Formula::state("door").or(Formula::state("shut")))
    .with_input("door", Input::Timeline)
    .with_input("temperature", Input::Points(Kind::Float));

// Once inputs are declared, `shut` must be one of them.
assert_eq!(processor.validate().unwrap_err().len(), 1);

// The schema describes the document that is saved, `inputs` included.
let schema = processor_json_schema();
assert_eq!(schema["properties"]["format"]["const"], 1);
assert!(schema["properties"]["inputs"].is_object());
```

## Not here yet

What is open:

- A `StreamSystem` has a batch mode only: its live mode, and saving and loading
  it, are to come.
- Record values have no lists, and a projection copies the values rather than
  viewing them in place.
- A live processor keeps every entity of a per-entity output it has seen. A way
  to forget the inactive ones would cost a late record of such an entity its
  member, and does not exist yet.
- A live processor evaluates its retained window again on each advance. Leaves
  that update in constant state would replace that if it proves too costly.
- Sequences have `range`, `nearest`, `slice` and `contains`, and no further
  queries such as the gaps or the coverage of a timeline, and the set algebra
  is defined for `TimeIntervalSequence` only, not for `TimePointSequence`.

Every `rust` block in this file is compiled and run by `cargo test`; the blocks
marked `ignore` need a feature and run as doctests of the `serialisation` module.

## Python

Python bindings are published separately as
[`chronoloompy`](https://pypi.org/project/chronoloompy/), an API over this
crate that keeps data in Rust memory and hands Python zero-copy NumPy views of
it.

## Development

```sh
make lint   # cargo fmt --check, clippy (also with --all-features), and the dependency assertions
make test   # unit and integration tests, then all tests with --all-features
```

This crate must never depend on pyo3 — that is what makes it usable from any
Rust project — and with default features it depends on nothing at all.
`make lint` enforces both.

## License

MIT — see [LICENSE](LICENSE).
