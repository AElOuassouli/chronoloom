//! Live members of a system: events in time order, late events refused, and a
//! frontier that moves forward.

#[path = "common/random.rs"]
mod common;

use std::collections::BTreeSet;

use chronoloom::{
    Batch, Condition, Extent, Formula, Pattern, Processor, RecordError, SequenceSystem,
    TimeIntervalSequence, TimePointEvent, TimePointSequence,
};

use common::{bounds, extent, for_each_seed, span, spans, Random, DOMAIN_END, DOMAIN_START};

/// Seeds per property.
const CASES: u64 = 500;

/// Steps of one run of the property.
const STEPS: u64 = 40;

/// The keys of the live members in the property: two point sequences and a
/// timeline.
const KEYS: [&str; 3] = ["a", "b", "c"];

/// What the property expects of one live member, kept independently of the
/// system.
struct Expected {
    frontier: i64,
    observed: BTreeSet<i64>,
    points: Vec<(i64, u64)>,
    spans: Vec<(i64, i64)>,
}

/// The instants in `[start, end)` that the given points and spans cover: what
/// an outage over that range still observes.
fn data_instants(
    points: &[(i64, u64)],
    spans: &[(i64, i64)],
    start: i64,
    end: i64,
) -> BTreeSet<i64> {
    let mut covered: BTreeSet<i64> = points
        .iter()
        .map(|&(timestamp, _)| timestamp)
        .filter(|timestamp| (start..end).contains(timestamp))
        .collect();
    for &(from, to) in spans {
        covered.extend(from.max(start)..to.min(end));
    }
    covered
}

fn instants() -> std::ops::Range<i64> {
    (DOMAIN_START - 4)..(DOMAIN_END + 40)
}

fn live_system(random: &mut Random) -> (SequenceSystem, Vec<Expected>) {
    let mut system = SequenceSystem::new();
    let mut expected = Vec::new();

    for key in KEYS {
        let from = random.between(DOMAIN_START, DOMAIN_END / 2);
        let before = extent(random);
        let observed = instants()
            .filter(|&instant| before.contains(instant) && instant < from)
            .collect();

        if key == "c" {
            let timeline = TimeIntervalSequence::new(key).with_observed(before);
            system.insert_live(key, timeline, from);
        } else {
            let points = TimePointSequence::<u64>::new(key).with_observed(before);
            system.insert_live(key, points, from);
        }
        expected.push(Expected {
            frontier: from,
            observed,
            points: Vec::new(),
            spans: Vec::new(),
        });
    }

    (system, expected)
}

fn check(system: &SequenceSystem, expected: &[Expected]) {
    for (key, expected) in KEYS.into_iter().zip(expected) {
        assert_eq!(system.frontier_of(key), Some(expected.frontier));

        let observed = system.get(key).expect("the member exists").observed();
        for instant in instants() {
            assert_eq!(
                observed.contains(instant),
                expected.observed.contains(&instant),
                "{key} at {instant}"
            );
        }

        if key == "c" {
            let timeline = system.intervals(key).expect("a timeline");
            assert_eq!(bounds(timeline), expected.spans);
        } else {
            let points = system.points::<u64>(key).expect("points of u64");
            let recorded: Vec<(i64, u64)> = points
                .iter()
                .map(|event| (event.timestamp(), *event.value()))
                .collect();
            assert_eq!(recorded, expected.points);
        }
    }
}

