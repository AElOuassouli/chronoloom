//! Running a processor online: the same outputs, produced as the inputs arrive.

use core::any::{self, Any};
use std::collections::{BTreeMap, BTreeSet};

use crate::primitives::{TimeIntervalEvent, TimePointEvent, Timestamp};
use crate::sequence_system::{Key, PointValue, RecordError, Sequence, SequenceSystem};
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::operations::union_spans;
use crate::sequences::{Extent, TimeIntervalSequence, TimePointSequence};
use crate::values::Value;

use super::condition::Pattern;
use super::edge::Edge;
use super::emission::{Emission, Emitted};
use super::errors::ProcessError;
use super::events::Events;
use super::formula::Formula;
use super::group::{self, EntityId, Group};
use super::input::Input;
use super::kind::Kind;
use super::processor::{Output, Processor};

#[cfg(feature = "serde")]
mod checkpoint;
#[cfg(feature = "serde")]
pub(crate) use checkpoint::{Checkpoint, Untyped};

/// A [`Processor`] run online: a state machine that is fed input events and
/// watermarks, and emits each part of its outputs as soon as it is final.
///
/// It has no clock, no input or output and no threads: a host feeds it with
/// [`on_point`] and [`on_span`], moves the frontier of an input with
/// [`advance`], and takes what each call returns. It is built by
/// [`Processor::live`], and what it emits is what [`Processor::run`] gives over
/// the whole stream, no more and no less.
///
/// ```
/// use chronoloom::{Condition, Edge, Emitted, Formula, Pattern, Processor, TimePointEvent};
///
/// let busy = Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64)));
/// let mut live = Processor::new().with("busy", busy).live(0)?;
///
/// live.on_point("count", TimePointEvent::new(3, 2_i64))?;
/// live.on_point("count", TimePointEvent::new(7, 9_i64))?;
///
/// // Nothing before the frontier changes any more: the state became active at 7.
/// let emitted = live.advance("count", 10)?;
/// assert_eq!(emitted.len(), 1);
/// assert_eq!(emitted[0].item, Emitted::Edge(TimePointEvent::new(7, Edge::Start)));
///
/// // Its end is not known yet: the span reaches the frontier.
/// assert_eq!(live.advance("count", 20)?, vec![]);
///
/// live.on_point("count", TimePointEvent::new(25, 1_i64))?;
/// let emitted = live.advance("count", 30)?;
/// assert_eq!(emitted[0].item, Emitted::Edge(TimePointEvent::new(25, Edge::End)));
/// assert_eq!(live.frontier("busy"), Some(30));
/// # Ok::<(), chronoloom::ProcessError>(())
/// ```
///
/// # The frontier and the lag of each output
///
/// Each input has a frontier, the watermark its host last gave it: its events
/// before it are final, an event before it is [`ProcessError::Late`], and an
/// event at it can still arrive. Every output has a frontier too, computed from
/// the frontiers of the inputs it reads by the lag of each node of its formula,
/// and everything the output denotes before its frontier is final:
///
/// | Node | Frontier |
/// |---|---|
/// | `State(k)`, `While`, `Occurs` over `k`, `Matching`, `Changes` over `k` | that of `k` |
/// | `Between` over `k` and `l` | the smaller of the two |
/// | `Not(f)` | that of `f` |
/// | `And(fs)`, `Or(fs)` | the smallest of the `fs`, and the end of time for none |
/// | `Shift { f, alpha, beta }` | that of `f` plus `min(alpha, beta)`, saturating |
///
/// A key that is itself an output (a chained output) has the frontier of that
/// output, so the lag of a chain is the sum of the lags along it. A shift by
/// `[-2, 2]` is final only up to the frontier minus two, since a span starting
/// at the frontier reaches back two ticks; a shift by `[3, 3]` is final up to
/// the frontier plus three. Because every node is pointwise in time or shifts
/// it, an output is final below its frontier whatever arrives later, which is
/// the determinism property: the online result below the frontier is the batch
/// result over the whole stream.
///
/// [`frontier`] reads the output frontier. It is the instant below which
/// everything has been returned to the host, so before the first
/// [`advance`] or [`finish`] it is `Timestamp::MIN`, and a call that moves it
/// returns what lies between the old and the new value.
///
/// # What is emitted
///
/// A call returns the emissions that became final, sorted by instant and, at
/// one instant, by key. Per output they are in time order, never repeat, and
/// never come back later in another place.
///
/// - A **state** output, and `Events::Edges`, emit [`Emitted::Edge`]s: a
///   `Start` at `s` once `s` is below the output frontier, and an `End` at `e`
///   once `e` is, so a start reaches the host before its span closes. An open
///   span has only emitted its start. The edges are those of the batch
///   evaluation: a span that reaches the edge of what is known ends there.
/// - **Unknown regions emit nothing.** Where the result is unobserved there is
///   no span and so no edge, and the extent of an output has no emission of its
///   own: it is the half-line below [`frontier`], and what the output was
///   unable to decide before that shows only as the absence of edges. A host
///   that needs the gaps reads them off its own watermarks.
/// - `Events::Project` emits [`Emitted::Point`]s too, the points of the
///   projection below the output frontier, each with the field read into the
///   type its `Kind` stands for (`bool`, `i64`, `f64`, `String` or `Value`),
///   erased as an [`AnyValue`](crate::AnyValue).
/// - `Events::Changes` emits [`Emitted::Point`]s too, the points of the member
///   that differ from the sample before them, below the output frontier, each
///   with its value erased as an [`AnyValue`](crate::AnyValue). An input that
///   only a `Changes` reads has the value type of its first `on_point`.
/// - `Events::Matching` emits [`Emitted::Point`]s: the points of the member
///   whose value matches, below the output frontier, each with its value erased
///   as an [`AnyValue`](crate::AnyValue). A processor does not know the value
///   type of the member a pattern reads, so the point cannot be typed
///   statically, and the erased value is the same one a [`Pattern`] condition
///   was checked against; `downcast_ref::<T>()` recovers it.
///
/// Edges rebuilt into spans with `Between(Edge = Start, Edge = End)` are the
/// spans of the batch result.
///
/// # Open spans
///
/// With [`OpenEnd::Extent`](super::OpenEnd::Extent) a span no end event has
/// closed stops where the extent does, and online the extent of an input stops
/// at its frontier, so the span is open and its end is emitted when an end
/// event or [`finish`] closes it. With [`OpenEnd::Head`](super::OpenEnd::Head)
/// the head is the frontier of the inputs, as the extent is, so both options
/// behave alike online, and the batch evaluation they are compared with is the
/// one with the head at the horizon rather than one past the latest point.
///
/// # Finishing
///
/// [`finish`] says the stream ended at a horizon. It advances every input that
/// is behind to `horizon`, drops the events at or after the frontier of each
/// input, and emits everything the outputs still have, ends of open spans
/// included. Over the inputs as recorded, with the extent of each ending at
/// the later of `horizon` and its frontier, the emissions of every call put
/// together are exactly the edges and points of [`Processor::run`], and the
/// frontier of every output becomes `Timestamp::MAX`. The processor then
/// refuses further events with [`ProcessError::Finished`].
///
/// # State and cost
///
/// Each output is the batch evaluation, chained outputs included, over a
/// retained window of the inputs, and an advance evaluates it again, so its
/// cost is that of the retained window, not of the whole stream; leaves with
/// constant state are for later. What the window must hold is what the part not
/// yet emitted can depend on, and nothing else is kept. Let `t` be the instant
/// before which everything of an output has been emitted, and `reach` the sum,
/// along a path of its formula, of the larger of `alpha` and `beta` of each
/// shift (never below zero). Then an input holds:
///
/// - its events at or after its own frontier, which are not final;
/// - the events from `t - 1 - reach` on, where a shift looks back to;
/// - for each `While` over it, the last sample at or before that instant, which
///   holds the state (none if its timeout ended before that instant, since what
///   follows is unknown either way); for each `Changes`, the last point before
///   that instant, which the first point kept is compared with; and for each
///   `Between`, the events since the start of the span open at that instant,
///   which is the state it carries.
///
/// The same holds for a chained output, which is evaluated over the window and
/// trusted from its own instant on. So the state is bounded by the lag along
/// each path and the open spans, and a stream of any length with spans that
/// close, samples that arrive and bounded shifts is held in constant space;
/// [`retained`] counts what is held, and a span held open for long holds the
/// events of its `Between` since it opened.
///
/// # Entities
///
/// An output [`PerEntity`], [`AnyEntity`] or [`CountEntities`] learns its
/// entities as records arrive: an entity is made when the first record of it is
/// fed with [`on_point`], and from then on [`Emission::key`] of its timeline is
/// the member key, `<output key>/<entity>`, as [`Group`] describes. The
/// processor keeps a prototype of the formula, which no host sees, over the
/// inputs it reads, so that an entity made late finds the inputs it needs where
/// the formula of an entity made at the start would. An entity exists from its
/// first record (an earlier record, fed later, moves that instant back to it),
/// and what a union or a count says of the instants before it does not change.
/// For that reason a union or a count is final only up to the frontier of the
/// records, whatever the lag of the formula: an entity may still appear there.
/// What is emitted equals the batch result below the frontier of each output and
/// in the end, as for any other output; that property is tested over random
/// processors that include these outputs. The frontier of a [`PerEntity`] output
/// is that of the prototype, the one an entity made now would start with.
///
/// The records of a group must be an input of the processor, not an output, and
/// no output may read a member of a [`PerEntity`] output
/// ([`ProcessError::GroupedRead`]): which members exist is only known as events
/// arrive. A union or a count is read by its own key like any output. Two
/// entities that would be filed under one member key are
/// [`ProcessError::DuplicateMember`] when the second is fed, and nothing is
/// recorded.
///
/// What is held grows with the entities seen, which are never forgotten: for
/// each, a view of its records and a member, each with the retained window a
/// single formula would hold, and the entity itself, so the state is that of one
/// formula times the number of entities, plus the records of the group that are
/// not final (each held once as fed and once in the view of its entity). An
/// advance evaluates every entity again, so its cost grows with them as well.
/// A stream with an unbounded number of entities (a session per visitor) holds
/// an unbounded state; dropping entities that have gone quiet is left open, as
/// the idea of a `forget_inactive` that would stop one when no record of it came
/// for some time, and cost its member the later records it might still
/// receive.
///
/// # Checkpoints
///
/// With the `serde` feature, a `LiveProcessor` is a document of its own,
/// `{ "format": 1, .. }`, that a host saves with its own input offsets and loads
/// to go on where it stopped: what is emitted after the restore, and the
/// [`frontier`] and [`retained`] of every call, are those of a processor that
/// never stopped. A checkpoint holds the [`processor`] itself, so it cannot be
/// restored against another one, and what the processor does not determine:
/// the live inputs with their frontiers and the events they hold (the retained
/// window), the instant each output has emitted up to and the one it is trusted
/// from, the instant each `Between` leaf starts at, the entities seen of each
/// per-entity output with their first instant and the view of their records,
/// the points of each count, and whether the stream was finished. The
/// prototypes, the members of the entities, the views of the `Between` leaves
/// and the lag of each output are recomputed on load, so the size of a
/// checkpoint is the state bound described above: the retained window and the
/// entities seen. An input that no event has typed yet is saved as such.
///
/// A checkpoint is checked on load as every document is: its processor passes
/// `Processor::check` and runs online, its inputs are those the processor
/// reads, in the shape and value type it reads them, and each live; the views
/// of the entities hold the records of their entity, no earlier than it was
/// first seen and as far on as their input; every output has a state, whose
/// cuts are as many as its `Between` leaves and no later than where the output
/// is trusted from, itself no later than where it has emitted up to; and no
/// output has emitted past the frontier its inputs give it (members and unions
/// of entities, which an entity seen since can lower, excepted). A finished
/// checkpoint has no inputs and every output final. A processor holding an
/// input of a value type outside the closed set of the format cannot be saved,
/// and the error names its key.
///
/// [`AnyEntity`]: super::Output::AnyEntity
/// [`CountEntities`]: super::Output::CountEntities
/// [`Group`]: super::Group
/// [`PerEntity`]: super::Output::PerEntity
/// [`advance`]: LiveProcessor::advance
/// [`finish`]: LiveProcessor::finish
/// [`frontier`]: LiveProcessor::frontier
/// [`on_point`]: LiveProcessor::on_point
/// [`on_span`]: LiveProcessor::on_span
/// [`processor`]: LiveProcessor::processor
/// [`retained`]: LiveProcessor::retained
#[derive(Debug, Clone)]
pub struct LiveProcessor {
    /// The processor this was built from, as it was given.
    processor: Processor,
    /// The outputs in dependency order, with every `Head` read as the extent.
    outputs: Vec<(Key, Output)>,
    /// The live members every output reads: the retained window.
    inputs: SequenceSystem,
    /// Per output, the instant before which everything has been emitted.
    through: BTreeMap<Key, Timestamp>,
    /// Per output, the instant from which its evaluation is right and the
    /// outputs reading it may rely on it.
    trusted: BTreeMap<Key, Timestamp>,
    /// Per `Between` leaf, by the key of the view its `start` reads.
    betweens: BTreeMap<Key, Between>,
    /// The inputs that only an `Events::Changes` reads, which have no value
    /// type until their first event arrives.
    untyped: BTreeSet<Key>,
    /// The per-entity outputs, with the entities seen so far.
    groups: Vec<GroupState>,
    /// The outputs in dependency order, a group standing for all it expands to.
    slots: Vec<Slot>,
    /// The points of each count as last evaluated, which carry the sample
    /// before the window over to the next evaluation.
    counts: BTreeMap<Key, Vec<TimePointEvent<i64>>>,
    /// What the outputs that belong to a group are.
    roles: BTreeMap<Key, Role>,
    finished: bool,
}

