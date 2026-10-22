"""The memory model: data stays in Rust, and Python reads it in place."""

import gc

import numpy as np
import pytest

from chronoloompy import SequenceSystem, TimeIntervalSequence, TimePointSequence
from tests.members import points, timeline


def readings() -> TimePointSequence:
    return TimePointSequence("temperature", [10, 20, 30], [1.0, 2.0, 3.0])


def address(array: np.ndarray) -> int:
    data: tuple[int, bool] = array.__array_interface__["data"]
    return data[0]


def test_reads_share_one_buffer_rather_than_copying():
    sequence = readings()
    timestamps = sequence.timestamps

    # Each event is a timestamp then a value, sixteen bytes in all.
    assert address(sequence.timestamps) == address(timestamps)
    assert address(sequence.values) == address(timestamps) + 8
    assert timestamps.strides == (16,)
    assert address(sequence.range(20, 30)[0]) == address(timestamps) + 16
    assert np.shares_memory(sequence.to_numpy(), timestamps)


def test_views_are_read_only():
    timestamps = readings().timestamps

    assert not timestamps.flags.writeable
    with pytest.raises(ValueError, match="read-only"):
        timestamps[0] = 0


def test_changing_a_viewed_sequence_is_refused_until_the_view_is_gone():
    sequence = readings()
    timestamps = sequence.timestamps

    with pytest.raises(BufferError):
        sequence.insert(40, 4.0)
    with pytest.raises(BufferError):
        sequence.clear()

    del timestamps
    gc.collect()
    sequence.insert(40, 4.0)
    assert len(sequence) == 4


def test_a_copy_releases_the_sequence():
    sequence = readings()
    copy = np.array(sequence.timestamps)
    sequence.clear()

    assert copy.tolist() == [10, 20, 30]


def test_renaming_a_viewed_sequence_moves_no_memory_and_is_allowed():
    sequence = readings()
    timestamps = sequence.timestamps
    sequence.set_attribute("humidity")

    assert timestamps.tolist() == [10, 20, 30]


def test_inserting_moves_the_events_and_leaves_a_handle():
    system = SequenceSystem()
    sequence = readings()
    system.insert(sequence)

    sequence.insert(40, 4.0)
    assert len(points(system, "temperature")) == 4
    assert np.shares_memory(
        sequence.timestamps, points(system, "temperature").timestamps
    )


def test_a_handle_cannot_be_inserted_twice():
    system = SequenceSystem()
    handle = system.add_points("temperature", [1], [1.0])

    with pytest.raises(TypeError, match="already belongs"):
        SequenceSystem().insert(handle)


def test_a_viewed_sequence_cannot_be_moved():
    sequence = readings()
    timestamps = sequence.timestamps

    with pytest.raises(BufferError):
        SequenceSystem().insert(sequence)
    assert timestamps.tolist() == [10, 20, 30]


def test_a_viewed_member_cannot_be_changed_removed_or_replaced():
    system = SequenceSystem()
    system.add_points("temperature", [10], [1.0])
    values = points(system, "temperature").values

    with pytest.raises(BufferError):
        system.record_point("temperature", 20, 2.0)
    with pytest.raises(BufferError):
        system.remove("temperature")
    with pytest.raises(BufferError):
        system.insert(TimePointSequence("temperature"))
    with pytest.raises(BufferError):
        system.clear()
    assert values.tolist() == [1.0]


def test_other_members_stay_writable_while_one_is_viewed():
    system = SequenceSystem()
    system.add_points("temperature", [10], [1.0])
    system.add_spans("up", [0], [5])
    spans = timeline(system, "up").spans

    for timestamp in range(1000):
        system.record_point("temperature", timestamp, 0.0)
    system.add_points("humidity", [1], [0.4])

    assert spans.tolist() == [[0, 5]]


def test_a_view_keeps_its_system_alive():
    system = SequenceSystem()
    system.add_spans("up", [0, 20], [10, 30])
    spans = timeline(system, "up").spans
    del system
    gc.collect()

    assert spans.tolist() == [[0, 10], [20, 30]]


def test_text_values_are_copied_so_they_do_not_pin_the_sequence():
    sequence = TimePointSequence("log", [1], ["boot"])
    values = sequence.values
    sequence.insert(2, "fan on")

    assert values.tolist() == ["boot"]


def test_interval_views_follow_the_same_rule():
    timeline = TimeIntervalSequence("up", [0], [10])
    spans = timeline.spans

    with pytest.raises(BufferError):
        timeline.insert(20, 30)
    del spans
    gc.collect()
    timeline.insert(20, 30)
    assert len(timeline) == 2