#[test]
fn live_members_follow_the_events_and_advances_applied_to_them() {
    for_each_seed(CASES, |random, _| {
        let (mut system, mut expected) = live_system(random);
        let mut earliest_frontier = system.frontier().expect("three live members");
        let mut order = 0;

        for _ in 0..STEPS {
            let index = random.below(3) as usize;
            let key = KEYS[index];
            let before = system.clone();

            match random.below(6) {
                0..=2 if key != "c" => {
                    let timestamp = random.between(DOMAIN_START, DOMAIN_END);
                    order += 1;
                    let outcome = system.record_point(key, TimePointEvent::new(timestamp, order));
                    if timestamp < expected[index].frontier {
                        assert_eq!(
                            outcome,
                            Err(RecordError::Late {
                                key: key.to_owned(),
                                timestamp,
                                frontier: expected[index].frontier,
                            })
                        );
                        assert_eq!(system, before);
                    } else {
                        assert_eq!(outcome, Ok(()));
                        expected[index].points.push((timestamp, order));
                        expected[index]
                            .points
                            .sort_by_key(|&(timestamp, _)| timestamp);
                    }
                }
                0..=2 => {
                    for drawn in spans(random, 1) {
                        let bounds_drawn = drawn.bounds();
                        let start = bounds_drawn.0;
                        let outcome = system.record_span(key, drawn);
                        if start < expected[index].frontier {
                            assert!(matches!(outcome, Err(RecordError::Late { .. })));
                            assert_eq!(system, before);
                        } else {
                            assert_eq!(outcome, Ok(()));
                            let mut all = expected[index].spans.clone();
                            all.push(bounds_drawn);
                            let merged = TimeIntervalSequence::from_spans(
                                "c",
                                all.into_iter()
                                    .map(|(start, end)| span(start, end))
                                    .collect(),
                            );
                            expected[index].spans = bounds(&merged);
                        }
                    }
                }
                3 | 4 => {
                    let frontier = expected[index].frontier;
                    let watermark = frontier + random.between(-2, 9);
                    let observed = random.chance(1, 2);
                    let outcome = if observed {
                        system.advance(key, watermark)
                    } else {
                        system.advance_unobserved(key, watermark)
                    };
                    assert_eq!(outcome, Ok(()));
                    if watermark > frontier {
                        let seen = if observed {
                            (frontier..watermark).collect()
                        } else {
                            data_instants(
                                &expected[index].points,
                                &expected[index].spans,
                                frontier,
                                watermark,
                            )
                        };
                        expected[index].observed.extend(seen);
                        expected[index].frontier = watermark;
                    }
                }
                _ => {
                    let watermark = expected[index].frontier + random.between(0, 6);
                    system.advance_all(watermark);
                    for member in &mut expected {
                        if watermark > member.frontier {
                            member.observed.extend(member.frontier..watermark);
                            member.frontier = watermark;
                        }
                    }
                }
            }

            let frontier = system.frontier().expect("three live members");
            assert!(frontier >= earliest_frontier);
            assert_eq!(
                Some(frontier),
                expected.iter().map(|member| member.frontier).min()
            );
            earliest_frontier = frontier;
        }

        check(&system, &expected);
    });
}

fn machine() -> SequenceSystem {
    let mut machine = SequenceSystem::new();
    machine.insert_live("live", TimePointSequence::<u64>::new("live"), 10);
    machine.insert("batch", TimePointSequence::<u64>::new("batch"));
    machine.insert_live("up", TimeIntervalSequence::new("up"), 10);
    machine
}

#[test]
fn a_late_event_inside_a_batch_records_nothing() {
    let mut system = machine();
    let before = system.clone();

    let mut batch = Batch::new();
    batch
        .points("batch", [TimePointEvent::new(-5, 1_u64)])
        .points(
            "live",
            [TimePointEvent::new(12, 2_u64), TimePointEvent::new(9, 3)],
        )
        .spans("up", [span(10, 20)]);

    assert_eq!(
        system.record_batch(batch),
        Err(RecordError::Late {
            key: "live".to_owned(),
            timestamp: 9,
            frontier: 10,
        })
    );
    assert_eq!(system, before);

    let mut late_span = Batch::new();
    late_span.spans("up", [span(12, 14), span(8, 12)]);
    assert!(matches!(
        system.record_batch(late_span),
        Err(RecordError::Late { timestamp: 8, .. })
    ));
    assert_eq!(system, before);
}

