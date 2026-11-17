//! Conditions, formulas and processors: what each leaf and node means, checked
//! against literal expectations and, for the three leaves that read points,
//! against definitions stated one instant at a time over random inputs.

#[path = "common/random.rs"]
mod common;

use chronoloom::{
    Condition, Edge, Events, Extent, Formula, OpenEnd, Output, Pattern, ProcessError, Processor,
    SequenceSystem, TimeIntervalSequence, TimePointEvent, TimePointSequence,
};

use common::{bounds, extent, for_each_seed, span, spans, Random, DOMAIN_END, DOMAIN_START};

/// First and one past the last instant the per-instant oracles look at: the
/// domain, and a margin on each side where nothing ever happens.
const CHECK: std::ops::Range<i64> = (DOMAIN_START - 20)..(DOMAIN_END + 20);

const CASES: u64 = 400;

fn window(start: i64, end: i64) -> Extent {
    Extent::from_spans(vec![span(start, end)])
}

fn words(label: &str, events: &[(i64, &'static str)]) -> TimePointSequence<&'static str> {
    TimePointSequence::from_events(
        label,
        events
            .iter()
            .map(|&(timestamp, word)| TimePointEvent::new(timestamp, word))
            .collect(),
    )
}

fn numbers(label: &str, events: &[(i64, i64)]) -> TimePointSequence<i64> {
    TimePointSequence::from_events(
        label,
        events
            .iter()
            .map(|&(timestamp, number)| TimePointEvent::new(timestamp, number))
            .collect(),
    )
}

fn word(key: &str, value: &'static str) -> Pattern {
    Pattern::new(key, Condition::Equals(value))
}

fn motion_between() -> Formula {
    Formula::between(word("motion", "start"), word("motion", "end"))
}

fn system_with(motion: TimePointSequence<&'static str>) -> SequenceSystem {
    let mut system = SequenceSystem::new();
    system.insert("motion", motion);
    system
}

fn motion_spans(events: &[(i64, &'static str)]) -> Vec<(i64, i64)> {
    let timeline = motion_between()
        .evaluate(&system_with(words("motion", events)))
        .unwrap();
    bounds(&timeline)
}

fn extent_bounds(extent: &Extent) -> Vec<(i64, i64)> {
    extent.as_slice().iter().map(|span| span.bounds()).collect()
}

fn timeline(label: &str, spans: &[(i64, i64)]) -> TimeIntervalSequence {
    TimeIntervalSequence::from_spans(label, spans.iter().map(|&(s, e)| span(s, e)).collect())
}

// Conditions

#[test]
fn comparisons_match_as_written() {
    assert!(Condition::Any.matches(&3));
    assert!(Condition::Equals(3).matches(&3) && !Condition::Equals(3).matches(&4));
    assert!(Condition::NotEquals(3).matches(&4) && !Condition::NotEquals(3).matches(&3));
    assert!(Condition::Less(3).matches(&2) && !Condition::Less(3).matches(&3));
    assert!(Condition::LessOrEqual(3).matches(&3) && !Condition::LessOrEqual(3).matches(&4));
    assert!(Condition::Greater(3).matches(&4) && !Condition::Greater(3).matches(&3));
    assert!(Condition::GreaterOrEqual(3).matches(&3) && !Condition::GreaterOrEqual(3).matches(&2));
    assert!(Condition::In(vec![1, 3]).matches(&3) && !Condition::In(vec![1, 3]).matches(&2));
    assert!(!Condition::<i64>::In(vec![]).matches(&2));
}

#[test]
fn within_is_half_open() {
    let range = Condition::Within { low: 5, high: 10 };
    assert!(!range.matches(&4) && range.matches(&5) && range.matches(&9) && !range.matches(&10));
}

#[test]
fn a_value_that_does_not_compare_matches_no_ordering() {
    let nan = f64::NAN;
    for condition in [
        Condition::Less(1.0),
        Condition::LessOrEqual(1.0),
        Condition::Greater(1.0),
        Condition::GreaterOrEqual(1.0),
        Condition::Within {
            low: 0.0,
            high: 2.0,
        },
        Condition::Equals(1.0),
        Condition::In(vec![1.0]),
    ] {
        assert!(!condition.matches(&nan), "{condition}");
    }
    assert!(Condition::NotEquals(1.0).matches(&nan));
    assert!(Condition::Any.matches(&nan));
}

#[test]
fn conditions_combine() {
    let moderate = Condition::And(vec![Condition::Greater(2), Condition::Less(6)]);
    assert!(moderate.matches(&3) && !moderate.matches(&6) && !moderate.matches(&2));

    let extreme = Condition::Or(vec![Condition::Less(2), Condition::Greater(6)]);
    assert!(extreme.matches(&1) && extreme.matches(&7) && !extreme.matches(&4));

    let unusual = Condition::Not(Box::new(Condition::Equals(4)));
    assert!(unusual.matches(&3) && !unusual.matches(&4));

    assert!(Condition::<i64>::And(vec![]).matches(&0));
    assert!(!Condition::<i64>::Or(vec![]).matches(&0));
}

#[test]
fn conditions_and_formulas_are_written_in_the_documented_notation() {
    assert_eq!(Condition::Equals("start").to_string(), "value = \"start\"");
    assert_eq!(
        Condition::Within { low: 5, high: 10 }.to_string(),
        "5 <= value < 10"
    );
    assert_eq!(Condition::In(vec![1, 2]).to_string(), "value in {1, 2}");
    assert_eq!(
        Condition::Not(Box::new(Condition::Or(vec![
            Condition::Less(1),
            Condition::Greater(2)
        ])))
        .to_string(),
        "¬(value < 1 ∨ value > 2)"
    );
    assert_eq!(Condition::<i64>::And(vec![]).to_string(), "⊤");

    let motion = motion_between();
    assert_eq!(
        motion.to_string(),
        "between(motion = \"start\", motion = \"end\")"
    );
    assert_eq!(
        Formula::between_with(word("a", "x"), word("b", "y"), OpenEnd::Head).to_string(),
        "between(a = \"x\", b = \"y\", open until head)"
    );

    let crowded = Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64)));
    let door = Formula::occurs(word("door", "open"));
    assert_eq!(crowded.to_string(), "while(count > 5)");
    assert_eq!(door.to_string(), "occurs(door = \"open\")");

    let mixed = crowded.clone().and(door.clone()).or(Formula::state("up"));
    assert_eq!(
        mixed.to_string(),
        "(while(count > 5) ∧ occurs(door = \"open\")) ∨ up"
    );
    assert_eq!(
        (!crowded.clone().or(door.clone())).to_string(),
        "¬(while(count > 5) ∨ occurs(door = \"open\"))"
    );
    assert_eq!(
        (!crowded.clone()).shift(1, 2).to_string(),
        "(¬while(count > 5))[1, 2]"
    );
    assert_eq!(
        door.shift(0, 2).to_string(),
        "occurs(door = \"open\")[0, 2]"
    );
    assert_eq!(Formula::And(vec![]).to_string(), "⊤");
    assert_eq!(Formula::Or(vec![]).to_string(), "⊥");
}

#[test]
fn patterns_compare_by_key_value_type_and_condition() {
    let pattern = Pattern::new("count", Condition::Greater(5_i64));
    assert_eq!(pattern, pattern.clone());
    assert_ne!(pattern, Pattern::new("count", Condition::Greater(6_i64)));
    assert_ne!(pattern, Pattern::new("other", Condition::Greater(5_i64)));
    assert_ne!(pattern, Pattern::new("count", Condition::Greater(5_u8)));
    assert_eq!(pattern.key().as_str(), "count");
    assert_eq!(pattern.value_type_name(), "i64");
    assert_eq!(
        format!("{pattern:?}"),
        "Pattern { key: Key(\"count\"), condition: Greater(5) }"
    );
}

#[test]
fn chained_and_and_or_stay_flat() {
    let (a, b, c) = (
        Formula::state("a"),
        Formula::state("b"),
        Formula::state("c"),
    );
    assert_eq!(
        a.clone().and(b.clone()).and(c.clone()),
        Formula::And(vec![a.clone(), b.clone(), c.clone()])
    );
    assert_eq!(
        a.clone().or(b.clone()).or(c.clone()),
        Formula::Or(vec![a, b, c])
    );
}

// Between

#[test]
fn between_spans_from_a_start_to_the_next_end() {
    let events = [(10, "start"), (25, "end"), (40, "start"), (50, "end")];
    let system = system_with(words("motion", &events));
    let active = motion_between().evaluate(&system).unwrap();

    assert_eq!(bounds(&active), [(10, 25), (40, 50)]);
    assert_eq!(active.observed(), &Extent::everything());
    assert_eq!(
        active.label(),
        "between(motion = \"start\", motion = \"end\")"
    );
}

#[test]
fn between_ignores_what_does_not_apply() {
    // A start while open does nothing.
    assert_eq!(
        motion_spans(&[(3, "start"), (5, "start"), (9, "end")]),
        [(3, 9)]
    );
    // An end while inactive does nothing: the formula is total.
    assert_eq!(
        motion_spans(&[(2, "end"), (4, "start"), (8, "end"), (9, "end")]),
        [(4, 8)]
    );
    // Points matching neither pattern say nothing.
    assert_eq!(
        motion_spans(&[(1, "noise"), (3, "start"), (5, "noise"), (9, "end")]),
        [(3, 9)]
    );
}

#[test]
fn between_ties_follow_the_order_of_recording() {
    // An end at the instant the span opened lasts one tick.
    assert_eq!(motion_spans(&[(7, "start"), (7, "end")]), [(7, 8)]);
    // End then start at one instant: the span carries on.
    assert_eq!(
        motion_spans(&[(3, "start"), (7, "end"), (7, "start"), (9, "end")]),
        [(3, 9)]
    );
    // Start recorded after the end: the end is an orphan.
    assert_eq!(
        motion_spans(&[(7, "end"), (7, "start"), (9, "end")]),
        [(7, 9)]
    );
}

#[test]
fn one_point_matching_both_patterns_opens_while_inactive_and_closes_while_active() {
    let both = Formula::between(
        Pattern::new("signal", Condition::<i64>::Any),
        Pattern::new("signal", Condition::<i64>::Any),
    );
    let mut system = SequenceSystem::new();
    system.insert("signal", numbers("signal", &[(1, 0), (4, 0), (6, 0)]));

    // Opens at 1, closes at 4, opens at 6, and stays open.
    assert_eq!(
        bounds(&both.evaluate(&system).unwrap()),
        [(1, 4), (6, i64::MAX)]
    );
}

#[test]
fn an_open_span_stops_at_the_end_of_its_run_of_the_extent() {
    let open = [(4, "start")];

    let forever = system_with(words("motion", &open));
    let active = motion_between().evaluate(&forever).unwrap();
    assert_eq!(bounds(&active), [(4, i64::MAX)]);

    let bounded = system_with(words("motion", &open).with_observed(window(0, 30)));
    let active = motion_between().evaluate(&bounded).unwrap();
    assert_eq!(bounds(&active), [(4, 30)]);
    assert_eq!(extent_bounds(active.observed()), [(0, 30)]);
    assert!(!active.contains(30) && !active.is_observed(30));
}

#[test]
fn an_open_span_up_to_the_head_is_unknown_from_the_head_on() {
    let head = |events: &[(i64, &'static str)], extent: Extent| {
        let system = system_with(words("motion", events).with_observed(extent));
        Formula::between_with(
            word("motion", "start"),
            word("motion", "end"),
            OpenEnd::Head,
        )
        .evaluate(&system)
        .unwrap()
    };

    // The head is one past the latest point, whichever points match.
    let active = head(&[(4, "start"), (20, "noise")], Extent::everything());
    assert_eq!(bounds(&active), [(4, 21)]);
    assert!(active.is_observed(20) && !active.is_observed(21));
    assert!(active.is_observed(-100));

    // A start at the head lasts one tick.
    let active = head(&[(4, "start")], Extent::everything());
    assert_eq!(bounds(&active), [(4, 5)]);

    // A closed span is untouched, but the formula is still unknown from the head.
    let active = head(&[(4, "start"), (9, "end")], Extent::everything());
    assert_eq!(bounds(&active), [(4, 9)]);
    assert!(!active.is_observed(10));

    // The extent and the head cut the same run.
    let active = head(&[(4, "start"), (29, "noise")], window(0, 30));
    assert_eq!(bounds(&active), [(4, 30)]);
    assert_eq!(extent_bounds(active.observed()), [(0, 30)]);

    // Nothing seen, nothing known.
    let active = head(&[], Extent::everything());
    assert!(active.is_empty() && !active.is_observed(0));
}

#[test]
fn between_reads_each_run_of_the_extent_by_itself() {
    let seen = Extent::from_spans(vec![span(0, 20), span(30, 60)]);
    let system = system_with(
        words(
            "motion",
            &[(10, "start"), (40, "end"), (45, "start"), (50, "end")],
        )
        .with_observed(seen.clone()),
    );
    let active = motion_between().evaluate(&system).unwrap();

    // Opened in the first run and left open at its end; the end at 40 finds
    // nothing open in the second run.
    assert_eq!(bounds(&active), [(10, 20), (45, 50)]);
    assert_eq!(active.observed(), &seen);
}

#[test]
fn between_two_members_is_observed_where_both_are() {
    let mut system = SequenceSystem::new();
    system.insert(
        "door",
        words("door", &[(5, "open"), (10, "open"), (60, "open")]).with_observed(window(0, 100)),
    );
    system.insert(
        "light",
        words("light", &[(5, "off"), (30, "off"), (70, "off")]).with_observed(window(0, 50)),
    );
    let formula = Formula::between(word("door", "open"), word("light", "off"));
    let active = formula.evaluate(&system).unwrap();

    // At 5 the start comes before the end, so the span lasts one tick; then
    // 10 opens another until 30. The points past 50 are outside the extent.
    assert_eq!(bounds(&active), [(5, 6), (10, 30)]);
    assert_eq!(extent_bounds(active.observed()), [(0, 50)]);
}

// While

#[test]
fn while_holds_the_last_sample() {
    let counts = [1, 7, 9, 3, 2, 6, 8, 4, 1];
    let events: Vec<(i64, i64)> = counts
        .iter()
        .enumerate()
        .map(|(index, count)| (index as i64 * 10, *count))
        .collect();
    let mut system = SequenceSystem::new();
    system.insert("count", numbers("count", &events));
    let crowded = Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64)));
    let active = crowded.evaluate(&system).unwrap();

    assert_eq!(bounds(&active), [(10, 30), (50, 70)]);
    assert_eq!(active.label(), "while(count > 5)");
    // Unknown before the first sample, known from it on.
    assert!(!active.is_observed(-1) && active.is_observed(0));
    assert_eq!(extent_bounds(active.observed()), [(0, i64::MAX)]);
}

