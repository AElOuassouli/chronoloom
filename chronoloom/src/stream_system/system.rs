//! The main abstraction of the crate: a system of streams and the processors
//! that derive new streams from them.

#[cfg(doc)]
use crate::processor::Processor;
use crate::sequence_system::SequenceSystem;

use super::pipeline::Pipeline;
use super::{Batch, Mode};

/// The streams of a subject and the processors that derive new streams from
/// them, kept up to date as data arrives.
///
/// # Streams and derived streams
///
/// A stream system holds two kinds of member, each under its own [`Key`]:
///
/// - **Streams**, the raw data: point sequences of any value type and
///   timelines, with their extent, metadata and schema. They are what is
///   recorded into.
/// - **Derived streams**, each the output of one [`Processor`]. They are
///   read-only and always current: whenever a stream a processor reads
///   changes, the system evaluates that processor again, and then every
///   processor that reads its output, so a derived stream never goes stale.
///   Recording into one is [`UpdateError::Derived`].
///
/// A processor may also rewrite a stream instead of deriving a new one: when
/// its output key is a stream it reads, it is a **transform** of that stream
/// (it cleans, filters or projects it). The stream then reads as its last
/// version, [`raw`](StreamSystem::raw) gives the recorded data, and recording
/// into it records into that data, after which the transforms run again.
/// Transforms of one stream chain in the order they are added, each reading
/// the version before it; every other processor reads the last version, so a
/// transform is added before the processors that read its stream
/// ([`UpdateError::Read`] otherwise). A transform keeps the shape of the
/// stream, points as points and a timeline as a timeline, though the value
/// type may change. [`transforms`](StreamSystem::transforms) lists the chain.
///
/// Reading is the same for all: [`get`](StreamSystem::get),
/// [`points`](StreamSystem::points) and [`intervals`](StreamSystem::intervals)
/// look a key up wherever it is, and [`is_derived`](StreamSystem::is_derived)
/// and [`producer`](StreamSystem::producer) tell them apart.
///
/// # Processors
///
/// A [`Processor`] is one recipe: the inputs it reads, a formula over them,
/// and the key of the one output it produces (for a per-entity formula, the
/// family of members under that key). The system holds a list of them.
/// A processor may read streams and the outputs of processors added before
/// it; reading a key the system does not hold is refused when it is added, and
/// a stream or a processor another processor reads cannot be removed. The list
/// is therefore always in an order where every processor comes after what it
/// reads, and the system evaluates it in that order.
///
/// # Modes
///
/// A stream system is in one [`Mode`], a type parameter, so that the shared
/// behaviour is written once and the mode-specific behaviour once per mode:
/// reading, metadata and the list of processors are the same in every mode,
/// while what an [`update`](StreamSystem::update) does depends on it.
///
/// - [`Batch`], the default: a dataset held whole. An update records its
///   points and evaluates again the processors that read what changed,
///   directly or through another processor's output, and no others. The
///   [`Report`] it returns names the derived streams whose content changed.
/// - [`Live`]: data arrives in time order. Each stream has a frontier, before
///   which everything is final, that watermarks in an [`Update`] move. An update
///   refuses events before the frontier of their stream, records the rest,
///   moves the frontiers, and runs the processors that read a stream whose
///   frontier moved, directly or through another's output, over the range that
///   became final only: each keeps a small running state instead of reading
///   its inputs again. What they give is appended to the derived streams, and
///   the [`LiveReport`] returns it as one [`Chunk`] per derived stream, with the
///   frontier of each. The chunks joined end to end are what batch mode gives
///   over the same data, below the frontiers. A live system does not run
///   transforms yet, and starts at an instant, [`StreamSystem::live`].
///   [`finish`](StreamSystem::finish) ends it, closing the streams at a
///   horizon so that the derived streams are the batch result over what the
///   streams hold. It keeps only what is not final once [`drain`] hands the
///   final part over as a batch system, which bounds its memory over a stream
///   that does not end: the processors carry their own state and never need
///   the data drained, while a processor [added](StreamSystem::add_processor)
///   to a live system starts over what the system still holds, so it sees
///   nothing that was drained. [`StreamSystem::into_live`] goes from batch to
///   live: the streams are cut to what lies before the instant, and the
///   processors start by stepping over it.
///
/// ```
/// use chronoloom::processor::Processor;
/// use chronoloom::stream_system::{Live, StreamSystem, Update};
/// use chronoloom::{Condition, Formula, Pattern, TimePointEvent, TimePointSequence};
///
/// let mut room: StreamSystem<Live> = StreamSystem::live(0);
/// room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))?;
/// room.add_processor(Processor::new("presence", Formula::between(
///     Pattern::new("motion", Condition::Equals("start")),
///     Pattern::new("motion", Condition::Equals("end")),
/// )))?;
///
/// let mut update = Update::new();
/// update.points("motion", [TimePointEvent::new(10, "start"), TimePointEvent::new(25, "end")]);
/// update.advance("motion", 30);
/// let report = room.update(update)?;
///
/// assert_eq!(report.chunks()["presence"].range(), 0..30);
/// assert_eq!(room.frontier("presence"), Some(30));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Every change is all or nothing: if recording, or evaluating any processor
/// it affects, fails, the system is left as it was. In this first version that
/// is done by working on a copy of the members, so a change costs a copy of the
/// data besides the evaluation.
///
/// ```
/// use chronoloom::processor::Processor;
/// use chronoloom::stream_system::{StreamSystem, Update};
/// use chronoloom::{Condition, Formula, Pattern, TimeIntervalEvent, TimeIntervalSequence};
/// use chronoloom::{TimePointEvent, TimePointSequence};
///
/// let mut room = StreamSystem::new();
/// room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))?;
/// room.insert_stream("door", TimeIntervalSequence::from_spans("door", vec![
///     TimeIntervalEvent::span(40, 50)?,
/// ]))?;
///
/// // Present between a start of motion and the next end.
/// room.add_processor(Processor::new("presence", Formula::between(
///     Pattern::new("motion", Condition::Equals("start")),
///     Pattern::new("motion", Condition::Equals("end")),
/// )))?;
/// // Occupied while present or while the door is open: reads an output.
/// room.add_processor(Processor::new(
///     "occupied",
///     Formula::state("presence").or(Formula::state("door")),
/// ))?;
///
/// // New motion: both derived streams follow.
/// let mut update = Update::new();
/// update.points("motion", [TimePointEvent::new(10, "start"), TimePointEvent::new(25, "end")]);
/// let report = room.update(update)?;
/// assert!(report.changed().contains("presence") && report.changed().contains("occupied"));
/// assert_eq!(room.intervals("presence").unwrap()[0].bounds(), (10, 25));
/// assert_eq!(room.intervals("occupied").unwrap().len(), 2);
///
/// // Derived streams are read-only.
/// let mut into_derived = Update::new();
/// into_derived.spans("presence", [TimeIntervalEvent::span(0, 5)?]);
/// assert!(room.update(into_derived).is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// [`Chunk`]: super::Chunk
/// [`drain`]: StreamSystem::drain
/// [`Live`]: super::Live
/// [`LiveReport`]: super::LiveReport
/// [`Report`]: super::Report
/// [`Update`]: super::Update
/// [`UpdateError::Derived`]: super::UpdateError::Derived
/// [`UpdateError::Read`]: super::UpdateError::Read
#[derive(Debug, Clone, PartialEq)]
pub struct StreamSystem<M: Mode = Batch> {
    /// Streams and derived streams together, so that a processor reads both
    /// the same way.
    pub(super) members: SequenceSystem,
    /// The recorded data of the streams that transforms rewrite: their
    /// `members` entry holds the last version.
    pub(super) raw: SequenceSystem,
    /// The processors, each after what it reads.
    pub(super) pipeline: Pipeline,
    pub(super) mode: M,
}