#[test]
fn the_frontier_is_the_minimum_and_batch_members_never_reject() {
    let mut system = machine();
    assert_eq!(system.frontier(), Some(10));
    assert_eq!(system.frontier_of("batch"), None);
    assert_eq!(system.frontier_of("missing"), None);
    assert_eq!(SequenceSystem::new().frontier(), None);

    assert_eq!(
        system.record_point("batch", TimePointEvent::new(-100, 1_u64)),
        Ok(())
    );
    assert!(system
        .get("batch")
        .is_some_and(|member| member.observed().contains(-100)));

    system.advance("live", 30).unwrap();
    assert_eq!(system.frontier(), Some(10));
    system.advance("up", 20).unwrap();
    assert_eq!(system.frontier(), Some(20));
    system.advance("up", 5).unwrap();
    assert_eq!(system.frontier(), Some(20));

    system.advance_all(25);
    assert_eq!(system.frontier_of("live"), Some(30));
    assert_eq!(system.frontier_of("up"), Some(25));
    assert_eq!(system.frontier_of("batch"), None);

    assert_eq!(
        system.advance("batch", 50),
        Err(RecordError::NotLive {
            key: "batch".to_owned()
        })
    );
    assert!(matches!(
        system.advance("missing", 50),
        Err(RecordError::Unknown { .. })
    ));
}

#[test]
fn recording_never_grows_the_extent_past_the_frontier() {
    let mut system = machine();

    // At the frontier is on time, and the span may reach past it.
    system
        .record_point("live", TimePointEvent::new(10, 1_u64))
        .unwrap();
    system.record_span("up", span(12, 40)).unwrap();

    let points = system.points::<u64>("live").unwrap();
    assert_eq!(points.len(), 1);
    assert!(!points.is_observed(10));

    let up = system.intervals("up").unwrap();
    assert_eq!(bounds(up), [(12, 40)]);
    assert!(!up.is_observed(20));

    // Advancing is what observes.
    system.advance("up", 30).unwrap();
    let up = system.intervals("up").unwrap();
    assert!(up.is_observed(29) && !up.is_observed(30));
}

#[test]
fn an_unobserved_advance_leaves_a_gap_until_the_next_advance() {
    let mut system = machine();

    system.advance_unobserved("up", 20).unwrap();
    system.advance("up", 30).unwrap();

    let up = system.intervals("up").unwrap();
    assert!(up.is_observed(9));
    assert!(!up.is_observed(10) && !up.is_observed(19));
    assert!(up.is_observed(20) && up.is_observed(29));
    assert!(!up.is_observed(30));
    assert_eq!(system.frontier_of("up"), Some(30));
}

#[test]
fn inserting_live_narrows_the_extent_and_equality_ignores_the_frontier() {
    let mut live = SequenceSystem::new();
    live.insert_live("up", TimeIntervalSequence::new("up"), 10);

    let up = live.intervals("up").unwrap();
    assert!(up.is_observed(9) && !up.is_observed(10));

    let mut batch = SequenceSystem::new();
    batch.insert(
        "up",
        TimeIntervalSequence::new("up").with_observed(Extent::from_spans(vec![span(i64::MIN, 10)])),
    );
    assert_eq!(live, batch);
    assert_ne!(live.frontier_of("up"), batch.frontier_of("up"));

    // Replacing a live member by a batch one makes it a batch member.
    live.insert("up", TimeIntervalSequence::new("up"));
    assert_eq!(live.frontier_of("up"), None);
}

/// What the drain property expects of one live member, kept independently of
/// the system: everything accepted so far, drained or not.
#[derive(Default)]
struct Accepted {
    frontier: i64,
    observed: BTreeSet<i64>,
    points: Vec<(i64, u64)>,
    spans: Vec<(i64, i64)>,
}

/// The instants an extent covers, over the instants the property looks at.
fn covered(extent: &Extent) -> BTreeSet<i64> {
    instants()
        .filter(|&instant| extent.contains(instant))
        .collect()
}

