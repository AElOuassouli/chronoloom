//! A system shaped like a real sensor dataset: one building, sensors filed under
//! id-like keys with their descriptions in the attributes, and the facts the
//! library never interprets (time unit, origin) in metadata. Each test checks
//! one way such a dataset is built or used.
//!
//! The clock is in seconds from the start of the recording, which spans
//! `[0, 600)`.

use chronoloom::stream_system::UpdateError;
use chronoloom::{
    Condition, Edge, Events, Extent, Formula, Metadata, Pattern, Processor, StreamSystem,
    TimeIntervalEvent, TimeIntervalSequence, TimePointEvent, TimePointSequence,
};

const MOTION: &str = "sensor-7f3a";
const MOTION_ACTIVE: &str = "sensor-7f3a:active";
const OCCUPANCY: &str = "sensor-2b91";
const CROWDED: &str = "sensor-2b91:crowded";
const CROSSINGS: &str = "sensor-c04d";
const FIRST_AGENT: &str = "agent-1";
const SECOND_AGENT: &str = "agent-2";
const ANY_AGENT: &str = "agents:any";

fn span(start: i64, end: i64) -> TimeIntervalEvent<()> {
    TimeIntervalEvent::span(start, end).unwrap()
}

fn recording() -> Extent {
    Extent::from_spans(vec![span(0, 600)])
}

fn bounds(timeline: &TimeIntervalSequence) -> Vec<(i64, i64)> {
    timeline.iter().map(|span| span.bounds()).collect()
}

fn extent_bounds(extent: &Extent) -> Vec<(i64, i64)> {
    extent.as_slice().iter().map(|span| span.bounds()).collect()
}

fn text(pairs: &[(i64, &str)]) -> Vec<TimePointEvent<String>> {
    pairs
        .iter()
        .map(|(timestamp, value)| TimePointEvent::new(*timestamp, String::from(*value)))
        .collect()
}

fn timestamps<T>(sequence: &TimePointSequence<T>) -> Vec<i64> {
    sequence.iter().map(|event| event.timestamp()).collect()
}

/// Edge events: motion starts and ends are stated outright.
fn motion_points() -> TimePointSequence<String> {
    TimePointSequence::from_events(
        "motion events",
        text(&[
            (100, "motion_start"),
            (250, "motion_end"),
            (400, "motion_start"),
            (460, "motion_end"),
        ]),
    )
    .with_observed(recording())
}

/// The motion state: active from each start to the next end.
fn motion_formula() -> Formula {
    let event = |name: &str| Pattern::new(MOTION, Condition::Equals(String::from(name)));
    Formula::between(event("motion_start"), event("motion_end"))
}

/// People counted in a room every minute.
fn occupancy_points() -> TimePointSequence<i64> {
    let counts = [2, 4, 6, 8, 7, 5, 3, 6, 9];
    let events = counts
        .iter()
        .enumerate()
        .map(|(index, count)| TimePointEvent::new(60 * index as i64, *count))
        .collect();
    TimePointSequence::from_events("people counted", events).with_observed(recording())
}

/// Level signal: crowded while the last count is above five, held to the end
/// of the recording.
fn crowded_formula() -> Formula {
    Formula::while_matching(Pattern::new(OCCUPANCY, Condition::Greater(5_i64)))
}

fn crossing_points() -> TimePointSequence<String> {
    TimePointSequence::from_events(
        "door crossings",
        text(&[
            (50, "in"),
            (50, "out"),
            (50, "in"),
            (120, "out"),
            (30, "in"),
        ]),
    )
    .with_observed(recording())
}

fn agent_timeline(label: &str, start: i64, end: i64) -> TimeIntervalSequence {
    TimeIntervalSequence::from_spans(label, vec![span(start, end)]).with_observed(recording())
}

/// The members derived from the others: motion, crowding and presence.
fn derivations() -> Vec<Processor> {
    vec![
        Processor::new(MOTION_ACTIVE, motion_formula()),
        Processor::new(CROWDED, crowded_formula()),
        Processor::new(
            ANY_AGENT,
            Formula::state(FIRST_AGENT).or(Formula::state(SECOND_AGENT)),
        ),
    ]
}

/// One building, every mapping of a sensor dataset at once.
fn building() -> StreamSystem {
    let mut system = StreamSystem::new();
    system.insert_stream(MOTION, motion_points()).unwrap();
    system.insert_stream(OCCUPANCY, occupancy_points()).unwrap();
    system.insert_stream(CROSSINGS, crossing_points()).unwrap();
    system
        .insert_stream(FIRST_AGENT, agent_timeline("agent 1 present", 0, 300))
        .unwrap();
    system
        .insert_stream(SECOND_AGENT, agent_timeline("agent 2 present", 200, 500))
        .unwrap();

    for processor in derivations() {
        system.add_processor(processor).unwrap();
    }

    system
}

