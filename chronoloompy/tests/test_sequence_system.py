import pytest

from chronoloompy import (
    Batch,
    RecordError,
    SequenceSystem,
    TimeIntervalSequence,
    TimePointSequence,
)
from tests.members import points, timeline


def machine() -> SequenceSystem:
    system = SequenceSystem()
    system.add_points("temperature", [10], [21.5])
    system.add_points("log", [10], ["boot"])
    system.add_spans("up", [0], [60])
    return system


def test_members_are_listed_in_attribute_order():
    system = machine()

    assert system.attributes() == ["log", "temperature", "up"]
    assert list(system) == ["log", "temperature", "up"]
    assert len(system) == 3
    assert "up" in system
    assert [attribute for attribute, _ in system.items()] == system.attributes()


def test_indexing_returns_a_handle_of_the_right_shape():
    system = machine()

    assert isinstance(system["temperature"], TimePointSequence)
    assert isinstance(system["up"], TimeIntervalSequence)
    assert system.get("missing") is None
    with pytest.raises(KeyError):
        system["missing"]


def test_recording_reaches_the_member():
    system = machine()
    system.record_point("temperature", 11, 21.7)
    system.record_span("up", 60, 90)

    assert points(system, "temperature").timestamps.tolist() == [10, 11]
    assert timeline(system, "up").spans.tolist() == [[0, 90]]


def test_recording_converts_to_the_member_value_type():
    system = machine()
    system.record_point("temperature", 12, 22)

    temperature = points(system, "temperature")
    assert temperature.values.tolist() == [21.5, 22.0]


def test_recording_into_an_unknown_attribute_is_refused():
    with pytest.raises(RecordError, match="no sequence for `humidity`"):
        machine().record_point("humidity", 10, 0.4)


def test_recording_the_wrong_shape_is_refused():
    with pytest.raises(RecordError, match="other shape"):
        machine().record_point("up", 10, 1.0)


def test_a_failing_batch_records_nothing():
    system = machine()
    batch = Batch().points("temperature", [11], [21.7]).points("humidity", [10], [0.4])

    with pytest.raises(RecordError):
        system.record_batch(batch)
    assert len(points(system, "temperature")) == 1


def test_a_batch_records_every_push():
    system = machine()
    batch = (
        Batch()
        .points("temperature", [12, 11], [21.9, 21.7])
        .points("log", [11], ["fan on"])
        .spans("up", [60], [70])
    )
    system.record_batch(batch)

    assert len(batch) == 4
    assert points(system, "temperature").timestamps.tolist() == [10, 11, 12]
    assert points(system, "log").values.tolist() == ["boot", "fan on"]
    assert timeline(system, "up").spans.tolist() == [[0, 70]]


def test_handles_record_through_the_system():
    system = machine()
    temperature = points(system, "temperature")
    temperature.insert(11, 21.7)
    temperature.extend([12], [21.9])

    assert len(points(system, "temperature")) == 3


def test_changes_the_system_forbids_are_refused_on_handles():
    handle = points(machine(), "temperature")

    with pytest.raises(TypeError, match="belongs to a SequenceSystem"):
        handle.clear()
    with pytest.raises(TypeError, match="belongs to a SequenceSystem"):
        handle.set_attribute("humidity")


def test_insert_returns_what_it_displaces():
    system = machine()
    displaced = system.insert(TimePointSequence("up", [1], [True]))

    assert isinstance(displaced, TimeIntervalSequence)
    assert displaced.spans.tolist() == [[0, 60]]
    assert points(system, "up").dtype == "bool"


def test_remove_hands_back_an_independent_sequence():
    system = machine()
    removed = system.remove("temperature")

    assert isinstance(removed, TimePointSequence)
    removed.clear()
    assert "temperature" not in system
    assert system.remove("temperature") is None


def test_derived_timelines_are_filed_under_their_label():
    system = machine()
    busy = TimeIntervalSequence("busy", [10], [20])
    up = timeline(system, "up")
    system.insert(up - busy)

    assert "up \\ busy" in system


def test_systems_compare_by_their_members():
    assert machine() == machine()
    assert repr(machine()) == "SequenceSystem(['log', 'temperature', 'up'])"