/// What an evaluation gives: each output, the system they were evaluated in, and
/// the points of each count.
type Evaluated = (
    Vec<Sequence>,
    SequenceSystem,
    Vec<(Key, Vec<TimePointEvent<i64>>)>,
);

/// An output of the user, or a group that expands to several.
#[derive(Debug, Clone)]
enum Slot {
    Plain(Key, Output),
    Group(usize),
}

/// What a per-entity output makes of its entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Aggregation {
    PerEntity,
    Any,
    Count,
}

/// A per-entity output run online: a prototype that is never emitted, which
/// keeps the inputs of the formula for entities not seen yet, and one member per
/// entity seen.
#[derive(Debug, Clone)]
struct GroupState {
    key: Key,
    aggregation: Aggregation,
    by: Group,
    /// The formula of one entity, with its `Between`s stopping at the extent.
    formula: Formula,
    /// The prototype output, and the view of the records it reads.
    template: Key,
    template_output: Output,
    template_betweens: Vec<Key>,
    template_view: Key,
    entities: BTreeMap<EntityId, Born>,
}

/// An entity seen: its member, the view of its records, and its formula.
#[derive(Debug, Clone)]
struct Born {
    name: String,
    /// The value of the field that names the entity, as its first record had it.
    #[cfg(feature = "serde")]
    value: Value,
    /// Its place among the entities of its group, in the order they were seen.
    #[cfg(feature = "serde")]
    index: usize,
    member: Key,
    view: Key,
    /// The earliest instant of one of its records fed so far.
    first: Timestamp,
    output: Output,
}

