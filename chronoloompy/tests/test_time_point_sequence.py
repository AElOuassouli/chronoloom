import numpy as np
import pytest

from chronoloompy import TimePointEvent, TimePointSequence


def readings() -> TimePointSequence:
    return TimePointSequence("temperature", [30, 10, 20], [3.0, 1.0, 2.0])


def test_events_are_kept_in_time_order():
    sequence = readings()

    assert sequence.timestamps.tolist() == [10, 20, 30]
    assert sequence.values.tolist() == [1.0, 2.0, 3.0]


def test_value_type_is_inferred_from_the_values():
    assert readings().dtype == "float64"
    assert TimePointSequence("count", [1], [3]).dtype == "int64"
    assert TimePointSequence("up", [1], [True]).dtype == "bool"
    assert TimePointSequence("log", [1], ["boot"]).dtype == "str"


def test_dtype_overrides_inference():
    sequence = TimePointSequence("temperature", [1], [3], dtype="float64")

    assert sequence.values.dtype == np.float64


def test_unsupported_values_are_refused():
    with pytest.raises(TypeError, match="not supported"):
        TimePointSequence("blobs", [1], [object()])  # type: ignore[arg-type]


def test_float_timestamps_are_refused_rather_than_truncated():
    with pytest.raises(TypeError, match="integers"):
        TimePointSequence("temperature", [1.5], [1.0])


def test_mismatched_lengths_are_refused():
    with pytest.raises(ValueError, match="2 timestamps but 1 values"):
        TimePointSequence("temperature", [1, 2], [1.0])


def test_an_empty_sequence_needs_no_events():
    sequence = TimePointSequence("temperature")

    assert len(sequence) == 0
    assert sequence.timestamps.shape == (0,)
    assert sequence.first() is None


def test_range_is_half_open():
    timestamps, values = readings().range(10, 30)

    assert timestamps.tolist() == [10, 20]
    assert values.tolist() == [1.0, 2.0]


def test_range_bounds_may_be_omitted_or_inverted():
    sequence = readings()

    assert sequence.range(start=20)[0].tolist() == [20, 30]
    assert sequence.range(end=20)[0].tolist() == [10]
    assert sequence.range(30, 10)[0].tolist() == []


def test_neighbour_queries():
    sequence = readings()

    assert sequence.before(25) == TimePointEvent(20, 2.0)
    assert sequence.after(25) == TimePointEvent(30, 3.0)
    assert sequence.nearest(15) == TimePointEvent(10, 1.0)
    assert sequence.last() == TimePointEvent(30, 3.0)


def test_events_sharing_an_instant_keep_their_order():
    sequence = TimePointSequence("log", [5, 5], ["first", "second"])

    assert [event.value for event in sequence.get(5)] == ["first", "second"]
    assert len(sequence) == 2
    assert sequence.instant_count() == 1


def test_positional_access_and_iteration():
    sequence = readings()

    assert sequence[0] == TimePointEvent(10, 1.0)
    assert sequence[-1] == TimePointEvent(30, 3.0)
    assert [event.timestamp for event in sequence] == [10, 20, 30]
    with pytest.raises(IndexError):
        sequence[3]


def test_insert_extend_and_remove():
    sequence = readings()
    sequence.insert(15, 1.5)
    sequence.extend(np.array([40, 0]), np.array([4.0, 0.0]))

    assert sequence.timestamps.tolist() == [0, 10, 15, 20, 30, 40]
    assert sequence.remove(15) == [TimePointEvent(15, 1.5)]
    assert sequence.contains(15) is False


def test_text_values_come_back_as_strings():
    sequence = TimePointSequence("log", [2, 1], ["fan on", "boot"])

    assert sequence.values.tolist() == ["boot", "fan on"]
    assert sequence[0].value == "boot"
    with pytest.raises(TypeError, match="cannot be viewed"):
        sequence.to_numpy()


def test_structured_view_has_named_fields():
    structured = readings().to_numpy()

    assert structured["timestamp"].tolist() == [10, 20, 30]
    assert structured["value"].tolist() == [1.0, 2.0, 3.0]


def test_attribute_is_part_of_equality():
    renamed = readings()
    renamed.set_attribute("humidity")

    assert readings() == readings()
    assert renamed != readings()
    assert renamed.attribute == "humidity"
    assert repr(renamed) == "<TimePointSequence 'humidity': 3 float64 events>"