#[test]
fn while_holds_the_last_sample_to_the_end_of_the_extent() {
    let mut system = SequenceSystem::new();
    system.insert(
        "count",
        numbers("count", &[(10, 2), (20, 9)]).with_observed(window(0, 100)),
    );
    let active = Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64)))
        .evaluate(&system)
        .unwrap();

    assert_eq!(bounds(&active), [(20, 100)]);
    assert_eq!(extent_bounds(active.observed()), [(10, 100)]);
}

#[test]
fn while_takes_the_last_recorded_of_the_samples_sharing_an_instant() {
    let crowded = Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64)));
    let spans_of = |events: &[(i64, i64)]| {
        let mut system = SequenceSystem::new();
        system.insert(
            "count",
            numbers("count", events).with_observed(window(0, 40)),
        );
        bounds(&crowded.evaluate(&system).unwrap())
    };

    assert_eq!(spans_of(&[(10, 9), (10, 2)]), []);
    assert_eq!(spans_of(&[(10, 2), (10, 9)]), [(10, 40)]);
}

#[test]
fn while_restarts_in_each_run_of_the_extent() {
    let mut system = SequenceSystem::new();
    system.insert(
        "count",
        numbers("count", &[(5, 8), (70, 8)])
            .with_observed(Extent::from_spans(vec![span(0, 50), span(60, 100)])),
    );
    let active = Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64)))
        .evaluate(&system)
        .unwrap();

    assert_eq!(bounds(&active), [(5, 50), (70, 100)]);
    assert_eq!(extent_bounds(active.observed()), [(5, 50), (70, 100)]);
}