fn describe(system: &mut StreamSystem) {
    system
        .metadata_mut()
        .insert("time_unit".into(), "seconds".into());
    system
        .metadata_mut()
        .insert("origin".into(), "2026-01-01T00:00:00Z".into());
    for (key, unit) in [
        (MOTION, "event"),
        (MOTION_ACTIVE, "state"),
        (OCCUPANCY, "people"),
        (CROWDED, "state"),
        (CROSSINGS, "event"),
        (FIRST_AGENT, "state"),
        (SECOND_AGENT, "state"),
        (ANY_AGENT, "state"),
    ] {
        system
            .member_metadata_mut(key)
            .unwrap()
            .insert("unit".into(), unit.into());
    }
}

fn unit_of(system: &StreamSystem, key: &str) -> Option<String> {
    system
        .member_metadata(key)
        .and_then(|metadata| metadata.get("unit"))
        .cloned()
}

#[test]
fn motion_edges_become_a_timeline_observed_like_the_events() {
    let mut system = building();
    system
        .add_processor(Processor::new(
            "edges",
            Events::Edges(Formula::state(MOTION_ACTIVE)),
        ))
        .unwrap();
    let active = system.intervals(MOTION_ACTIVE).unwrap();

    assert_eq!(bounds(active), [(100, 250), (400, 460)]);
    assert_eq!(extent_bounds(active.observed()), [(0, 600)]);
    // Inactive and unknown differ: the sensor watched [0, 600).
    assert!(!active.contains(300) && active.is_observed(300));
    assert!(!active.contains(700) && !active.is_observed(700));
    // The labels live in the attributes, the keys stay the sensor ids, and a
    // derived member is labelled with the formula it came from.
    assert_eq!(system.get(MOTION).unwrap().label(), "motion events");
    assert_eq!(
        active.label(),
        format!("between({MOTION} = \"motion_start\", {MOTION} = \"motion_end\")")
    );
    // And the edges of the state hand the events back.
    assert_eq!(
        timestamps(system.points::<Edge>("edges").unwrap()),
        [100, 250, 400, 460]
    );
}

#[test]
fn a_threshold_over_sampled_counts_gives_the_crowded_spans() {
    let system = building();
    let counts = system.points::<i64>(OCCUPANCY).unwrap();
    assert_eq!(counts.len(), 9);
    assert_eq!(counts.nearest(130).map(|event| *event.value()), Some(6));

    // Above five from 120 until the sample at 300 (exactly five), and again from
    // 420 until the recording ends at 600.
    let crowded = system.intervals(CROWDED).unwrap();
    assert_eq!(bounds(crowded), [(120, 300), (420, 600)]);
    assert!(crowded.contains(120) && !crowded.contains(300));
    assert_eq!(extent_bounds(crowded.observed()), [(0, 600)]);
}

#[test]
fn crossings_keep_their_timing_and_the_insertion_order_of_ties() {
    let system = building();
    let crossings = system.points::<String>(CROSSINGS).unwrap();

    // Added out of time order; stored in time order.
    assert_eq!(timestamps(crossings), [30, 50, 50, 50, 120]);
    // The three events at one instant keep the order they were added in.
    let tied: Vec<&str> = crossings
        .get(50)
        .iter()
        .map(|event| event.value().as_str())
        .collect();
    assert_eq!(tied, ["in", "out", "in"]);
}

#[test]
fn presence_is_one_member_per_agent_and_their_union() {
    let system = building();
    let first = system.intervals(FIRST_AGENT).unwrap();
    let second = system.intervals(SECOND_AGENT).unwrap();
    let any = system.intervals(ANY_AGENT).unwrap();

    assert_eq!(bounds(first), [(0, 300)]);
    assert_eq!(bounds(second), [(200, 500)]);
    assert_eq!(bounds(any), [(0, 500)]);
    assert!(any.covers_same(&first.union(second)));
    assert_eq!(any.label(), format!("{FIRST_AGENT} ∨ {SECOND_AGENT}"));
}

