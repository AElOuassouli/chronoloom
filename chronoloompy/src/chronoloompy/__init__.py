"""Temporal sequences held in Rust, manipulated from Python.

Every class here is implemented by the `chronoloom` Rust crate. Python objects
are handles onto data in Rust memory, and reads return read-only NumPy views of
that memory rather than copies.
"""

from chronoloompy._core import (
    Batch,
    IntervalError,
    RecordError,
    SequenceSystem,
    TimeIntervalEvent,
    TimeIntervalSequence,
    TimePointEvent,
    TimePointSequence,
)

__all__ = [
    "Batch",
    "IntervalError",
    "RecordError",
    "SequenceSystem",
    "TimeIntervalEvent",
    "TimeIntervalSequence",
    "TimePointEvent",
    "TimePointSequence",
]