#[test]
fn while_within_reads_a_range() {
    let mut system = SequenceSystem::new();
    system.insert(
        "level",
        numbers("level", &[(0, 4), (10, 5), (20, 9), (30, 10)]).with_observed(window(0, 50)),
    );
    let active = Formula::while_matching(Pattern::new(
        "level",
        Condition::Within {
            low: 5_i64,
            high: 10,
        },
    ))
    .evaluate(&system)
    .unwrap();

    assert_eq!(bounds(&active), [(10, 30)]);
}

// Occurs

#[test]
fn occurs_is_active_on_the_instant_of_each_matching_point() {
    let mut system = SequenceSystem::new();
    system.insert(
        "door",
        words(
            "door",
            &[(5, "open"), (9, "open"), (9, "open"), (12, "close")],
        )
        .with_observed(window(0, 20)),
    );
    let opened = Formula::occurs(word("door", "open"));
    let active = opened.evaluate(&system).unwrap();

    assert_eq!(bounds(&active), [(5, 6), (9, 10)]);
    assert_eq!(extent_bounds(active.observed()), [(0, 20)]);
    assert!(active.is_observed(7) && !active.contains(7));

    // Holding an event for three ticks is a shift of the upper bound.
    let held = opened.shift(0, 2).evaluate(&system).unwrap();
    assert_eq!(bounds(&held), [(5, 8), (9, 12)]);
}