/// What an output that belongs to a group is.
#[derive(Debug, Clone)]
enum Role {
    /// The prototype of a group.
    Template(usize),
    /// The member of an entity of group `group`.
    Member { group: usize, id: EntityId },
    /// The union or the count of the group.
    Aggregate(usize),
}

/// An entity a record is about, and whether it was seen before.
struct Sighting {
    group: usize,
    id: EntityId,
    /// The value that names the entity, kept only for an entity seen first.
    new: Option<Value>,
}

/// What a `Between` leaf carries from one evaluation to the next: the instant
/// its state machine starts at, inactive, over views of the two series it
/// reads. The state before that instant was inactive, so the machine, started
/// there, is in the state it is in over the whole history.
#[derive(Debug, Clone)]
struct Between {
    start: Key,
    end: Key,
    cut: Timestamp,
}

/// How a leaf reads a key: as a timeline, as points a pattern is over, or as
/// the record values a projection reads.
#[derive(Clone, Copy)]
enum Read<'a> {
    Timeline,
    /// Points of any value type.
    Any,
    Points(&'a Pattern),
    Records,
}

/// What a leaf of a formula reads.
enum Leaf<'a> {
    State(&'a Key),
    Records(&'a Key),
    Occurs(&'a Pattern),
    While {
        pattern: &'a Pattern,
        timeout: Option<Timestamp>,
    },
    Changes(&'a Key),
    Between {
        node: &'a Formula,
        start: &'a Pattern,
        end: &'a Pattern,
    },
}

impl Processor {
    /// A [`LiveProcessor`] for the outputs, whose inputs are final before
    /// `from`.
    ///
    /// Every key the outputs read that is not itself an output is an input, a
    /// live member of the shape and value type its reads need, with nothing
    /// recorded yet.
    ///
    /// # Errors
    ///
    /// [`ProcessError::Cycle`] if the outputs read each other in a circle, and
    /// [`ProcessError::NotATimeline`], [`ProcessError::NotPoints`] or
    /// [`ProcessError::ValueTypeMismatch`] if the outputs read one key as two
    /// shapes or value types, or read an output as the wrong one.
    pub fn live(&self, from: Timestamp) -> Result<LiveProcessor, ProcessError> {
        let order = self.order()?;

        // A member of a per-entity output is only known once its entity is
        // seen, and the records a group reads must be an input.
        let per_entity: Vec<&Key> = self
            .outputs()
            .iter()
            .filter(|(_, output)| matches!(output, Output::PerEntity { .. }))
            .map(|(key, _)| key)
            .collect();
        for key in &order {
            let output = &self.outputs()[*key];
            let mut read = Vec::new();
            output.read_keys(&mut read);
            if let Some(member) = read
                .into_iter()
                .find(|name| per_entity.iter().any(|owner| group::is_under(name, owner)))
            {
                return Err(ProcessError::GroupedRead {
                    key: member.clone(),
                });
            }
            if let Output::PerEntity { by, .. }
            | Output::AnyEntity { by, .. }
            | Output::CountEntities { by, .. } = output
            {
                if self.outputs().contains_key(&by.key) {
                    return Err(ProcessError::GroupedRead {
                        key: by.key.clone(),
                    });
                }
            }
        }

        // The outputs as they are read, a group standing for its prototype.
        let mut groups: Vec<GroupState> = Vec::new();
        let mut unviewed: Vec<(Key, Output)> = Vec::new();
        let mut layout: Vec<(Option<usize>, usize)> = Vec::new();
        let mut aggregates: Vec<(Key, Output)> = Vec::new();
        for key in order {
            let output = &self.outputs()[key];
            let (by, formula, aggregation) = match output {
                Output::PerEntity { by, formula } => (by, formula, Aggregation::PerEntity),
                Output::AnyEntity { by, formula } => (by, formula, Aggregation::Any),
                Output::CountEntities { by, formula } => (by, formula, Aggregation::Count),
                plain => {
                    layout.push((None, unviewed.len()));
                    unviewed.push((key.clone(), until_extent(plain)));
                    continue;
                }
            };

            let formula = formula.open_until_extent();
            let template_view = Key::from(format!("{key}\0template"));
            let template = Key::from(format!("{key}\0prototype"));
            layout.push((Some(groups.len()), unviewed.len()));
            unviewed.push((
                template.clone(),
                Output::State(group::retarget(&formula, &by.key, &template_view)),
            ));
            if aggregation != Aggregation::PerEntity {
                aggregates.push((key.clone(), Output::State(Formula::Or(Vec::new()))));
            }
            groups.push(GroupState {
                key: key.clone(),
                aggregation,
                by: by.clone(),
                formula,
                template,
                template_output: Output::State(Formula::Or(Vec::new())),
                template_betweens: Vec::new(),
                template_view,
                entities: BTreeMap::new(),
            });
        }
        let known: Vec<(Key, Output)> = unviewed.iter().cloned().chain(aggregates).collect();

        // The reads of every leaf, each by the input it reaches: a read of the
        // output of an `Events::Changes` is a read of what it reads, and of an
        // output of another kind is no input.
        let mut reads: Vec<(&Key, Read<'_>)> = Vec::new();
        for group in &groups {
            reads.push((&group.by.key, Read::Records));
            reads.push((&group.template_view, Read::Records));
        }
        for (_, output) in &unviewed {
            for (leaf, _) in leaves(output) {
                for (key, read) in leaf.reads() {
                    let reached = match read {
                        Read::Timeline => {
                            (!known.iter().any(|(output_key, _)| output_key == key)).then_some(key)
                        }
                        _ => source(&known, key),
                    };
                    reads.extend(reached.map(|key| (key, read)));
                }
            }
        }

        // The declared inputs come first, so that every read is checked against
        // them and points of any type take the type declared.
        let mut declared: BTreeMap<&Key, Sequence> = BTreeMap::new();
        let mut untyped = BTreeSet::new();
        for (key, input) in self.inputs() {
            if self.outputs().contains_key(key) {
                return Err(ProcessError::InputIsOutput { key: key.clone() });
            }
            declared.insert(key, empty_input(key, input));
        }

        // Typed reads first, so that points of any type take the type read.
        for (key, read) in &reads {
            if !matches!(read, Read::Any) {
                declare(&mut declared, key, *read)?;
            }
        }
        for (key, read) in &reads {
            if matches!(read, Read::Any) {
                if !declared.contains_key(key) {
                    untyped.insert((*key).clone());
                }
                declare(&mut declared, key, *read)?;
            }
        }

        let mut inputs = SequenceSystem::new();
        for (key, sequence) in declared {
            inputs.insert_live(key.clone(), sequence, from);
            if let Some(schema) = self.inputs().get(key).and_then(Input::schema) {
                inputs
                    .set_schema(key.as_str(), schema.clone())
                    .map_err(|error| refused(error, key.as_str(), false))?;
            }
        }

        let mut betweens = BTreeMap::new();
        let mut viewed: Vec<(Key, Output, Vec<Key>)> = Vec::new();
        for (key, output) in &unviewed {
            let mut created = Vec::new();
            let output = with_views(output, &mut betweens, &mut created);
            viewed.push((key.clone(), output, created));
        }

        let mut slots = Vec::new();
        let mut roles = BTreeMap::new();
        for (group, index) in layout {
            let (key, output, created) = viewed[index].clone();
            let Some(group) = group else {
                slots.push(Slot::Plain(key, output));
                continue;
            };
            let state = &mut groups[group];
            state.template_output = output;
            state.template_betweens = created;
            roles.insert(key, Role::Template(group));
            if state.aggregation != Aggregation::PerEntity {
                roles.insert(state.key.clone(), Role::Aggregate(group));
            }
            slots.push(Slot::Group(group));
        }

        let mut live = LiveProcessor {
            processor: self.clone(),
            outputs: Vec::new(),
            inputs,
            through: BTreeMap::new(),
            trusted: BTreeMap::new(),
            betweens,
            untyped,
            groups,
            slots,
            roles,
            counts: BTreeMap::new(),
            finished: false,
        };
        live.rebuild();
        for (key, _) in &live.outputs {
            live.through.insert(key.clone(), Timestamp::MIN);
            live.trusted.insert(key.clone(), Timestamp::MIN);
        }
        // Reading the outputs once surfaces what only a chain can get wrong.
        live.evaluate()?;

        Ok(live)
    }
}

impl LiveProcessor {
    /// The processor this was built from.
    #[must_use]
    pub fn processor(&self) -> &Processor {
        &self.processor
    }

    /// Record a point event into the input `key`.
    ///
    /// # Errors
    ///
    /// [`ProcessError::Late`] if the event is before the frontier of the input,
    /// [`ProcessError::UnknownKey`] if no output reads `key`,
    /// [`ProcessError::NotPoints`] if the input is a timeline,
    /// [`ProcessError::ValueTypeMismatch`] if the input's points carry another
    /// value type than `T`, and [`ProcessError::Finished`] after `finish`.
    /// Nothing is recorded in any of those cases. An input that only an
    /// `Events::Changes` reads takes the type `T` of its first event.
    pub fn on_point<T: PointValue>(
        &mut self,
        key: &str,
        event: TimePointEvent<T>,
    ) -> Result<(), ProcessError> {
        self.ensure_running()?;

        let (record, sightings) = self.sight(key, &event)?;

        let typing = self.untyped.contains(key) && any::type_name::<T>() != any::type_name::<()>();
        if typing {
            let frontier = self.inputs.frontier_of(key).unwrap_or(Timestamp::MIN);
            self.inputs
                .insert_live(key, TimePointSequence::<T>::new(key), frontier);
        }

        let recorded = self.inputs.record_point(key, event);
        if typing {
            if recorded.is_ok() {
                self.untyped.remove(key);
            } else {
                // Nothing was recorded, so the input has no type yet.
                let frontier = self.inputs.frontier_of(key).unwrap_or(Timestamp::MIN);
                self.inputs
                    .insert_live(key, TimePointSequence::<()>::new(key), frontier);
            }
        }

        recorded.map_err(|error| refused(error, key, false))?;

        if let Some(record) = record {
            for sighting in sightings {
                self.route(sighting, &record)?;
            }
        }

        Ok(())
    }

    /// Record a span into the timeline input `key`.
    ///
    /// # Errors
    ///
    /// [`ProcessError::Late`] if the span starts before the frontier of the
    /// input, [`ProcessError::UnknownKey`] if no output reads `key`,
    /// [`ProcessError::NotATimeline`] if the input holds points, and
    /// [`ProcessError::Finished`] after `finish`. Nothing is recorded in any of
    /// those cases.
    pub fn on_span(&mut self, key: &str, span: TimeIntervalEvent<()>) -> Result<(), ProcessError> {
        self.ensure_running()?;
        self.inputs
            .record_span(key, span)
            .map_err(|error| refused(error, key, true))
    }

    /// Move the frontier of the input `key` to `watermark` and return what
    /// became final, as the type documentation says.
    ///
    /// A watermark at or before the frontier moves nothing, and returns what
    /// the outputs still had to emit, which is nothing after a first call.
    ///
    /// # Errors
    ///
    /// [`ProcessError::UnknownKey`] if no output reads `key`, and
    /// [`ProcessError::Finished`] after `finish`.
    pub fn advance(
        &mut self,
        key: &str,
        watermark: Timestamp,
    ) -> Result<Vec<Emission>, ProcessError> {
        self.ensure_running()?;
        self.inputs
            .advance(key, watermark)
            .map_err(|error| refused(error, key, false))?;
        for group in &self.groups {
            if group.by.key.as_str() == key {
                let views = group
                    .entities
                    .values()
                    .map(|born| &born.view)
                    .chain([&group.template_view]);
                for view in views {
                    self.inputs
                        .advance(view.as_str(), watermark)
                        .map_err(|error| refused(error, view.as_str(), false))?;
                }
            }
        }

        self.step(false)
    }

    /// End the stream at `horizon`, and return everything the outputs still
    /// have to emit, as the type documentation says. A second call returns
    /// nothing.
    ///
    /// # Errors
    ///
    /// Never in practice: the outputs were checked against the inputs when the
    /// processor was built. The signature is fallible so that a later check
    /// cannot become a panic.
    pub fn finish(&mut self, horizon: Timestamp) -> Result<Vec<Emission>, ProcessError> {
        if self.finished {
            return Ok(Vec::new());
        }

        self.inputs.advance_all(horizon);
        // What lies at or after a frontier is outside the extent now: drop it,
        // and with it an entity none of whose records is left.
        self.inputs.observe(&Extent::everything());
        for group in &mut self.groups {
            let end = self.inputs.frontier_of(group.by.key.as_str());
            for born in group.entities.values_mut() {
                if end.is_some_and(|end| born.first >= end) {
                    born.first = Timestamp::MAX;
                }
            }
        }

        let emissions = self.step(true)?;
        self.finished = true;
        self.inputs.clear();

        Ok(emissions)
    }

    /// The frontier of the output `key`: everything before it has been
    /// returned. `None` if `key` is not an output.
    #[must_use]
    pub fn frontier(&self, key: &str) -> Option<Timestamp> {
        self.through.get(key).copied().or_else(|| {
            // A per-entity output has no member of its own: its frontier is
            // that of an entity not seen yet.
            self.groups
                .iter()
                .find(|group| group.key.as_str() == key)
                .and_then(|group| self.through.get(&group.template).copied())
        })
    }

    /// How many events and spans the inputs hold at the moment: the retained
    /// window of the type documentation.
    #[must_use]
    pub fn retained(&self) -> usize {
        self.inputs.iter().map(|(_, sequence)| sequence.len()).sum()
    }

    /// The record of an event for a group input, and the entities of each
    /// group it is about, checked for keys they would share.
    fn sight<T: PointValue>(
        &self,
        key: &str,
        event: &TimePointEvent<T>,
    ) -> Result<(Option<TimePointEvent<Value>>, Vec<Sighting>), ProcessError> {
        let Some(record) = (event as &dyn Any).downcast_ref::<TimePointEvent<Value>>() else {
            return Ok((None, Vec::new()));
        };

        let mut sightings = Vec::new();
        for (index, group) in self.groups.iter().enumerate() {
            if group.by.key.as_str() != key {
                continue;
            }
            let Some((id, value)) = group::entity_of(record.value(), &group.by.path) else {
                continue;
            };

            let new = !group.entities.contains_key(&id);
            let name = group::render(value);
            if new {
                let member = group::member_key(&group.key, &name);
                // Only the members a host can see are filed under their name.
                let shared = group.aggregation == Aggregation::PerEntity
                    && (group.entities.values().any(|born| born.name == name)
                        || self.outputs.iter().any(|(output, _)| *output == member));
                if shared {
                    return Err(ProcessError::DuplicateMember { key: member });
                }
            }
            sightings.push(Sighting {
                group: index,
                id,
                new: new.then(|| value.clone()),
            });
        }

        Ok((Some(record.clone()), sightings))
    }

    /// Record a record into the view of its entity, which a first sight makes.
    fn route(
        &mut self,
        sighting: Sighting,
        record: &TimePointEvent<Value>,
    ) -> Result<(), ProcessError> {
        let Sighting { group, id, new } = sighting;
        if let Some(value) = new {
            self.bear(group, id.clone(), value, record.timestamp());
        }

        let born = self.groups[group]
            .entities
            .get_mut(&id)
            .expect("the entity was born above");
        born.first = born.first.min(record.timestamp());
        let view = born.view.clone();

        self.inputs
            .record_point(view.as_str(), record.clone())
            .map_err(|error| refused(error, view.as_str(), false))
    }

    /// File the entity `id` of `group`, named by `value` and first seen at
    /// `timestamp`: a view of its records, and a member that starts where the
    /// prototype stands.
    fn bear(&mut self, group: usize, id: EntityId, value: Value, timestamp: Timestamp) {
        let (member, view, created) = self.file_entity(group, id, value, timestamp);
        for (new, old) in created.iter().zip(&self.groups[group].template_betweens) {
            let cut = self.betweens[old].cut;
            if let Some(between) = self.betweens.get_mut(new) {
                between.cut = cut;
            }
        }

        let state = &self.groups[group];
        let frontier = self
            .inputs
            .frontier_of(state.by.key.as_str())
            .unwrap_or(Timestamp::MIN);
        self.inputs.insert_live(
            view.clone(),
            TimePointSequence::<Value>::new(view.as_str()),
            frontier,
        );

        let template = state.template.clone();
        let through = self
            .through
            .get(&template)
            .copied()
            .unwrap_or(Timestamp::MIN);
        let trusted = self
            .trusted
            .get(&template)
            .copied()
            .unwrap_or(Timestamp::MIN);
        self.through.insert(member.clone(), through);
        self.trusted.insert(member, trusted);
        self.rebuild();
    }

    /// Make the entity `id` of `group` with the keys its place gives it and the
    /// formula over its own view, and file it. Returns its member, its view and
    /// the views of its `Between` leaves.
    fn file_entity(
        &mut self,
        group: usize,
        id: EntityId,
        value: Value,
        first: Timestamp,
    ) -> (Key, Key, Vec<Key>) {
        let state = &self.groups[group];
        let index = state.entities.len();
        let name = group::render(&value);
        let member = if state.aggregation == Aggregation::PerEntity {
            group::member_key(&state.key, &name)
        } else {
            Key::from(format!("{}\0member\0{index}", state.key))
        };
        let view = Key::from(format!("{}\0view\0{index}", state.key));

        let formula = group::retarget(&state.formula, &state.by.key, &view);
        let mut created = Vec::new();
        let output = with_views(&Output::State(formula), &mut self.betweens, &mut created);

        self.roles.insert(
            member.clone(),
            Role::Member {
                group,
                id: id.clone(),
            },
        );
        self.groups[group].entities.insert(
            id,
            Born {
                name,
                #[cfg(feature = "serde")]
                value,
                #[cfg(feature = "serde")]
                index,
                member: member.clone(),
                view: view.clone(),
                first,
                output,
            },
        );

        (member, view, created)
    }

    /// The outputs in dependency order, every group expanded into its
    /// prototype, its entities and what it makes of them.
    fn rebuild(&mut self) {
        let mut outputs = Vec::new();
        for slot in &self.slots {
            let index = match slot {
                Slot::Plain(key, output) => {
                    outputs.push((key.clone(), output.clone()));
                    continue;
                }
                Slot::Group(index) => *index,
            };

            let group = &self.groups[index];
            outputs.push((group.template.clone(), group.template_output.clone()));
            let mut members: Vec<&Born> = group.entities.values().collect();
            members.sort_by(|first, second| first.member.cmp(&second.member));
            for born in &members {
                outputs.push((born.member.clone(), born.output.clone()));
            }

            let states = Formula::Or(
                members
                    .iter()
                    .map(|born| Formula::State(born.member.clone()))
                    .collect(),
            );
            let by = group.by.clone();
            match group.aggregation {
                Aggregation::PerEntity => {}
                Aggregation::Any => {
                    outputs.push((
                        group.key.clone(),
                        Output::AnyEntity {
                            by,
                            formula: states,
                        },
                    ));
                }
                Aggregation::Count => {
                    outputs.push((
                        group.key.clone(),
                        Output::CountEntities {
                            by,
                            formula: states,
                        },
                    ));
                }
            }
        }
        self.outputs = outputs;
    }

    /// Whether the output is for the host: not a prototype, nor a member that
    /// only a union or a count is made of.
    fn is_silent(&self, key: &Key) -> bool {
        match self.roles.get(key) {
            Some(Role::Template(_)) => true,
            Some(Role::Member { group, .. }) => {
                self.groups[*group].aggregation != Aggregation::PerEntity
            }
            _ => false,
        }
    }

    /// The most an output can be final up to: an entity may still appear at the
    /// frontier of its records, so the prototype and what is made of all the
    /// entities are final only up to there.
    fn cap(&self, key: &Key) -> Option<Timestamp> {
        match self.roles.get(key)? {
            Role::Template(group) | Role::Aggregate(group) => {
                self.inputs.frontier_of(self.groups[*group].by.key.as_str())
            }
            Role::Member { .. } => None,
        }
    }

    /// What a union or count of a group is, over its members as they stand in
    /// `working`.
    ///
    /// For a count, also the points it is made of, to be carried to the next
    /// evaluation.
    fn aggregate(
        &self,
        group: usize,
        key: &Key,
        output: &Output,
        working: &SequenceSystem,
    ) -> (Sequence, Option<Vec<TimePointEvent<i64>>>) {
        let state = &self.groups[group];
        let mut members: Vec<&Born> = state.entities.values().collect();
        members.sort_by(|first, second| first.member.cmp(&second.member));
        let timelines: Vec<(Timestamp, &TimeIntervalSequence)> = members
            .iter()
            .filter_map(|born| match working.get(born.member.as_str()) {
                Some(Sequence::TimeInterval(timeline)) => Some((born.first, timeline)),
                _ => None,
            })
            .collect();
        // The records of a live input were observed up to their frontier, the
        // part of the window kept being no measure of it.
        let observed = self
            .inputs
            .frontier_of(state.by.key.as_str())
            .map_or_else(Extent::everything, |frontier| {
                Extent::of_range(&(..frontier))
            });
        let label = output.to_string();

        if state.aggregation != Aggregation::Count {
            return (
                Sequence::from(group::any_of(label, &timelines, &observed)),
                None,
            );
        }

        // The members start where they were cut, which makes the count start
        // there, with a point that is no change: the point before it, which the
        // window no longer holds, is carried over in its place.
        // A member that first appeared after its cut lost nothing to it.
        let cut = members
            .iter()
            .filter_map(|born| {
                self.trusted
                    .get(&born.member)
                    .copied()
                    .filter(|&trusted| born.first < trusted)
            })
            .max()
            .unwrap_or(Timestamp::MIN);
        let counted = group::count_of(label.clone(), &timelines, &observed, cut);
        let mut events: Vec<TimePointEvent<i64>> = counted.iter().copied().collect();
        let mut extent = counted.observed().clone();
        let starts_at_cut = extent
            .as_slice()
            .first()
            .is_some_and(|run| run.start() == cut)
            && events.first().is_some_and(|point| point.timestamp() == cut);
        if cut > Timestamp::MIN && starts_at_cut {
            let held = self
                .counts
                .get(key)
                .and_then(|points| points.iter().rev().find(|point| point.timestamp() <= cut));
            if let Some(held) = held.filter(|point| point.timestamp() < cut) {
                events[0] = *held;
                let before = [TimeIntervalEvent::raw(held.timestamp(), cut)];
                extent = Extent::from_normalized(union_spans(extent.as_slice(), &before));
            }
        }

        let points = events.clone();
        let sequence = TimePointSequence::from_parts(Attribute::name(label), events, extent);

        (Sequence::from(sequence), Some(points))
    }

    fn ensure_running(&self) -> Result<(), ProcessError> {
        if self.finished {
            Err(ProcessError::Finished)
        } else {
            Ok(())
        }
    }

    /// Every output evaluated over the retained window, in dependency order,
    /// with a working system in which each output stands under its key, cut to
    /// where it is trusted.
    fn evaluate(&self) -> Result<Evaluated, ProcessError> {
        let mut working = self.inputs.clone();
        let mut sequences = Vec::with_capacity(self.outputs.len());
        let mut counts = Vec::new();

        for (key, output) in &self.outputs {
            for (leaf, _) in leaves(output) {
                if let Leaf::Between { start, end, .. } = leaf {
                    self.add_views(&mut working, &start.key, &end.key)?;
                }
            }

            let mut sequence = match self.roles.get(key) {
                Some(Role::Aggregate(group)) => {
                    let (sequence, points) = self.aggregate(*group, key, output, &working);
                    counts.extend(points.map(|points| (key.clone(), points)));
                    sequence
                }
                _ => output.evaluate(&working)?,
            };
            if let Some(Role::Member { group, id }) = self.roles.get(key) {
                // An entity exists from its first record on.
                cut_before(&mut sequence, self.groups[*group].entities[id].first);
            }
            let mut trusted = sequence.clone();
            cut_before(&mut trusted, self.trusted[key]);
            working.insert(key.clone(), trusted);
            sequences.push(sequence);
        }

        Ok((sequences, working, counts))
    }

    /// File the views of the two series of a `Between` leaf, each cut to the
    /// instant its machine starts at.
    fn add_views(
        &self,
        working: &mut SequenceSystem,
        start_view: &Key,
        end_view: &Key,
    ) -> Result<(), ProcessError> {
        let between = &self.betweens[start_view];
        for (series, view) in [(&between.start, start_view), (&between.end, end_view)] {
            let mut sequence =
                working
                    .get(series.as_str())
                    .cloned()
                    .ok_or_else(|| ProcessError::UnknownKey {
                        key: series.clone(),
                    })?;
            cut_before(&mut sequence, between.cut);
            working.insert(view.clone(), sequence);
        }

        Ok(())
    }

    /// The frontier of the series under `key`: an output's, else an input's.
    fn series_frontier(&self, key: &str, outputs: &BTreeMap<Key, Timestamp>) -> Timestamp {
        outputs
            .get(key)
            .copied()
            .or_else(|| self.inputs.frontier_of(key))
            .unwrap_or(Timestamp::MAX)
    }

    /// The lag rule over a formula.
    fn formula_frontier(&self, formula: &Formula, outputs: &BTreeMap<Key, Timestamp>) -> Timestamp {
        let series = |key: &Key| self.series_frontier(key.as_str(), outputs);

        match formula {
            Formula::State(key) => series(key),
            Formula::Between { start, .. } => {
                let between = &self.betweens[&start.key];
                series(&between.start).min(series(&between.end))
            }
            Formula::While { pattern, .. } | Formula::Occurs(pattern) => series(&pattern.key),
            Formula::Not(inner) => self.formula_frontier(inner, outputs),
            Formula::And(formulas) | Formula::Or(formulas) => formulas
                .iter()
                .map(|formula| self.formula_frontier(formula, outputs))
                .min()
                .unwrap_or(Timestamp::MAX),
            Formula::Shift {
                formula,
                alpha,
                beta,
            } => match self.formula_frontier(formula, outputs) {
                Timestamp::MAX => Timestamp::MAX,
                frontier => frontier.saturating_add(*alpha.min(beta)),
            },
        }
    }

    fn output_frontier(&self, output: &Output, outputs: &BTreeMap<Key, Timestamp>) -> Timestamp {
        match output {
            Output::State(formula)
            | Output::Events(Events::Edges(formula))
            | Output::PerEntity { formula, .. }
            | Output::AnyEntity { formula, .. }
            | Output::CountEntities { formula, .. } => self.formula_frontier(formula, outputs),
            Output::Events(Events::Matching(pattern)) => {
                self.series_frontier(pattern.key.as_str(), outputs)
            }
            Output::Events(Events::Project { key, .. } | Events::Changes { key }) => {
                self.series_frontier(key.as_str(), outputs)
            }
        }
    }

    /// Evaluate, emit what is final (everything when `finishing`), and, unless
    /// finishing, trim the inputs to what the rest can depend on.
    fn step(&mut self, finishing: bool) -> Result<Vec<Emission>, ProcessError> {
        let (sequences, working, counts) = self.evaluate()?;
        self.counts.extend(counts);

        let mut frontiers: BTreeMap<Key, Timestamp> = BTreeMap::new();
        let mut emissions = Vec::new();
        for ((key, output), sequence) in self.outputs.iter().zip(&sequences) {
            let mut frontier = self.output_frontier(output, &frontiers);
            if let Some(cap) = self.cap(key) {
                frontier = frontier.min(cap);
            }
            // An entity first seen after an output was final does not take that
            // back: what it adds starts at its first record, past the frontier.
            frontier = frontier.max(self.through[key]);
            let below = (!finishing).then_some(frontier);
            if !self.is_silent(key) {
                emit(
                    key,
                    output,
                    sequence,
                    self.through[key],
                    below,
                    &mut emissions,
                );
            }
            frontiers.insert(key.clone(), frontier);
        }
        emissions.sort_by(|first, second| {
            first
                .timestamp()
                .cmp(&second.timestamp())
                .then_with(|| first.key.cmp(&second.key))
        });

        for (key, frontier) in frontiers {
            let through = self.through.entry(key).or_insert(Timestamp::MIN);
            *through = (*through).max(if finishing { Timestamp::MAX } else { frontier });
        }
        if !finishing {
            self.retain(&working)?;
        }

        Ok(emissions)
    }

    /// Cut the inputs, and note where each output is trusted from, so that
    /// what is evaluated next is right from the instant the next emission
    /// needs.
    ///
    /// Walking the outputs from the last to the first, each says how far back
    /// it needs the series it reads: its own instant, less the reach of the
    /// leaf, and then the instant of the state the leaf carries there.
    fn retain(&mut self, working: &SequenceSystem) -> Result<(), ProcessError> {
        let mut required: BTreeMap<Key, Timestamp> = BTreeMap::new();
        for (key, _) in self.inputs.iter() {
            let frontier = self.inputs.frontier_of(key.as_str());
            required.insert(key.clone(), frontier.unwrap_or(Timestamp::MAX));
        }
        for (key, output) in &self.outputs {
            // A state is needed from the instant before the first emission,
            // whose edges look back one tick; edges and points are series, and
            // are needed from their first emission.
            let through = self.through[key];
            let lower = match output {
                Output::State(_)
                | Output::PerEntity { .. }
                | Output::AnyEntity { .. }
                | Output::CountEntities { .. }
                    if through != Timestamp::MIN =>
                {
                    through - 1
                }
                _ => through,
            };
            required.insert(key.clone(), lower);
        }

        let mut cuts: Vec<(Key, Timestamp)> = Vec::new();
        for (key, output) in self.outputs.iter().rev() {
            // The edges of a formula at an instant read its state just before.
            let lower = match output {
                Output::Events(Events::Edges(_)) => required[key].saturating_sub(1),
                _ => required[key],
            };
            for (leaf, reach) in leaves(output) {
                let need = if lower == Timestamp::MIN {
                    lower
                } else {
                    lower.saturating_sub(reach)
                };

                let (keys, anchor): (Vec<Key>, Timestamp) = match &leaf {
                    Leaf::State(state) => (vec![(*state).clone()], need),
                    Leaf::Occurs(pattern) => (vec![pattern.key.clone()], need),
                    Leaf::Records(records) => (vec![(*records).clone()], need),
                    // A sample that stopped holding before `need` says nothing
                    // more than the unknown after it, which a cut gives too.
                    Leaf::While { pattern, timeout } => (
                        vec![pattern.key.clone()],
                        last_sample(working, pattern, need)
                            .filter(|sample| {
                                timeout.is_none_or(|timeout| sample.saturating_add(timeout) > need)
                            })
                            .unwrap_or(need),
                    ),
                    // The first point kept is a change only if the sample
                    // before it differs.
                    Leaf::Changes(key) => (
                        vec![(*key).clone()],
                        last_point_before(working, key, need).unwrap_or(need),
                    ),
                    Leaf::Between { node, start, .. } => {
                        // The points at `need` meet the state just before it:
                        // start the machine where that state is inactive.
                        let timeline = node.evaluate(working)?;
                        let opened = timeline
                            .at(need.saturating_sub(1))
                            .map(TimeIntervalEvent::start);
                        let anchor = opened.map_or(need, |from| from.min(need));
                        let between = &self.betweens[&start.key];
                        cuts.push((start.key.clone(), anchor));
                        (vec![between.start.clone(), between.end.clone()], anchor)
                    }
                };
                for read in keys {
                    if let Some(required) = required.get_mut(&read) {
                        *required = (*required).min(anchor);
                    }
                }
            }
        }
        for (view, cut) in cuts {
            if let Some(between) = self.betweens.get_mut(&view) {
                between.cut = cut;
            }
        }

        for (key, _) in &self.outputs {
            self.trusted.insert(key.clone(), required[key]);
        }
        let inputs: Vec<Key> = self.inputs.keys().cloned().collect();
        for key in inputs {
            self.inputs.discard_before(key.as_str(), required[&key]);
        }

        Ok(())
    }
}

impl<'a> Leaf<'a> {
    /// The keys the leaf reads, each with the pattern that reads it, if any.
    fn reads(&self) -> Vec<(&'a Key, Read<'a>)> {
        match self {
            Leaf::State(key) => vec![(*key, Read::Timeline)],
            Leaf::Records(key) => vec![(*key, Read::Records)],
            Leaf::Occurs(pattern) | Leaf::While { pattern, .. } => {
                vec![(&pattern.key, Read::Points(pattern))]
            }
            Leaf::Changes(key) => vec![(*key, Read::Any)],
            Leaf::Between { start, end, .. } => {
                vec![
                    (&start.key, Read::Points(start)),
                    (&end.key, Read::Points(end)),
                ]
            }
        }
    }
}

/// The output with every `Between` that stops at the head stopping at the end
/// of the extent, which online ends at the frontier, the head.
fn until_extent(output: &Output) -> Output {
    match output {
        Output::State(formula) => Output::State(formula.open_until_extent()),
        Output::Events(Events::Edges(formula)) => {
            Output::Events(Events::Edges(formula.open_until_extent()))
        }
        Output::Events(
            events @ (Events::Matching(_) | Events::Project { .. } | Events::Changes { .. }),
        ) => Output::Events(events.clone()),
        Output::PerEntity { by, formula } => Output::PerEntity {
            by: by.clone(),
            formula: formula.open_until_extent(),
        },
        Output::AnyEntity { by, formula } => Output::AnyEntity {
            by: by.clone(),
            formula: formula.open_until_extent(),
        },
        Output::CountEntities { by, formula } => Output::CountEntities {
            by: by.clone(),
            formula: formula.open_until_extent(),
        },
    }
}

/// The output with each `Between` reading views of its series, which the
/// processor cuts at the instant the machine of that leaf starts at, filed in
/// `betweens` by the view of its `start`.
fn with_views(
    output: &Output,
    betweens: &mut BTreeMap<Key, Between>,
    created: &mut Vec<Key>,
) -> Output {
    fn view(pattern: &Pattern, key: Key) -> Pattern {
        Pattern {
            key,
            condition: pattern.condition.clone_box(),
        }
    }

    fn walk(
        formula: &Formula,
        betweens: &mut BTreeMap<Key, Between>,
        created: &mut Vec<Key>,
    ) -> Formula {
        match formula {
            Formula::Between { start, end, open } => {
                let id = betweens.len();
                let start_view = Key::from(format!("{}\0between{id}", start.key).as_str());
                let end_view = if start.key == end.key {
                    start_view.clone()
                } else {
                    Key::from(format!("{}\0between{id}", end.key).as_str())
                };
                created.push(start_view.clone());
                betweens.insert(
                    start_view.clone(),
                    Between {
                        start: start.key.clone(),
                        end: end.key.clone(),
                        cut: Timestamp::MIN,
                    },
                );
                Formula::Between {
                    start: view(start, start_view),
                    end: view(end, end_view),
                    open: *open,
                }
            }
            Formula::State(_) | Formula::While { .. } | Formula::Occurs(_) => formula.clone(),
            Formula::Not(inner) => Formula::Not(Box::new(walk(inner, betweens, created))),
            Formula::And(formulas) => Formula::And(
                formulas
                    .iter()
                    .map(|part| walk(part, betweens, created))
                    .collect(),
            ),
            Formula::Or(formulas) => Formula::Or(
                formulas
                    .iter()
                    .map(|part| walk(part, betweens, created))
                    .collect(),
            ),
            Formula::Shift {
                formula,
                alpha,
                beta,
            } => Formula::Shift {
                formula: Box::new(walk(formula, betweens, created)),
                alpha: *alpha,
                beta: *beta,
            },
        }
    }

    match output {
        Output::State(formula) => Output::State(walk(formula, betweens, created)),
        Output::Events(Events::Edges(formula)) => {
            Output::Events(Events::Edges(walk(formula, betweens, created)))
        }
        Output::PerEntity { by, formula } => Output::PerEntity {
            by: by.clone(),
            formula: walk(formula, betweens, created),
        },
        Output::AnyEntity { by, formula } => Output::AnyEntity {
            by: by.clone(),
            formula: walk(formula, betweens, created),
        },
        Output::CountEntities { by, formula } => Output::CountEntities {
            by: by.clone(),
            formula: walk(formula, betweens, created),
        },
        points @ Output::Events(
            Events::Matching(_) | Events::Project { .. } | Events::Changes { .. },
        ) => points.clone(),
    }
}

/// The leaves of an output with the reach of each: the sum, along the path from
/// the root, of the larger bound of every shift, never below zero.
fn leaves(output: &Output) -> Vec<(Leaf<'_>, Timestamp)> {
    fn walk<'a>(formula: &'a Formula, reach: Timestamp, found: &mut Vec<(Leaf<'a>, Timestamp)>) {
        match formula {
            Formula::State(key) => found.push((Leaf::State(key), reach)),
            Formula::Between { start, end, .. } => found.push((
                Leaf::Between {
                    node: formula,
                    start,
                    end,
                },
                reach,
            )),
            Formula::While { pattern, timeout } => found.push((
                Leaf::While {
                    pattern,
                    timeout: *timeout,
                },
                reach,
            )),
            Formula::Occurs(pattern) => found.push((Leaf::Occurs(pattern), reach)),
            Formula::Not(inner) => walk(inner, reach, found),
            Formula::And(formulas) | Formula::Or(formulas) => {
                for formula in formulas {
                    walk(formula, reach, found);
                }
            }
            Formula::Shift {
                formula,
                alpha,
                beta,
            } => walk(
                formula,
                reach.saturating_add(*alpha.max(beta).max(&0)),
                found,
            ),
        }
    }

    let mut found = Vec::new();
    match output {
        Output::State(formula)
        | Output::Events(Events::Edges(formula))
        | Output::PerEntity { formula, .. }
        | Output::AnyEntity { formula, .. }
        | Output::CountEntities { formula, .. } => walk(formula, 0, &mut found),
        Output::Events(Events::Matching(pattern)) => found.push((Leaf::Occurs(pattern), 0)),
        Output::Events(Events::Project { key, .. }) => found.push((Leaf::Records(key), 0)),
        Output::Events(Events::Changes { key }) => found.push((Leaf::Changes(key), 0)),
    }

    found
}

/// The input a read of `key` reaches: `key` itself, or what the `Events::Changes`
/// output under it reads, and so on; `None` when `key` is an output of another
/// kind, which is not an input.
fn source<'a>(outputs: &'a [(Key, Output)], mut key: &'a Key) -> Option<&'a Key> {
    loop {
        match outputs.iter().find(|(output_key, _)| output_key == key) {
            None => return Some(key),
            Some((_, Output::Events(Events::Changes { key: inner }))) => key = inner,
            Some(_) => return None,
        }
    }
}

/// An empty input of the shape and value type a declaration asks for.
fn empty_input(key: &Key, input: &Input) -> Sequence {
    let label = key.as_str();
    match input {
        Input::Timeline => TimeIntervalSequence::new(label).into(),
        Input::Points(Kind::Bool) => TimePointSequence::<bool>::new(label).into(),
        Input::Points(Kind::Integer) => TimePointSequence::<i64>::new(label).into(),
        Input::Points(Kind::Float) => TimePointSequence::<f64>::new(label).into(),
        Input::Points(Kind::Text) => TimePointSequence::<String>::new(label).into(),
        Input::Points(Kind::Value) | Input::Records(_) => {
            TimePointSequence::<Value>::new(label).into()
        }
        Input::Edges => TimePointSequence::<Edge>::new(label).into(),
    }
}

/// File an empty live input for what a leaf reads under `key`, or check it
/// against the one already filed.
fn declare<'a>(
    declared: &mut BTreeMap<&'a Key, Sequence>,
    key: &'a Key,
    read: Read<'_>,
) -> Result<(), ProcessError> {
    let Some(existing) = declared.get(key) else {
        let sequence = match read {
            Read::Timeline => Sequence::from(TimeIntervalSequence::new(key.as_str())),
            Read::Any => Sequence::from(TimePointSequence::<()>::new(key.as_str())),
            Read::Points(pattern) => Sequence::from(pattern.condition.empty_points(key.as_str())),
            Read::Records => Sequence::from(TimePointSequence::<Value>::new(key.as_str())),
        };
        declared.insert(key, sequence);
        return Ok(());
    };

    let found = match read {
        Read::Any => {
            return match existing {
                Sequence::TimePoint(_) => Ok(()),
                Sequence::TimeInterval(_) => Err(ProcessError::NotPoints { key: key.clone() }),
            };
        }
        Read::Timeline => {
            return match existing {
                Sequence::TimeInterval(_) => Ok(()),
                Sequence::TimePoint(_) => Err(ProcessError::NotATimeline { key: key.clone() }),
            };
        }
        Read::Points(pattern) => pattern.value_type_name(),
        Read::Records => any::type_name::<Value>(),
    };
    match existing {
        Sequence::TimeInterval(_) => Err(ProcessError::NotPoints { key: key.clone() }),
        Sequence::TimePoint(points) if points.value_type_name() == found => Ok(()),
        Sequence::TimePoint(points) => Err(ProcessError::ValueTypeMismatch {
            key: key.clone(),
            expected: points.value_type_name(),
            found,
        }),
    }
}

/// The error a refused recording is, for the input `key`; `span` says whether
/// a span was being recorded.
fn refused(error: RecordError, key: &str, span: bool) -> ProcessError {
    let key = Key::from(key);

    match error {
        RecordError::Unknown { .. } | RecordError::NotLive { .. } => {
            ProcessError::UnknownKey { key }
        }
        RecordError::ShapeMismatch { .. } if span => ProcessError::NotATimeline { key },
        RecordError::ShapeMismatch { .. } => ProcessError::NotPoints { key },
        RecordError::ValueTypeMismatch {
            expected, found, ..
        } => ProcessError::ValueTypeMismatch {
            key,
            expected,
            found,
        },
        RecordError::SchemaMismatch { error, .. } => ProcessError::SchemaMismatch { key, error },
        RecordError::Late {
            timestamp,
            frontier,
            ..
        } => ProcessError::Late {
            key,
            timestamp,
            frontier,
        },
    }
}

/// Cut a series to the instants from `from` on, data and extent.
fn cut_before(sequence: &mut Sequence, from: Timestamp) {
    if from == Timestamp::MIN {
        return;
    }

    let kept = Extent::of_range(&(from..));
    match sequence {
        Sequence::TimePoint(points) => points.as_erased_mut().observe(&kept),
        Sequence::TimeInterval(timeline) => timeline.observe(&kept),
    }
}

/// The instant of the last point the pattern reads at or before `instant`.
fn last_sample(
    working: &SequenceSystem,
    pattern: &Pattern,
    instant: Timestamp,
) -> Option<Timestamp> {
    let Some(Sequence::TimePoint(points)) = working.get(pattern.key.as_str()) else {
        return None;
    };

    pattern
        .condition
        .marks(points)?
        .into_iter()
        .rev()
        .map(|(timestamp, _)| timestamp)
        .find(|&timestamp| timestamp <= instant)
}

/// The instant of the last point of the series `key` strictly before `instant`.
fn last_point_before(working: &SequenceSystem, key: &Key, instant: Timestamp) -> Option<Timestamp> {
    match working.get(key.as_str()) {
        Some(Sequence::TimePoint(points)) => points.last_timestamp_before(instant),
        _ => None,
    }
}

/// Push the emissions of `sequence`, the result of `output`, from `from` up to
/// `below`, or up to the end of time when there is no bound.
fn emit(
    key: &Key,
    output: &Output,
    sequence: &Sequence,
    from: Timestamp,
    below: Option<Timestamp>,
    emissions: &mut Vec<Emission>,
) {
    let wanted =
        |timestamp: Timestamp| timestamp >= from && below.is_none_or(|bound| timestamp < bound);
    let mut push = |item: Emitted| {
        if wanted(item.timestamp()) {
            emissions.push(Emission {
                key: key.clone(),
                item,
            });
        }
    };

    match (output, sequence) {
        (
            Output::State(_) | Output::PerEntity { .. } | Output::AnyEntity { .. },
            Sequence::TimeInterval(timeline),
        ) => {
            for span in timeline {
                push(Emitted::Edge(TimePointEvent::new(
                    span.start(),
                    Edge::Start,
                )));
                push(Emitted::Edge(TimePointEvent::new(span.end(), Edge::End)));
            }
        }
        (Output::Events(Events::Edges(_)), Sequence::TimePoint(points)) => {
            for edge in points.downcast_ref::<Edge>().into_iter().flatten() {
                push(Emitted::Edge(*edge));
            }
        }
        (
            Output::Events(Events::Matching(_) | Events::Project { .. } | Events::Changes { .. })
            | Output::CountEntities { .. },
            Sequence::TimePoint(points),
        ) => {
            for point in points.events_from(from) {
                push(Emitted::Point(point));
            }
        }
        // An output is evaluated to the shape it denotes.
        _ => {}
    }
}