#[test]
fn draining_hands_over_exactly_what_is_final_and_loses_nothing() {
    for_each_seed(CASES, |random, _| {
        let mut system = SequenceSystem::new();
        let mut accepted: Vec<Accepted> = Vec::new();
        for key in KEYS {
            if key == "c" {
                system.insert_live(key, TimeIntervalSequence::new(key), DOMAIN_START);
            } else {
                system.insert_live(key, TimePointSequence::<u64>::new(key), DOMAIN_START);
            }
            accepted.push(Accepted {
                frontier: DOMAIN_START,
                observed: instants()
                    .filter(|&instant| instant < DOMAIN_START)
                    .collect(),
                ..Accepted::default()
            });
            system
                .member_metadata_mut(key)
                .unwrap()
                .insert("id".into(), key.into());
        }
        system.metadata_mut().insert("unit".into(), "ms".into());
        system.insert("batch", TimePointSequence::<u64>::new("batch"));

        let mut drained_points: Vec<Vec<(i64, u64)>> = vec![Vec::new(); 3];
        let mut drained_spans: Vec<Vec<(i64, i64)>> = vec![Vec::new(); 3];
        let mut drained_observed: Vec<BTreeSet<i64>> = vec![BTreeSet::new(); 3];
        let mut order = 0;

        for _ in 0..STEPS {
            let index = random.below(3) as usize;
            let key = KEYS[index];

            match random.below(5) {
                0 | 1 if key != "c" => {
                    let timestamp = random.between(DOMAIN_START, DOMAIN_END);
                    order += 1;
                    let event = TimePointEvent::new(timestamp, order);
                    if system.record_point(key, event).is_ok() {
                        accepted[index].points.push((timestamp, order));
                    }
                }
                0 | 1 => {
                    for drawn in spans(random, 1) {
                        let bounds_drawn = drawn.bounds();
                        if system.record_span(key, drawn).is_ok() {
                            accepted[index].spans.push(bounds_drawn);
                        }
                    }
                }
                2 => {
                    let frontier = accepted[index].frontier;
                    let watermark = frontier + random.between(0, 9);
                    if random.chance(1, 2) {
                        system.advance(key, watermark).unwrap();
                        accepted[index].observed.extend(frontier..watermark);
                    } else {
                        system.advance_unobserved(key, watermark).unwrap();
                        let seen = data_instants(
                            &accepted[index].points,
                            &accepted[index].spans,
                            frontier,
                            watermark,
                        );
                        accepted[index].observed.extend(seen);
                    }
                    accepted[index].frontier = frontier.max(watermark);
                }
                _ => {
                    let before = system.clone();
                    let drained = system.drain_finalised();

                    assert_eq!(drained.metadata(), system.metadata());
                    assert!(!drained.contains("batch"));
                    assert_eq!(system.get("batch"), before.get("batch"));
                    for (index, key) in KEYS.into_iter().enumerate() {
                        let frontier = accepted[index].frontier;
                        assert_eq!(system.frontier_of(key), Some(frontier));
                        assert_eq!(drained.frontier_of(key), None);
                        assert_eq!(drained.member_metadata(key), before.member_metadata(key));
                        assert_eq!(system.member_metadata(key), before.member_metadata(key));

                        let whole = before.get(key).unwrap().observed();
                        let part = drained.get(key).unwrap().observed();
                        let rest = system.get(key).unwrap().observed();
                        let part = covered(part);
                        let rest = covered(rest);
                        assert!(part.is_disjoint(&rest));
                        let together: BTreeSet<i64> = part.union(&rest).copied().collect();
                        assert_eq!(together, covered(whole), "{key}");
                        drained_observed[index].extend(part);

                        if key == "c" {
                            let part = bounds(drained.intervals(key).unwrap());
                            let rest = bounds(system.intervals(key).unwrap());
                            assert!(part.iter().all(|&(_, end)| end < frontier));
                            assert!(rest.iter().all(|&(_, end)| end >= frontier));
                            drained_spans[index].extend(part);
                        } else {
                            let take = |system: &SequenceSystem| -> Vec<(i64, u64)> {
                                system
                                    .points::<u64>(key)
                                    .unwrap()
                                    .iter()
                                    .map(|event| (event.timestamp(), *event.value()))
                                    .collect()
                            };
                            let part = take(&drained);
                            let rest = take(&system);
                            assert!(part.iter().all(|&(timestamp, _)| timestamp < frontier));
                            assert!(rest.iter().all(|&(timestamp, _)| timestamp >= frontier));
                            drained_points[index].extend(part);
                        }
                    }
                }
            }
        }

        // Nothing is lost: what was drained, plus what remains, is everything
        // that was accepted, extent included, and no instant is observed twice.
        for (index, key) in KEYS.into_iter().enumerate() {
            let expected = &accepted[index];
            let rest = covered(system.get(key).unwrap().observed());
            assert!(drained_observed[index].is_disjoint(&rest));
            let together: BTreeSet<i64> = drained_observed[index].union(&rest).copied().collect();
            assert_eq!(together, expected.observed, "{key}");

            if key == "c" {
                let mut all = drained_spans[index].clone();
                all.extend(bounds(system.intervals(key).unwrap()));
                let merged = TimeIntervalSequence::from_spans(
                    "c",
                    all.into_iter()
                        .map(|(start, end)| span(start, end))
                        .collect(),
                );
                let batch = TimeIntervalSequence::from_spans(
                    "c",
                    expected
                        .spans
                        .iter()
                        .map(|&(start, end)| span(start, end))
                        .collect(),
                );
                assert_eq!(bounds(&merged), bounds(&batch));
            } else {
                let mut all = drained_points[index].clone();
                all.extend(
                    system
                        .points::<u64>(key)
                        .unwrap()
                        .iter()
                        .map(|event| (event.timestamp(), *event.value())),
                );
                all.sort_by_key(|&(timestamp, _)| timestamp);
                let mut batch = expected.points.clone();
                batch.sort_by_key(|&(timestamp, _)| timestamp);
                assert_eq!(all, batch);
            }
        }
    });
}