// Combinations

fn two_states() -> SequenceSystem {
    let mut system = SequenceSystem::new();
    system.insert(
        "a",
        timeline("a", &[(0, 10), (20, 30)]).with_observed(window(0, 40)),
    );
    system.insert("b", timeline("b", &[(5, 25)]).with_observed(window(0, 50)));
    system
}

#[test]
fn the_algebra_nodes_are_the_operations_of_the_sequences() {
    let system = two_states();
    let a = system.intervals("a").unwrap();
    let b = system.intervals("b").unwrap();
    let same = |formula: Formula, expected: &TimeIntervalSequence| {
        let evaluated = formula.evaluate(&system).unwrap();
        assert!(evaluated.covers_same(expected), "{formula}");
        assert_eq!(evaluated.observed(), expected.observed(), "{formula}");
        assert_eq!(evaluated.label(), formula.to_string());
    };
    let (fa, fb) = (Formula::state("a"), Formula::state("b"));

    same(fa.clone(), a);
    same(fa.clone().and(fb.clone()), &a.intersection(b));
    same(fa.clone().or(fb.clone()), &a.union(b));
    same(!fa.clone(), &a.complement());
    same(fa.clone().shift(-2, 3), &a.transform(-2, 3).unwrap());
    same(
        !(fa.clone().and(fb.clone())).shift(1, 1),
        &a.intersection(b).transform(1, 1).unwrap().complement(),
    );
}

