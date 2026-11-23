//! Conditions, formulas and processors: what each leaf and node means, checked
//! against literal expectations and, for the three leaves that read points,
//! against definitions stated one instant at a time over random inputs.

#[path = "common/random.rs"]
mod common;
#[path = "common/update.rs"]
mod update;

use chronoloom::{
    Condition, Edge, Events, Extent, Formula, Key, Kind, OpenEnd, Pattern, ProcessError, Processor,
    Recipe, SequenceSystem, TimeIntervalSequence, TimePointEvent, TimePointSequence, Value,
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

#[test]
fn nested_algebra_over_between_while_and_shift_matches_the_sequence_algebra() {
    let system = system_with(words(
        "motion",
        &[(10, "start"), (20, "stop"), (40, "start")],
    ));
    let between = motion_between();
    let held = Formula::While {
        pattern: word("motion", "start"),
        timeout: None,
    };
    let occurs = Formula::Occurs(word("motion", "stop"));
    let evaluate = |formula: &Formula| formula.evaluate(&system).unwrap();
    let (shifted, held_timeline, stops) = (
        evaluate(&between.clone().shift(-1, 2)),
        evaluate(&held),
        evaluate(&occurs),
    );

    let formula = !(between
        .clone()
        .shift(-1, 2)
        .and(held.clone())
        .or(occurs.clone()));
    let expected = shifted
        .intersection(&held_timeline)
        .union(&stops)
        .complement();
    let evaluated = evaluate(&formula);
    assert!(evaluated.covers_same(&expected), "{formula}");
    assert_eq!(evaluated.observed(), expected.observed());
}

#[test]
fn an_error_inside_any_node_surfaces_unchanged() {
    let system = two_states();
    let missing = || Formula::state("nope");
    let error = || Err(ProcessError::UnknownKey { key: "nope".into() });
    let state = || Formula::state("a");

    assert_eq!(missing().shift(1, 1).evaluate(&system).map(|_| ()), error());
    assert_eq!((!missing()).evaluate(&system).map(|_| ()), error());
    assert_eq!(
        state().and(missing()).evaluate(&system).map(|_| ()),
        error()
    );
    assert_eq!(missing().or(state()).evaluate(&system).map(|_| ()), error());
    assert_eq!(
        Formula::Occurs(word("nope", "x"))
            .and(state())
            .evaluate(&system)
            .map(|_| ()),
        error()
    );
}

#[test]
fn a_recipe_evaluates_like_its_formula_or_events_and_keeps_the_label() {
    let system = two_states();
    let formula = Formula::state("a").and(!Formula::state("b"));
    let direct = formula.evaluate(&system).unwrap();
    let through = Recipe::Timeline(formula.clone()).evaluate(&system).unwrap();
    let timeline = through.as_time_interval().expect("a timeline");
    assert!(timeline.covers_same(&direct));
    assert_eq!(timeline.observed(), direct.observed());
    assert_eq!(timeline.label(), formula.to_string());

    let events = Events::Edges(Formula::state("a"));
    assert_eq!(
        Recipe::Points(events.clone()).evaluate(&system).unwrap(),
        events.evaluate(&system).unwrap()
    );
    assert_eq!(
        Recipe::Timeline(Formula::state("nope"))
            .evaluate(&system)
            .map(|_| ()),
        Err(ProcessError::UnknownKey { key: "nope".into() })
    );
}

#[test]
fn every_points_recipe_evaluates_like_its_events() {
    let mut system = sightings();
    system.insert(
        "door",
        words("door", &[(5, "open"), (7, "open"), (9, "close")]),
    );
    system.insert("up", timeline("up", &[(0, 5)]));
    for events in [
        Events::Edges(Formula::state("up")),
        Events::Matching(word("door", "open")),
        Events::changes("door"),
        Events::project("sensor", "agent.id", Kind::Integer),
    ] {
        let through = Recipe::Points(events.clone()).evaluate(&system);
        assert_eq!(through, events.evaluate(&system));
        assert!(through.is_ok(), "{events}");
    }
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
fn matching_and_edges_report_what_cannot_be_read() {
    let mut system = system_with(words("motion", &[(10, "start")]));
    system.insert("up", timeline("up", &[(0, 5)]));

    assert_eq!(
        Events::Matching(word("nothing", "x")).evaluate(&system),
        Err(ProcessError::UnknownKey {
            key: "nothing".into()
        })
    );
    assert_eq!(
        Events::Matching(Pattern::new("up", Condition::<i64>::Any)).evaluate(&system),
        Err(ProcessError::NotPoints { key: "up".into() })
    );
    assert_eq!(
        Events::Edges(Formula::state("nothing")).evaluate(&system),
        Err(ProcessError::UnknownKey {
            key: "nothing".into()
        })
    );
    // Both patterns of a between are read, whichever fails.
    for (start, end) in [("nothing", "motion"), ("motion", "nothing")] {
        let formula = Formula::between(word(start, "start"), word(end, "stop"));
        assert_eq!(
            formula.evaluate(&system).map(|_| ()),
            Err(ProcessError::UnknownKey {
                key: "nothing".into()
            })
        );
    }
}

#[test]
fn edges_of_an_empty_timeline_are_no_points_and_keep_the_extent() {
    let mut system = SequenceSystem::new();
    system.insert("up", timeline("up", &[]).with_observed(window(0, 30)));
    let edges = Events::Edges(Formula::state("up"))
        .evaluate(&system)
        .unwrap();
    let points = edges.as_time_point::<Edge>().unwrap();

    assert_eq!(points.iter().count(), 0);
    assert_eq!(points.observed(), &window(0, 30));
}

#[test]
fn edges_read_back_by_between_rebuild_the_state() {
    for_each_seed(CASES, |random, _| {
        let up =
            TimeIntervalSequence::from_spans("up", spans(random, 10)).with_observed(extent(random));
        let mut system = SequenceSystem::new();
        system.insert("up", up.clone());

        apply(
            &mut system,
            &[Processor::new("edges", Events::Edges(Formula::state("up")))],
        )
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

#[test]
fn changes_drop_the_repeats_and_keep_the_first_point_of_each_run() {
    let mut system = SequenceSystem::new();
    system.insert(
        "door",
        words(
            "door",
            &[
                (5, "open"),
                (7, "open"),
                (9, "close"),
                (12, "close"),
                (30, "close"),
                (32, "open"),
            ],
        )
        .with_observed(Extent::from_spans(vec![span(0, 20), span(25, 40)])),
    );
    let changes = Events::changes("door").evaluate(&system).unwrap();
    let points = changes.as_time_point::<&str>().unwrap();

    let listed: Vec<(i64, &str)> = points
        .iter()
        .map(|event| (event.timestamp(), *event.value()))
        .collect();
    // 30 starts a run, so it is kept although it repeats what was before it.
    assert_eq!(
        listed,
        [(5, "open"), (9, "close"), (30, "close"), (32, "open")]
    );
    assert_eq!(points.label(), "changes(door)");
    assert_eq!(
        points.observed(),
        system.points::<&str>("door").unwrap().observed()
    );
}

#[test]
fn changes_take_the_last_recorded_of_the_points_sharing_an_instant() {
    let mut system = SequenceSystem::new();
    system.insert(
        "count",
        numbers(
            "count",
            &[(5, 1), (10, 2), (10, 1), (15, 1), (20, 3), (20, 3)],
        ),
    );
    let changes = Events::changes("count").evaluate(&system).unwrap();
    let points = changes.as_time_point::<i64>().unwrap();

    let listed: Vec<(i64, i64)> = points
        .iter()
        .map(|event| (event.timestamp(), *event.value()))
        .collect();
    assert_eq!(listed, [(5, 1), (20, 3)]);
}

#[test]
fn changes_report_what_cannot_be_read() {
    let system = system_with(words("motion", &[(10, "start")]));
    let mut with_timeline = system.clone();
    with_timeline.insert("up", timeline("up", &[(0, 5)]));

    assert!(matches!(
        Events::changes("nothing").evaluate(&system),
        Err(ProcessError::UnknownKey { .. })
    ));
    assert!(matches!(
        Events::changes("up").evaluate(&with_timeline),
        Err(ProcessError::NotPoints { .. })
    ));
}

#[test]
fn changes_are_idempotent_and_leave_while_unchanged() {
    for_each_seed(CASES, |random, _| {
        let points = random_numbers(random);
        let (condition, _) = random_test(random);
        let mut system = SequenceSystem::new();
        system.insert("p", points);

        let processor = vec![
            Processor::new("changes", Events::changes("p")),
            Processor::new("again", Events::changes("changes")),
        ];
        let derived = update::run(&system, &processor).unwrap();
        let once = derived.points::<i64>("changes").unwrap();
        let twice = derived.points::<i64>("again").unwrap();
        assert!(once.iter().eq(twice.iter()), "applying it twice");
        assert_eq!(once.observed(), twice.observed());

        // No two consecutive samples of a run are equal.
        let kept: Vec<_> = once.iter().collect();
        assert!(kept.windows(2).all(|pair| {
            let (before, after) = (pair[0], pair[1]);
            before.timestamp() < after.timestamp()
                && (before.value() != after.value()
                    || (before.timestamp()..after.timestamp())
                        .any(|instant| !once.is_observed(instant)))
        }));

        let mut together = system.clone();
        together.insert("changes", once.clone());
        let original = Formula::while_matching(Pattern::new("p", condition.clone()))
            .evaluate(&together)
            .unwrap();
        let reduced = Formula::while_matching(Pattern::new("changes", condition))
            .evaluate(&together)
            .unwrap();
        assert!(original.covers_same(&reduced));
        assert_eq!(original.observed(), reduced.observed());
    });
}

// Processors

/// The outputs of `processors` filed into `system`, all or nothing.
fn apply(system: &mut SequenceSystem, processors: &[Processor]) -> Result<(), ProcessError> {
    let derived = update::run(system, processors)?;
    for key in derived.derived() {
        let sequence = derived.get(key.as_str()).expect("listed").clone();
        system.insert(key.clone(), sequence);
    }
    Ok(())
}

#[test]
fn each_processor_files_its_output_under_its_key() {
    let mut system = system_with(words("motion", &[(10, "start"), (25, "end")]));
    system.insert("up", timeline("up", &[(0, 5)]));

    let processor = vec![
        Processor::new("active", motion_between()),
        Processor::new("changes", Events::Edges(Formula::state("up"))),
    ];
    assert_eq!(processor[0].output().as_str(), "active");
    assert_eq!(processor[0].recipe(), &Recipe::Timeline(motion_between()));

    let derived = update::run(&system, &processor).unwrap();
    assert!(derived
        .derived()
        .map(|key| key.as_str())
        .eq(["active", "changes"]));
    assert_eq!(bounds(derived.intervals("active").unwrap()), [(10, 25)]);
    assert!(derived.points::<Edge>("changes").is_some());
}

#[test]
fn outputs_that_read_no_other_output_see_the_system_as_it_was() {
    let system = system_with(words("motion", &[(10, "start"), (25, "end")]));
    let processor = vec![
        Processor::new("a", motion_between()),
        Processor::new("b", Formula::state("missing")),
    ];

    assert_eq!(
        update::run(&system, &processor).map(drop),
        Err(ProcessError::UnknownKey {
            key: "missing".into()
        })
    );
}

#[test]
fn a_failing_output_leaves_the_system_untouched() {
    let mut system = system_with(words("motion", &[(10, "start"), (25, "end")]));
    let before = system.clone();

    let processor = vec![
        Processor::new("good", motion_between()),
        Processor::new("zzz-bad", Formula::state("missing")),
    ];

    assert_eq!(
        apply(&mut system, &processor),
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
fn hold_is_active_for_a_duration_from_each_matching_point() {
    let mut system = SequenceSystem::new();
    system.insert(
        "p",
        words("p", &[(0, "a"), (2, "a"), (3, "b"), (10, "a"), (20, "a")]),
    );
    let pattern = word("p", "a");

    // Holds [0, 3) and [2, 5) merge, [10, 13) stands alone, and the one at 20
    // lasts three ticks.
    let held = Formula::hold(pattern.clone(), 3).evaluate(&system).unwrap();
    assert_eq!(bounds(&held), [(0, 5), (10, 13), (20, 23)]);

    // Holds that touch merge too.
    let touching = Formula::hold(pattern.clone(), 2).evaluate(&system).unwrap();
    assert_eq!(bounds(&touching), [(0, 4), (10, 12), (20, 22)]);

    // One tick is `Occurs`, and a hold is the shift it stands for.
    assert_eq!(
        Formula::hold(pattern.clone(), 1),
        Formula::occurs(pattern.clone()).shift(0, 0)
    );
    assert_eq!(
        Formula::hold(pattern.clone(), 4),
        Formula::Shift {
            formula: Box::new(Formula::Occurs(pattern)),
            alpha: 0,
            beta: 3,
        }
    );
}

#[test]
#[should_panic(expected = "a hold lasts at least one tick")]
fn a_hold_of_no_duration_panics() {
    let _ = Formula::hold(word("p", "a"), 0);
}

#[test]
#[should_panic(expected = "a hold lasts at least one tick")]
fn a_hold_of_a_negative_duration_panics() {
    let _ = Formula::hold(word("p", "a"), -2);
}

#[test]
fn hold_equals_its_shifted_occurs_and_its_definition_at_every_instant() {
    for_each_seed(CASES, |random, _| {
        let points = random_numbers(random);
        let (condition, test) = random_test(random);
        let ticks = random.between(1, 6);
        let mut system = SequenceSystem::new();
        system.insert("p", points.clone());

        let pattern = Pattern::new("p", condition);
        let held = Formula::hold(pattern.clone(), ticks)
            .evaluate(&system)
            .unwrap();
        let definition = Formula::occurs(pattern)
            .shift(0, ticks - 1)
            .evaluate(&system)
            .unwrap();
        assert_eq!(held, definition);

        // Active at `instant` when a matching point `t` has t <= instant <
        // t + ticks. Known when active, or when the whole window of points
        // that could have made it active, `instant - ticks < t <= instant`, was
        // observed: the three-valued meaning of a shift.
        for instant in CHECK {
            let active = points.iter().any(|event| {
                test(*event.value())
                    && event.timestamp() <= instant
                    && instant < event.timestamp() + ticks
            });
            let window_seen = (instant - ticks + 1..=instant).all(|t| points.is_observed(t));
            let expected = (active || window_seen).then_some(active);
            assert_eq!(state(&held, instant), expected, "instant {instant}");
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
fn while_within_matches_its_definition_at_every_instant() {
    for_each_seed(CASES, |random, _| {
        let points = random_numbers(random);
        let (condition, test) = random_test(random);
        let timeout = random.between(1, 12);
        let mut system = SequenceSystem::new();
        system.insert("p", points.clone());

        let active = Formula::while_within(Pattern::new("p", condition), timeout)
            .evaluate(&system)
            .unwrap();

        for instant in CHECK {
            let expected = if points.is_observed(instant) {
                let from = run_start(instant, &|t| points.is_observed(t));
                // The last sample at or before the instant, in its run, which
                // is known only for `timeout` ticks.
                points
                    .iter()
                    .rfind(|event| (from..=instant).contains(&event.timestamp()))
                    .filter(|event| instant - event.timestamp() < timeout)
                    .map(|event| test(*event.value()))
            } else {
                None
            };
            assert_eq!(
                state(&active, instant),
                expected,
                "instant {instant}, timeout {timeout}"
            );
        }
    });
}

#[test]
fn while_within_equals_while_matching_when_no_gap_outlasts_the_timeout() {
    for_each_seed(CASES, |random, _| {
        let points = random_numbers(random);
        let (condition, _) = random_test(random);
        let mut system = SequenceSystem::new();
        system.insert("p", points);

        let pattern = || Pattern::new("p", condition.clone());
        // Longer than the domain, so no sample or run is ever that long.
        let within = Formula::while_within(pattern(), 10 * (DOMAIN_END - DOMAIN_START) + 100)
            .evaluate(&system)
            .unwrap();
        let matching = Formula::while_matching(pattern())
            .evaluate(&system)
            .unwrap();

        // The timeout only matters past the domain, where the last sample
        // would hold to the end of time.
        for instant in CHECK {
            assert_eq!(
                state(&within, instant),
                state(&matching, instant),
                "instant {instant}"
            );
        }
    });
}

#[test]
fn while_within_leaves_what_follows_the_timeout_unknown() {
    let mut system = SequenceSystem::new();
    system.insert(
        "count",
        numbers("count", &[(10, 9), (50, 1), (60, 9)]).with_observed(window(0, 100)),
    );
    let crowded = Formula::while_within(Pattern::new("count", Condition::Greater(5_i64)), 15);
    let active = crowded.evaluate(&system).unwrap();

    assert_eq!(bounds(&active), [(10, 25), (60, 75)]);
    // Known from each sample for 15 ticks, unknown until the next one, and
    // unknown from the last timeout to the end of the run.
    assert_eq!(extent_bounds(active.observed()), [(10, 25), (50, 75)]);
    assert!(!active.is_observed(30) && !active.is_observed(90));
    assert_eq!(active.label(), "while(count > 5, within 15)");
    assert_eq!(crowded.to_string(), "while(count > 5, within 15)");
}

#[test]
fn while_within_is_an_atom_of_the_normal_form() {
    let sample = Formula::while_within(Pattern::new("count", Condition::Greater(5_i64)), 15);
    let formula = !(sample.clone().and(Formula::state("up")));

    assert_eq!(
        formula.conjunctive_normal_form().to_string(),
        "(¬while(count > 5, within 15) ∨ ¬up)"
    );
}

#[test]
#[should_panic(expected = "a sample holds at least one tick")]
fn while_within_refuses_a_timeout_that_is_not_positive() {
    let _ = Formula::while_within(Pattern::new("count", Condition::Any::<i64>), 0);
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

// Chained outputs

fn keys(names: &[&str]) -> Vec<Key> {
    names.iter().map(|&name| Key::from(name)).collect()
}

#[test]
fn point_outputs_can_read_point_outputs() {
    let mut system = SequenceSystem::new();
    system.insert("up", timeline("up", &[(0, 5), (8, 9)]));

    let processor = vec![
        Processor::new("edges", Events::Edges(Formula::state("up"))),
        Processor::new(
            "starts",
            Events::Matching(Pattern::new("edges", Condition::Equals(Edge::Start))),
        ),
    ];
    let derived = update::run(&system, &processor).unwrap();

    assert_eq!(derived.points::<Edge>("starts").unwrap().len(), 2);
    assert_eq!(derived.points::<Edge>("edges").unwrap().len(), 4);
}

#[test]
fn a_formula_that_reads_its_own_output_is_a_cycle() {
    let processor = Processor::new("a", !Formula::state("a"));
    let error = ProcessError::Cycle { keys: keys(&["a"]) };

    assert_eq!(processor.check(), Err(error.clone()));
    assert_eq!(
        update::run(&SequenceSystem::new(), &[processor]).map(drop),
        Err(error)
    );
}

#[test]
fn check_accepts_a_processor_that_reads_another_output_or_a_member() {
    for processor in [
        Processor::new("b", Formula::state("a")),
        Processor::new("c", Formula::state("b").and(Formula::state("outside"))),
    ] {
        assert_eq!(processor.check(), Ok(()), "{processor}");
    }
}

#[test]
fn a_failure_in_a_chain_leaves_the_system_as_it_was() {
    let mut system = SequenceSystem::new();
    system.insert("up", timeline("up", &[(0, 5)]));
    let untouched = system.clone();

    let processor = vec![
        Processor::new("first", Formula::state("up")),
        Processor::new(
            "second",
            Formula::state("first").and(Formula::state("missing")),
        ),
    ];

    assert_eq!(
        apply(&mut system, &processor),
        Err(ProcessError::UnknownKey {
            key: "missing".into()
        })
    );
    assert_eq!(system, untouched);
}

// ---------------------------------------------------------------------------
// Projection of record values
// ---------------------------------------------------------------------------

fn sighting(agent: i64, zone: &str) -> Value {
    Value::record([
        (
            "agent",
            Value::record([("id", Value::from(agent)), ("speed", Value::from(1.5))]),
        ),
        ("zone", Value::from(zone)),
        ("seen", Value::from(true)),
    ])
}

/// Three sightings, the middle one with no `agent` and no `seen`, over an
/// extent that stops at 40.
fn sightings() -> SequenceSystem {
    let mut system = SequenceSystem::new();
    system.insert(
        "sensor",
        TimePointSequence::from_events(
            "sensor",
            vec![
                TimePointEvent::new(10, sighting(7, "gate")),
                TimePointEvent::new(20, Value::record([("zone", Value::from("hall"))])),
                TimePointEvent::new(30, sighting(9, "yard")),
            ],
        )
        .with_observed(window(0, 40)),
    );
    system
}

fn project(
    system: &SequenceSystem,
    path: &str,
    into: Kind,
) -> Result<chronoloom::Sequence, ProcessError> {
    Events::project("sensor", path, into).evaluate(system)
}

fn instants<T>(points: &TimePointSequence<T>) -> Vec<i64> {
    points.iter().map(TimePointEvent::timestamp).collect()
}

#[test]
fn a_nested_field_is_projected_into_each_kind() {
    let system = sightings();

    let integers = project(&system, "agent.id", Kind::Integer).unwrap();
    let integers = integers.as_time_point::<i64>().unwrap();
    assert_eq!(
        integers
            .iter()
            .map(|event| *event.value())
            .collect::<Vec<_>>(),
        [7, 9]
    );

    let floats = project(&system, "agent.speed", Kind::Float).unwrap();
    let floats = floats.as_time_point::<f64>().unwrap();
    assert_eq!(
        floats
            .iter()
            .map(|event| *event.value())
            .collect::<Vec<_>>(),
        [1.5, 1.5]
    );

    let text = project(&system, "zone", Kind::Text).unwrap();
    let text = text.as_time_point::<String>().unwrap();
    assert_eq!(
        text.iter()
            .map(|event| event.value().as_str())
            .collect::<Vec<_>>(),
        ["gate", "hall", "yard"]
    );

    let flags = project(&system, "seen", Kind::Bool).unwrap();
    let flags = flags.as_time_point::<bool>().unwrap();
    assert_eq!(
        flags.iter().map(|event| *event.value()).collect::<Vec<_>>(),
        [true, true]
    );

    let records = project(&system, "agent", Kind::Value).unwrap();
    let records = records.as_time_point::<Value>().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records.nth(0).map(|event| event.value().to_string()),
        Some(String::from(r#"{"id": 7, "speed": 1.5}"#))
    );
}

#[test]
fn the_empty_path_projects_the_whole_value() {
    let system = sightings();

    let whole = project(&system, "", Kind::Value).unwrap();
    let whole = whole.as_time_point::<Value>().unwrap();
    assert_eq!(whole.len(), 3);
    assert_eq!(whole.label(), "sensor");
}

#[test]
fn an_integer_field_widens_into_a_float_and_not_the_other_way() {
    let system = sightings();

    let widened = project(&system, "agent.id", Kind::Float).unwrap();
    assert_eq!(
        widened
            .as_time_point::<f64>()
            .unwrap()
            .iter()
            .map(|event| *event.value())
            .collect::<Vec<_>>(),
        [7.0, 9.0]
    );
    assert_eq!(
        project(&system, "agent.speed", Kind::Integer),
        Err(ProcessError::ProjectionMismatch {
            key: Key::from("sensor"),
            path: String::from("agent.speed"),
            expected: Kind::Integer,
            found: "float",
        })
    );
}

#[test]
fn a_missing_or_null_field_is_skipped_and_a_field_of_another_kind_is_the_error() {
    let mut system = sightings();
    let mut sensor = TimePointSequence::<Value>::new("sensor");
    sensor.insert(TimePointEvent::new(1, Value::record([("n", Value::Null)])));
    sensor.insert(TimePointEvent::new(
        2,
        Value::record([("n", Value::from(5))]),
    ));
    // Not a record at all, so it has no field.
    sensor.insert(TimePointEvent::new(3, Value::from(9)));
    system.insert("sensor", sensor);

    let integers = project(&system, "n", Kind::Integer).unwrap();
    assert_eq!(instants(integers.as_time_point::<i64>().unwrap()), [2]);

    // Projected as a value, a null field is kept as it is.
    let values = project(&system, "n", Kind::Value).unwrap();
    assert_eq!(instants(values.as_time_point::<Value>().unwrap()), [1, 2]);

    assert_eq!(
        project(&system, "n", Kind::Text),
        Err(ProcessError::ProjectionMismatch {
            key: Key::from("sensor"),
            path: String::from("n"),
            expected: Kind::Text,
            found: "integer",
        })
    );
}

#[test]
fn a_projection_keeps_the_timestamps_the_extent_and_names_the_field() {
    let system = sightings();

    let projected = project(&system, "agent.id", Kind::Integer).unwrap();
    let projected = projected.as_time_point::<i64>().unwrap();

    assert_eq!(instants(projected), [10, 30]);
    assert_eq!(projected.observed(), &window(0, 40));
    assert_eq!(projected.label(), "sensor.agent.id");
    assert_eq!(
        Events::project("sensor", "agent.id", Kind::Integer).to_string(),
        "sensor.agent.id"
    );
    assert_eq!(
        Events::project("sensor", "", Kind::Value).to_string(),
        "sensor"
    );
}

#[test]
fn a_projection_reads_a_member_of_values_and_nothing_else() {
    let mut system = sightings();
    system.insert("count", numbers("count", &[(1, 5)]));
    system.insert("up", timeline("up", &[(0, 5)]));

    assert!(matches!(
        Events::project("count", "n", Kind::Integer).evaluate(&system),
        Err(ProcessError::ValueTypeMismatch { key, .. }) if key.as_str() == "count"
    ));
    assert_eq!(
        Events::project("up", "n", Kind::Integer).evaluate(&system),
        Err(ProcessError::NotPoints {
            key: Key::from("up")
        })
    );
    assert_eq!(
        Events::project("nothing", "n", Kind::Integer).evaluate(&system),
        Err(ProcessError::UnknownKey {
            key: Key::from("nothing")
        })
    );
}

#[test]
fn a_projection_is_an_output_and_chains_into_formulas() {
    let system = sightings();
    let processor = vec![
        Processor::new(
            "agent",
            Events::project("sensor", "agent.id", Kind::Integer),
        ),
        Processor::new(
            "known",
            Formula::while_matching(Pattern::new("agent", Condition::Greater(8_i64))),
        ),
    ];

    let result = update::run(&system, &processor).unwrap();

    assert_eq!(
        result.points::<i64>("agent").map(instants),
        Some(vec![10, 30])
    );
    assert_eq!(bounds(result.intervals("known").unwrap()), vec![(30, 40)]);
    assert_eq!(
        result.producer("agent").map(Processor::recipe),
        Some(&Recipe::Points(Events::project(
            "sensor",
            "agent.id",
            Kind::Integer
        )))
    );
}

#[test]
fn a_projection_mismatch_names_the_field_and_changes_nothing_when_applied() {
    let mut system = sightings();
    let before = system.clone();
    let processor = vec![
        Processor::new("zone", Events::project("sensor", "zone", Kind::Text)),
        Processor::new("wrong", Events::project("sensor", "zone", Kind::Integer)),
    ];

    let error = apply(&mut system, &processor).unwrap_err();

    assert_eq!(
        error,
        ProcessError::ProjectionMismatch {
            key: Key::from("sensor"),
            path: String::from("zone"),
            expected: Kind::Integer,
            found: "text",
        }
    );
    assert_eq!(
        error.to_string(),
        "the field `zone` of `sensor` cannot be read as integer, it is text"
    );
    assert_eq!(system, before);
}

#[test]
fn conditions_over_whole_values_work_in_formulas() {
    let mut system = SequenceSystem::new();
    system.insert(
        "reading",
        TimePointSequence::from_events(
            "reading",
            vec![
                TimePointEvent::new(0, Value::from(2)),
                TimePointEvent::new(10, Value::from(7.5)),
                TimePointEvent::new(20, Value::from("high")),
                TimePointEvent::new(30, Value::from(3)),
            ],
        )
        .with_observed(window(0, 40)),
    );

    let high = Formula::while_matching(Pattern::new("reading", Condition::Greater(Value::from(5))));
    let spans = high.evaluate(&system).unwrap();

    // Only the 7.5 is above five: the text matches no ordering, so its sample
    // is inactive, and so is the 3 after it.
    assert_eq!(bounds(&spans), vec![(10, 20)]);
    assert_eq!(high.to_string(), "while(reading > 5)");

    let matching = Events::Matching(Pattern::new(
        "reading",
        Condition::GreaterOrEqual(Value::from(7.5)),
    ))
    .evaluate(&system)
    .unwrap();
    assert_eq!(instants(matching.as_time_point::<Value>().unwrap()), [10]);
}

// ---------------------------------------------------------------------------
// Field patterns
// ---------------------------------------------------------------------------

fn count_record(count: Value) -> Value {
    Value::record([("count", count)])
}

fn counts(events: &[(i64, Value)]) -> SequenceSystem {
    let mut system = SequenceSystem::new();
    system.insert(
        "motion",
        TimePointSequence::from_events(
            "motion",
            events
                .iter()
                .map(|(timestamp, value)| TimePointEvent::new(*timestamp, value.clone()))
                .collect(),
        )
        .with_observed(window(0, 100)),
    );
    system
}

fn field(path: &str, condition: Condition<Value>) -> Pattern {
    Pattern::field("motion", path, condition)
}

fn matched_instants(pattern: Pattern, system: &SequenceSystem) -> Vec<i64> {
    let matching = Events::Matching(pattern).evaluate(system).unwrap();
    instants(matching.as_time_point::<Value>().unwrap())
}

#[test]
fn a_field_pattern_reads_nested_paths_and_compares_numbers_across_kinds() {
    let nested = |count: Value| Value::record([("agent", Value::record([("count", count)]))]);
    let system = counts(&[
        (1, nested(Value::from(7))),
        (2, nested(Value::from(5.5))),
        (3, nested(Value::from(3))),
        (4, nested(Value::from("many"))),
    ]);

    let above = field("agent.count", Condition::Greater(Value::from(5)));
    assert_eq!(matched_instants(above, &system), [1, 2]);

    let exact = field("agent.count", Condition::Equals(Value::from(7.0)));
    assert_eq!(matched_instants(exact, &system), [1]);

    let text = field("agent.count", Condition::Equals(Value::from("many")));
    assert_eq!(matched_instants(text, &system), [4]);

    // The empty path is the value itself.
    let whole = field("", Condition::Equals(nested(Value::from(3))));
    assert_eq!(matched_instants(whole, &system), [3]);
}

#[test]
fn a_missing_field_never_matches_and_a_null_field_is_a_value() {
    let system = counts(&[
        (1, count_record(Value::Null)),
        (2, Value::record([("zone", Value::from("a"))])),
        (3, Value::from(9)),
        (4, count_record(Value::from(1))),
    ]);

    // Missing fields (a record without it, a value that is not a record) never
    // match, `Any` and `NotEquals` included.
    let any = field("count", Condition::Any);
    assert_eq!(matched_instants(any, &system), [1, 4]);
    let different = field("count", Condition::NotEquals(Value::from(5)));
    assert_eq!(matched_instants(different, &system), [1, 4]);
    let negated = field("count", Condition::Not(Box::new(Condition::Any)));
    assert!(matched_instants(negated, &system).is_empty());

    // Null is a value: it matches a condition about null, and no ordering.
    let null = field("count", Condition::Equals(Value::Null));
    assert_eq!(matched_instants(null, &system), [1]);
    let below = field("count", Condition::Less(Value::from(5)));
    assert_eq!(matched_instants(below, &system), [4]);
}

#[test]
fn a_field_pattern_is_equal_clonable_and_printed_with_its_path() {
    let pattern = Pattern::field("motion", "direction", Condition::Equals(Value::from("in")));

    assert_eq!(pattern.to_string(), r#"motion.direction = "in""#);
    assert_eq!(pattern.clone(), pattern);
    assert_eq!(pattern.key().as_str(), "motion");
    assert_ne!(
        pattern,
        Pattern::field("motion", "zone", Condition::Equals(Value::from("in")))
    );
    assert_ne!(
        pattern,
        Pattern::new("motion", Condition::Equals(Value::from("in")))
    );
    assert_eq!(
        Pattern::field("motion", "", Condition::Any).to_string(),
        "any motion"
    );
}

#[test]
fn a_field_pattern_works_in_every_leaf() {
    let system = counts(&[
        (10, count_record(Value::from("start"))),
        (20, count_record(Value::from(1))),
        (30, Value::record([("zone", Value::from("a"))])),
        (40, count_record(Value::from("end"))),
        (50, count_record(Value::from("start"))),
    ]);
    let is = |text: &str| field("count", Condition::Equals(Value::from(text)));

    let between = Formula::between(is("start"), is("end"))
        .evaluate(&system)
        .unwrap();
    assert_eq!(bounds(&between), [(10, 40), (50, 100)]);

    let occurs = Formula::occurs(is("end")).evaluate(&system).unwrap();
    assert_eq!(bounds(&occurs), [(40, 41)]);

    let hold = Formula::hold(is("end"), 5).evaluate(&system).unwrap();
    assert_eq!(bounds(&hold), [(40, 45)]);

    // The record without the field is a sample that does not match.
    let low = field("count", Condition::Less(Value::from(5)));
    let held = Formula::while_matching(low).evaluate(&system).unwrap();
    assert_eq!(bounds(&held), [(20, 30)]);

    // Two fields: two patterns, combined by formulas.
    let mut two = SequenceSystem::new();
    let both = |count: i64, zone: &str| {
        Value::record([("count", Value::from(count)), ("zone", Value::from(zone))])
    };
    two.insert(
        "motion",
        TimePointSequence::from_events(
            "motion",
            vec![
                TimePointEvent::new(10, both(7, "gate")),
                TimePointEvent::new(20, both(9, "yard")),
                TimePointEvent::new(30, both(2, "yard")),
            ],
        )
        .with_observed(window(0, 40)),
    );
    let busy = Formula::while_matching(field("count", Condition::Greater(Value::from(5))));
    let gate = Formula::while_matching(field("zone", Condition::Equals(Value::from("gate"))));
    let both_at_once = Formula::And(vec![busy, gate]).evaluate(&two).unwrap();
    assert_eq!(bounds(&both_at_once), [(10, 20)]);
}

#[test]
fn a_field_pattern_over_a_member_that_is_not_values_is_a_type_mismatch() {
    let mut system = SequenceSystem::new();
    system.insert("motion", numbers("motion", &[(1, 5)]));
    let pattern = field("count", Condition::Any);

    assert!(matches!(
        Formula::occurs(pattern.clone()).evaluate(&system),
        Err(ProcessError::ValueTypeMismatch { key, .. }) if key.as_str() == "motion"
    ));
    assert!(matches!(
        Events::Matching(pattern.clone()).evaluate(&system),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));

    // A whole-value pattern over a member of records is a mismatch the other way.
    let records = counts(&[(1, count_record(Value::from(1)))]);
    assert!(
        Formula::occurs(Pattern::new("motion", Condition::Any::<i64>))
            .evaluate(&records)
            .is_err()
    );
    assert!(Formula::occurs(pattern).evaluate(&records).is_ok());
}

#[test]
fn a_field_and_a_projection_differ_only_for_an_event_without_the_field() {
    let system = counts(&[
        (10, count_record(Value::from(7))),
        (20, Value::record([("zone", Value::from("a"))])),
        (30, count_record(Value::from(9))),
    ]);
    let processor = vec![
        Processor::new("count", Events::project("motion", "count", Kind::Value)),
        Processor::new(
            "by_field",
            Formula::while_matching(field("count", Condition::Greater(Value::from(5)))),
        ),
        Processor::new(
            "by_projection",
            Formula::while_matching(Pattern::new("count", Condition::Greater(Value::from(5)))),
        ),
    ];
    let result = update::run(&system, &processor).unwrap();

    // The field pattern reads the record without a count as a sample that does
    // not match, which ends the span; the projection drops it.
    assert_eq!(
        bounds(result.intervals("by_field").unwrap()),
        [(10, 20), (30, 100)]
    );
    assert_eq!(
        bounds(result.intervals("by_projection").unwrap()),
        [(10, 100)]
    );
}

/// A record stream where `n` is, at each event, an integer, a float, null,
/// text, or (when `gaps` allows) absent, and `a.n` an integer.
fn field_stream(random: &mut Random, gaps: bool) -> SequenceSystem {
    let mut timestamps: Vec<i64> = (0..random.below(16))
        .map(|_| random.between(DOMAIN_START, DOMAIN_END))
        .collect();
    timestamps.sort_unstable();
    timestamps.dedup();

    let events = timestamps
        .into_iter()
        .map(|timestamp| {
            let mut fields = vec![(
                "a",
                Value::record([("n", Value::from(random.between(0, 4)))]),
            )];
            match random.below(if gaps { 6 } else { 5 }) {
                0 | 1 => fields.push(("n", Value::from(random.between(0, 4)))),
                2 => fields.push(("n", Value::from(random.between(0, 8) as f64 / 2.0))),
                3 => fields.push(("n", Value::Null)),
                4 => fields.push(("n", Value::from("text"))),
                _ => {}
            }
            TimePointEvent::new(timestamp, Value::record(fields))
        })
        .collect();

    let mut system = SequenceSystem::new();
    system.insert(
        "motion",
        TimePointSequence::from_events("motion", events).with_observed(extent(random)),
    );
    system
}

fn random_field_condition(random: &mut Random) -> Condition<Value> {
    let bound = || Value::from(2);
    match random.below(7) {
        0 => Condition::Equals(bound()),
        1 => Condition::Less(bound()),
        2 => Condition::GreaterOrEqual(Value::from(1.5)),
        3 => Condition::Equals(Value::Null),
        4 => Condition::Not(Box::new(Condition::Equals(bound()))),
        5 => Condition::Within {
            low: Value::from(1),
            high: Value::from(3),
        },
        _ => Condition::Any,
    }
}

/// The same formulas over a field and over the field projected with
/// `Kind::Value`, as a processor with the outputs `field` and `projected`.
fn field_and_projection(random: &mut Random, path: &str, between: bool) -> Vec<Processor> {
    let condition = random_field_condition(random);
    let end = random_field_condition(random);
    let (by_field, by_projection) = if between {
        (
            Formula::between(
                Pattern::field("motion", path, condition.clone()),
                Pattern::field("motion", path, end.clone()),
            ),
            Formula::between(
                Pattern::new("projected", condition),
                Pattern::new("projected", end),
            ),
        )
    } else {
        (
            Formula::while_matching(Pattern::field("motion", path, condition.clone())),
            Formula::while_matching(Pattern::new("projected", condition)),
        )
    };

    vec![
        Processor::new("projected", Events::project("motion", path, Kind::Value)),
        Processor::new("field", by_field),
        Processor::new("projection", by_projection),
    ]
}

#[test]
fn between_and_while_over_a_field_equal_them_over_its_projection() {
    for_each_seed(CASES, |random, _| {
        let system = field_stream(random, false);
        let path = if random.chance(1, 2) { "n" } else { "a.n" };
        let between = random.chance(1, 2);
        let result = update::run(&system, &field_and_projection(random, path, between)).unwrap();

        let by_field = result.intervals("field").unwrap();
        let by_projection = result.intervals("projection").unwrap();
        assert_eq!(bounds(by_field), bounds(by_projection));
        assert_eq!(by_field.observed(), by_projection.observed());
    });
}

#[test]
fn matching_a_field_equals_matching_its_projection_even_with_gaps() {
    for_each_seed(CASES, |random, _| {
        let system = field_stream(random, true);
        let condition = random_field_condition(random);
        let processor = vec![
            Processor::new("projected", Events::project("motion", "n", Kind::Value)),
            Processor::new(
                "projection",
                Events::Matching(Pattern::new("projected", condition.clone())),
            ),
        ];
        let result = update::run(&system, &processor).unwrap();
        let by_projection = instants(result.points::<Value>("projection").unwrap());

        assert_eq!(
            matched_instants(field("n", condition), &system),
            by_projection
        );
    });
}