#[test]
fn an_open_span_stays_whole_and_a_later_span_never_touches_a_drained_one() {
    let mut system = SequenceSystem::new();
    system.insert_live("up", TimeIntervalSequence::new("up"), 0);
    system.record_span("up", span(2, 5)).unwrap();
    system.record_span("up", span(8, 30)).unwrap();
    system.advance("up", 20).unwrap();

    let drained = system.drain_finalised();
    assert_eq!(bounds(drained.intervals("up").unwrap()), [(2, 5)]);
    assert_eq!(bounds(system.intervals("up").unwrap()), [(8, 30)]);

    // The drained part is observed up to where the open span starts, and the
    // member from there to its frontier.
    let part = drained.intervals("up").unwrap();
    assert!(part.is_observed(7) && !part.is_observed(8));
    let rest = system.intervals("up").unwrap();
    assert!(!rest.is_observed(7) && rest.is_observed(8) && rest.is_observed(19));
    assert!(!rest.is_observed(20));
    assert_eq!(system.frontier_of("up"), Some(20));

    // A span at the frontier is on time and merges with the open one.
    system.record_span("up", span(30, 35)).unwrap();
    assert_eq!(bounds(system.intervals("up").unwrap()), [(8, 35)]);
    assert!(system.record_span("up", span(6, 9)).is_err());
}

#[test]
fn a_member_with_nothing_final_is_present_and_keeps_what_it_observed() {
    let mut system = SequenceSystem::new();
    system.insert_live("points", TimePointSequence::<u64>::new("points"), 0);
    system.insert_live("up", TimeIntervalSequence::new("up"), 0);
    system.insert_live("silent", TimePointSequence::<u64>::new("silent"), 5);
    system
        .record_point("points", TimePointEvent::new(50, 1_u64))
        .unwrap();
    system.advance("points", 10).unwrap();
    system.advance("up", 10).unwrap();

    let drained = system.drain_finalised();
    assert_eq!(drained.points::<u64>("points").unwrap().len(), 0);
    assert!(drained.points::<u64>("points").unwrap().is_observed(9));
    assert_eq!(system.points::<u64>("points").unwrap().len(), 1);

    assert_eq!(drained.intervals("up").unwrap().len(), 0);
    assert!(drained.intervals("up").unwrap().is_observed(9));

    // The silent member observed everything before 5 and was handed that.
    let silent = drained.points::<u64>("silent").unwrap();
    assert!(silent.is_empty() && silent.is_observed(4));

    // Draining again hands over empty, unobserved parts: they say nothing.
    let again = system.drain_finalised();
    for key in ["points", "up", "silent"] {
        assert!(!again.get(key).unwrap().observed().contains(0));
        assert!(!again.get(key).unwrap().observed().contains(9));
    }
    assert_eq!(system.frontier_of("silent"), Some(5));
}