#[test]
fn an_empty_and_is_everywhere_and_an_empty_or_nowhere() {
    let system = SequenceSystem::new();
    let everywhere = Formula::And(vec![]).evaluate(&system).unwrap();
    assert_eq!(bounds(&everywhere), [(i64::MIN, i64::MAX)]);
    assert_eq!(everywhere.observed(), &Extent::everything());

    let nowhere = Formula::Or(vec![]).evaluate(&system).unwrap();
    assert!(nowhere.is_empty());
    assert_eq!(nowhere.observed(), &Extent::everything());
}

// Errors

#[test]
fn evaluation_names_what_went_wrong() {
    let mut system = SequenceSystem::new();
    system.insert("count", numbers("count", &[(1, 1)]));
    system.insert("up", timeline("up", &[(0, 5)]));
    let count = |condition| Formula::while_matching(Pattern::new("count", condition));

    assert_eq!(
        Formula::state("nope").evaluate(&system),
        Err(ProcessError::UnknownKey { key: "nope".into() })
    );
    assert_eq!(
        Formula::occurs(Pattern::new("nope", Condition::<i64>::Any)).evaluate(&system),
        Err(ProcessError::UnknownKey { key: "nope".into() })
    );
    assert_eq!(
        Formula::state("count").evaluate(&system),
        Err(ProcessError::NotATimeline {
            key: "count".into()
        })
    );
    assert_eq!(
        Formula::while_matching(Pattern::new("up", Condition::<i64>::Any)).evaluate(&system),
        Err(ProcessError::NotPoints { key: "up".into() })
    );
    assert_eq!(
        count(Condition::Equals(1.0_f64)).evaluate(&system),
        Err(ProcessError::ValueTypeMismatch {
            key: "count".into(),
            expected: "i64",
            found: "f64",
        })
    );
    // The end pattern is checked too, even over the same member.
    let wrong_end = Formula::between(
        Pattern::new("count", Condition::Equals(1_i64)),
        Pattern::new("count", Condition::Equals("end")),
    );
    assert!(matches!(
        wrong_end.evaluate(&system),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));
    assert!(!ProcessError::UnknownKey { key: "x".into() }
        .to_string()
        .is_empty());
}