#[test]
fn a_dataset_stopped_early_is_not_observed_after_the_stop() {
    let mut system = building();
    let stopped_at = 450;
    system
        .observe(&Extent::from_spans(vec![span(i64::MIN, stopped_at)]))
        .unwrap();

    // Every member ends at the stop, whatever it observed before.
    for key in system.keys() {
        let observed = system.get(key.as_str()).unwrap().observed();
        assert!(observed.contains(stopped_at - 1), "{key} stops too early");
        assert!(!observed.contains(stopped_at), "{key} outlives the stop");
        assert_eq!(
            observed.as_slice().last().map(|span| span.bounds().1),
            Some(stopped_at),
            "{key}"
        );
    }

    // Before the stop there are instants observed and inactive: the sensor
    // looked and saw nothing. After it, an instant is not active either, but it
    // is not observed, so only `is_observed` tells the two apart.
    let active = system.intervals(MOTION_ACTIVE).unwrap();
    assert!(!active.contains(300) && active.is_observed(300));
    assert!(!active.contains(500) && !active.is_observed(500));
    // The data is cut at the stop: the second motion span ends there.
    assert_eq!(bounds(active), [(100, 250), (400, 450)]);
    assert!(active.contains(449) && !active.contains(450));

    let crowded = system.intervals(CROWDED).unwrap();
    assert_eq!(bounds(crowded), [(120, 300), (420, 450)]);

    // Points after the stop are dropped, those before it stay.
    let counts = system.points::<i64>(OCCUPANCY).unwrap();
    assert_eq!(counts.len(), 8);
    assert!(counts.is_observed(420) && !counts.is_observed(480));
}

#[test]
fn metadata_describes_the_system_and_each_member_and_survives_a_relabel() {
    let mut system = building();
    describe(&mut system);

    assert_eq!(
        system.metadata().get("time_unit").map(String::as_str),
        Some("seconds")
    );
    assert_eq!(
        system.metadata().get("origin").map(String::as_str),
        Some("2026-01-01T00:00:00Z")
    );
    assert_eq!(unit_of(&system, OCCUPANCY).as_deref(), Some("people"));
    assert_eq!(unit_of(&system, MOTION_ACTIVE).as_deref(), Some("state"));

    let before: Metadata = system.member_metadata(CROSSINGS).unwrap().clone();
    let data = timestamps(system.points::<String>(CROSSINGS).unwrap());
    system.relabel(CROSSINGS, "entrance traffic").unwrap();

    // Key, data and metadata stay; only the description changes.
    assert_eq!(system.get(CROSSINGS).unwrap().label(), "entrance traffic");
    assert_eq!(
        timestamps(system.points::<String>(CROSSINGS).unwrap()),
        data
    );
    assert_eq!(system.member_metadata(CROSSINGS), Some(&before));
    // A derived stream is described by its formula, and is not relabelled.
    assert_eq!(
        system.relabel(CROWDED, "busy room"),
        Err(UpdateError::Derived {
            key: CROWDED.into()
        })
    );
    assert_eq!(system.metadata().len(), 2);

    // A clone carries it all, and equality sees it.
    let copy = system.clone();
    assert_eq!(copy, system);
    system
        .member_metadata_mut(CROSSINGS)
        .unwrap()
        .insert("unit".into(), "people".into());
    assert_ne!(copy, system);
}

#[test]
fn a_playback_window_slices_every_member_and_keeps_what_describes_them() {
    let mut system = building();
    describe(&mut system);
    system.relabel(MOTION, "movement events").unwrap();

    let window = system.slice(100..400);

    assert!(window.keys().eq(system.keys()));
    assert_eq!(window.metadata(), system.metadata());
    for key in system.keys() {
        let key = key.as_str();
        assert_eq!(
            window.member_metadata(key),
            system.member_metadata(key),
            "{key}"
        );
        assert_eq!(
            window.get(key).unwrap().label(),
            system.get(key).unwrap().label(),
            "{key}"
        );
        // Every member is observed over the window and nowhere else.
        let observed = window.get(key).unwrap().observed();
        assert_eq!(extent_bounds(observed), [(100, 400)], "{key}");
    }

    assert_eq!(
        timestamps(window.points::<String>(MOTION).unwrap()),
        [100, 250]
    );
    assert_eq!(
        timestamps(window.points::<String>(CROSSINGS).unwrap()),
        [120]
    );
    assert_eq!(
        timestamps(window.points::<i64>(OCCUPANCY).unwrap()),
        [120, 180, 240, 300, 360]
    );

    assert_eq!(
        bounds(window.intervals(MOTION_ACTIVE).unwrap()),
        [(100, 250)]
    );
    assert_eq!(bounds(window.intervals(CROWDED).unwrap()), [(120, 300)]);
    assert_eq!(bounds(window.intervals(FIRST_AGENT).unwrap()), [(100, 300)]);
    assert_eq!(
        bounds(window.intervals(SECOND_AGENT).unwrap()),
        [(200, 400)]
    );
    assert_eq!(bounds(window.intervals(ANY_AGENT).unwrap()), [(100, 400)]);

    // The original is untouched.
    assert_eq!(
        bounds(system.intervals(CROWDED).unwrap()),
        [(120, 300), (420, 600)]
    );
}