#[test]
fn draining_is_per_member_and_leaves_batch_members_and_their_metadata() {
    let mut system = machine();
    system
        .record_point("batch", TimePointEvent::new(-5, 1_u64))
        .unwrap();
    system
        .record_point("live", TimePointEvent::new(35, 2_u64))
        .unwrap();
    system
        .record_point("live", TimePointEvent::new(45, 3_u64))
        .unwrap();
    system.advance("live", 40).unwrap();
    system.advance("up", 20).unwrap();
    system.metadata_mut().insert("unit".into(), "ms".into());
    system
        .member_metadata_mut("live")
        .unwrap()
        .insert("id".into(), "7".into());
    system
        .member_metadata_mut("batch")
        .unwrap()
        .insert("id".into(), "8".into());

    let drained = system.drain_finalised();

    assert_eq!(drained.len(), 2);
    assert!(!drained.contains("batch"));
    assert_eq!(drained.points::<u64>("live").unwrap().len(), 1);
    assert_eq!(
        drained.metadata().get("unit").map(String::as_str),
        Some("ms")
    );
    assert_eq!(
        drained
            .member_metadata("live")
            .unwrap()
            .get("id")
            .map(String::as_str),
        Some("7")
    );
    assert_eq!(
        system
            .member_metadata("live")
            .unwrap()
            .get("id")
            .map(String::as_str),
        Some("7")
    );
    assert_eq!(
        system.metadata().get("unit").map(String::as_str),
        Some("ms")
    );

    assert_eq!(system.points::<u64>("batch").unwrap().len(), 1);
    assert_eq!(
        system
            .member_metadata("batch")
            .unwrap()
            .get("id")
            .map(String::as_str),
        Some("8")
    );
    assert_eq!(system.points::<u64>("live").unwrap().len(), 1);
    assert_eq!(system.frontier_of("live"), Some(40));
    assert_eq!(system.frontier_of("up"), Some(20));
    assert_eq!(drained.frontier_of("live"), None);
}

#[test]
fn a_derived_member_is_a_batch_member_and_rederives_from_what_is_left() {
    let mut system = SequenceSystem::new();
    system.insert_live("motion", TimePointSequence::<&str>::new("motion"), 0);
    for (timestamp, value) in [(2, "start"), (6, "end"), (10, "start"), (14, "end")] {
        system
            .record_point("motion", TimePointEvent::new(timestamp, value))
            .unwrap();
    }
    system.advance("motion", 12).unwrap();

    let present = Formula::between(
        Pattern::new("motion", Condition::Equals("start")),
        Pattern::new("motion", Condition::Equals("end")),
    );
    Processor::new()
        .with("present", present)
        .apply(&mut system)
        .unwrap();
    assert_eq!(
        bounds(system.intervals("present").unwrap()),
        [(2, 6), (10, 12)]
    );
    let lineage = system.lineage("present").cloned();
    assert!(lineage.is_some());

    let drained = system.drain_finalised();
    assert!(!drained.contains("present"));
    assert_eq!(drained.points::<&str>("motion").unwrap().len(), 3);

    // The derived member keeps its lineage and its data.
    assert_eq!(system.lineage("present").cloned(), lineage);
    assert_eq!(
        bounds(system.intervals("present").unwrap()),
        [(2, 6), (10, 12)]
    );

    // Deriving it again reads what is left of its input.
    system.rederive("present").unwrap();
    assert!(system.lineage("present").is_some());
    // Its start was drained, so the end left over is an orphan: no span.
    assert!(bounds(system.intervals("present").unwrap()).is_empty());
}

#[test]
fn an_outage_keeps_the_data_it_passes_over_inside_the_extent() {
    let mut system = SequenceSystem::new();
    system.insert_live("door", TimePointSequence::<i64>::new("door"), 10);
    system.insert_live("up", TimeIntervalSequence::new("up"), 10);

    // Recorded ahead of the frontier, then an outage passes over them.
    system
        .record_point("door", TimePointEvent::new(12, 1_i64))
        .unwrap();
    system
        .record_point("door", TimePointEvent::new(13, 2_i64))
        .unwrap();
    system.record_span("up", span(14, 16)).unwrap();
    system.advance_unobserved("door", 20).unwrap();
    system.advance_unobserved("up", 20).unwrap();

    let door = system.points::<i64>("door").unwrap();
    assert!(door.is_observed(12) && door.is_observed(13));
    assert!(!door.is_observed(11) && !door.is_observed(14));
    let up = system.intervals("up").unwrap();
    assert!(up.is_observed(14) && up.is_observed(15));
    assert!(!up.is_observed(13) && !up.is_observed(16));
}