#[test]
fn a_shift_keeps_the_end_of_time_and_saturates_other_bounds() {
    // On the default extent an open span lasts for good, so it reaches the end
    // of time; shifting it must neither fail nor move that end.
    let system = system_with(words("motion", &[(10, "start")]));
    let widened = motion_between().shift(-2, 5).evaluate(&system).unwrap();
    assert_eq!(bounds(&widened), [(8, i64::MAX)]);

    // A shift too large for a bounded span saturates rather than failing.
    let mut system = SequenceSystem::new();
    system.insert("up", timeline("up", &[(0, 5)]));
    let pushed = Formula::state("up")
        .shift(i64::MAX, i64::MAX)
        .evaluate(&system)
        .unwrap();
    assert!(pushed.is_empty());
}

// Events

#[test]
fn edges_are_a_start_and_an_end_for_each_span() {
    let mut system = SequenceSystem::new();
    system.insert(
        "up",
        timeline("up", &[(5, 10), (20, 50)]).with_observed(window(0, 50)),
    );
    let edges = Events::Edges(Formula::state("up"))
        .evaluate(&system)
        .unwrap();
    let points = edges.as_time_point::<Edge>().unwrap();

    let listed: Vec<(i64, Edge)> = points
        .iter()
        .map(|event| (event.timestamp(), *event.value()))
        .collect();
    assert_eq!(
        listed,
        [
            (5, Edge::Start),
            (10, Edge::End),
            (20, Edge::Start),
            (50, Edge::End)
        ]
    );
    assert_eq!(points.label(), "edges(up)");
    assert_eq!(points.observed(), &window(0, 50));
}

#[test]
fn matching_keeps_the_points_and_their_values() {
    let mut system = SequenceSystem::new();
    system.insert(
        "door",
        words("door", &[(5, "open"), (7, "close"), (9, "open")]).with_observed(window(0, 20)),
    );
    let matched = Events::Matching(word("door", "open"))
        .evaluate(&system)
        .unwrap();
    let points = matched.as_time_point::<&str>().unwrap();

    let listed: Vec<(i64, &str)> = points
        .iter()
        .map(|event| (event.timestamp(), *event.value()))
        .collect();
    assert_eq!(listed, [(5, "open"), (9, "open")]);
    assert_eq!(points.label(), "matching(door = \"open\")");
    assert_eq!(points.observed(), &window(0, 20));

    assert!(matches!(
        Events::Matching(Pattern::new("door", Condition::Equals(1_i64))).evaluate(&system),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));
}

#[test]
fn edges_read_back_by_between_rebuild_the_state() {
    for_each_seed(CASES, |random, _| {
        let up =
            TimeIntervalSequence::from_spans("up", spans(random, 10)).with_observed(extent(random));
        let mut system = SequenceSystem::new();
        system.insert("up", up.clone());

        Processor::new()
            .with("edges", Events::Edges(Formula::state("up")))
            .apply(&mut system)
            .unwrap();
        let rebuilt = Formula::between(
            Pattern::new("edges", Condition::Equals(Edge::Start)),
            Pattern::new("edges", Condition::Equals(Edge::End)),
        )
        .evaluate(&system)
        .unwrap();

        assert!(rebuilt.covers_same(&up));
        assert_eq!(rebuilt.observed(), up.observed());
    });
}

// Processors

#[test]
fn a_processor_files_each_output_under_its_key() {
    let mut system = system_with(words("motion", &[(10, "start"), (25, "end")]));
    system.insert("up", timeline("up", &[(0, 5)]));
    system
        .metadata_mut()
        .insert("unit".into(), "seconds".into());

    let mut processor = Processor::new()
        .with("active", motion_between())
        .with("changes", Events::Edges(Formula::state("up")));
    assert_eq!(processor.outputs().len(), 2);
    assert_eq!(
        processor.outputs().get("active"),
        Some(&Output::State(motion_between()))
    );
    assert_eq!(
        processor.insert("active", Formula::state("up")),
        Some(Output::State(motion_between()))
    );
    processor.insert("active", motion_between());

    // `run` holds the outputs alone.
    let derived = processor.run(&system).unwrap();
    assert!(derived
        .keys()
        .map(|key| key.as_str())
        .eq(["active", "changes"]));
    assert_eq!(bounds(derived.intervals("active").unwrap()), [(10, 25)]);
    assert!(derived.points::<Edge>("changes").is_some());
    assert!(derived.metadata().is_empty());

    // `apply` adds them to the input.
    processor.apply(&mut system).unwrap();
    assert_eq!(system.len(), 4);
    assert_eq!(system.get("active"), derived.get("active"));
    assert_eq!(system.metadata().len(), 1);
}

#[test]
fn outputs_read_the_system_as_it_was() {
    let system = system_with(words("motion", &[(10, "start"), (25, "end")]));
    let processor = Processor::new()
        .with("a", motion_between())
        // Not yet a member of the system, so it cannot be read.
        .with("b", Formula::state("a"));

    assert_eq!(
        processor.run(&system),
        Err(ProcessError::UnknownKey { key: "a".into() })
    );
}

#[test]
fn a_failing_output_leaves_the_system_untouched() {
    let mut system = system_with(words("motion", &[(10, "start"), (25, "end")]));
    let before = system.clone();

    let processor = Processor::new()
        .with("good", motion_between())
        .with("zzz-bad", Formula::state("missing"));

    assert_eq!(
        processor.apply(&mut system),
        Err(ProcessError::UnknownKey {
            key: "missing".into()
        })
    );
    assert_eq!(system, before);
}

// Per-instant oracles

/// A random point sequence of small numbers, observed over a random extent.
fn random_numbers(random: &mut Random) -> TimePointSequence<i64> {
    let drawn = random.below(13);
    let events = (0..drawn)
        .map(|_| {
            TimePointEvent::new(
                random.between(DOMAIN_START, DOMAIN_END),
                random.between(0, 4),
            )
        })
        .collect();
    TimePointSequence::from_events("p", events).with_observed(extent(random))
}

/// A random condition on the small numbers, with its meaning as a closure.
fn random_test(random: &mut Random) -> (Condition<i64>, Box<dyn Fn(i64) -> bool>) {
    let bound = random.between(0, 4);
    match random.below(4) {
        0 => (Condition::Equals(bound), Box::new(move |v| v == bound)),
        1 => (Condition::Less(bound), Box::new(move |v| v < bound)),
        2 => (
            Condition::GreaterOrEqual(bound),
            Box::new(move |v| v >= bound),
        ),
        _ => (Condition::Any, Box::new(|_| true)),
    }
}

/// What is known of `timeline` at `instant`: `None` when unobserved.
fn state(timeline: &TimeIntervalSequence, instant: i64) -> Option<bool> {
    if timeline.is_observed(instant) {
        Some(timeline.contains(instant))
    } else {
        assert!(
            !timeline.contains(instant),
            "active but unobserved at {instant}"
        );
        None
    }
}

/// The first instant of the run of `observed` instants that holds `instant`,
/// with nothing happening before the domain.
fn run_start(instant: i64, observed: &dyn Fn(i64) -> bool) -> i64 {
    let mut start = instant;
    while start > DOMAIN_START - 1 && observed(start - 1) {
        start -= 1;
    }
    start
}

#[test]
fn occurs_matches_its_definition_at_every_instant() {
    for_each_seed(CASES, |random, _| {
        let points = random_numbers(random);
        let (condition, test) = random_test(random);
        let mut system = SequenceSystem::new();
        system.insert("p", points.clone());

        let active = Formula::occurs(Pattern::new("p", condition))
            .evaluate(&system)
            .unwrap();

        for instant in CHECK {
            let expected = points
                .is_observed(instant)
                .then(|| points.get(instant).iter().any(|event| test(*event.value())));
            assert_eq!(state(&active, instant), expected, "instant {instant}");
        }
    });
}

#[test]
fn while_matches_its_definition_at_every_instant() {
    for_each_seed(CASES, |random, _| {
        let points = random_numbers(random);
        let (condition, test) = random_test(random);
        let mut system = SequenceSystem::new();
        system.insert("p", points.clone());

        let active = Formula::while_matching(Pattern::new("p", condition))
            .evaluate(&system)
            .unwrap();

        for instant in CHECK {
            let expected = if points.is_observed(instant) {
                let from = run_start(instant, &|t| points.is_observed(t));
                // The last sample at or before the instant, in its run.
                points
                    .iter()
                    .rfind(|event| (from..=instant).contains(&event.timestamp()))
                    .map(|event| test(*event.value()))
            } else {
                None
            };
            assert_eq!(state(&active, instant), expected, "instant {instant}");
        }
    });
}

#[test]
fn between_matches_its_definition_at_every_instant() {
    for_each_seed(CASES, |random, _| {
        let first = random_numbers(random);
        let second = random_numbers(random);
        let same_member = random.chance(1, 2);
        let head = random.chance(1, 2);
        let (start_condition, starts) = random_test(random);
        let (end_condition, ends) = random_test(random);

        let mut system = SequenceSystem::new();
        system.insert("p", first.clone());
        system.insert("q", second.clone());
        let end_key = if same_member { "p" } else { "q" };
        let closing = if same_member { &first } else { &second };

        let formula = Formula::between_with(
            Pattern::new("p", start_condition),
            Pattern::new(end_key, end_condition),
            if head { OpenEnd::Head } else { OpenEnd::Extent },
        );
        let active = formula.evaluate(&system).unwrap();

        // One past the latest point of the two members, and nothing before any.
        let latest = [&first, closing]
            .iter()
            .filter_map(|points| points.last().map(TimePointEvent::timestamp))
            .max();
        let head_of = latest.map_or(i64::MIN, |timestamp| timestamp + 1);
        let observed = |instant: i64| {
            first.is_observed(instant)
                && closing.is_observed(instant)
                && (!head || instant < head_of)
        };

        for instant in CHECK {
            let expected = observed(instant).then(|| {
                let from = run_start(instant, &observed);
                let mut open = false;
                let mut opened_now = false;
                for at in from..=instant {
                    // Points of `start` come first, then those of `end`; one
                    // member's points keep their order, read once each.
                    let reads: Vec<(bool, bool)> = if same_member {
                        first
                            .get(at)
                            .iter()
                            .map(|event| (starts(*event.value()), ends(*event.value())))
                            .collect()
                    } else {
                        first
                            .get(at)
                            .iter()
                            .map(|event| (starts(*event.value()), false))
                            .chain(
                                second
                                    .get(at)
                                    .iter()
                                    .map(|event| (false, ends(*event.value()))),
                            )
                            .collect()
                    };
                    for (opens, closes) in reads {
                        if !open && opens {
                            open = true;
                            opened_now |= at == instant;
                        } else if open && closes {
                            open = false;
                        }
                    }
                }
                open || opened_now
            });
            assert_eq!(state(&active, instant), expected, "instant {instant}");
        }
    });
}
