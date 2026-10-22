"""Type stubs for the Rust extension module `chronoloompy._core`.

Implemented in ../../rust, a thin pyo3 adapter over the `chronoloom` crate.
Keep these signatures in sync with the `#[pymethods]` there.

# Memory model

Sequences live in Rust memory. A `TimePointSequence` or `TimeIntervalSequence`
either owns its events or is a handle onto a member of a `SequenceSystem`:
`system["temperature"]` copies nothing, and `SequenceSystem.insert` moves a
sequence's events into the system, after which the object is a handle onto
them there.

`timestamps`, `values`, `spans`, `to_numpy()` and `range()` return read-only
NumPy arrays over that memory, not copies; the only exception is `str` values,
which NumPy cannot read in place and are copied. While any such array is
alive, a change that could move or free the memory raises `BufferError` — the
same rule `bytearray` follows. Delete the arrays, or copy them with
`numpy.array(view)`, to change the sequence again.
"""

from collections.abc import Iterator
from typing import Any, Literal, Self

import numpy as np
from numpy.typing import ArrayLike, NDArray

type ValueType = Literal["float64", "int64", "bool", "str"]

class IntervalError(ValueError):
    """A span whose end is not after its start, or a bound shifted out of range."""

class RecordError(ValueError):
    """Events that could not be recorded into a system. Nothing was recorded."""

class TimePointEvent:
    """A value observed at a single instant, with no duration."""

    def __init__(self, timestamp: int, value: Any) -> None: ...
    @property
    def timestamp(self) -> int: ...
    @property
    def value(self) -> Any: ...

class TimeIntervalEvent:
    """A value attached to the half-open span `[start, end)`.

    `start` is included and `end` excluded, so intervals that merely touch share
    no instant. `end` must be strictly after `start`, or `IntervalError` is
    raised.
    """

    def __init__(self, start: int, end: int, value: Any = None) -> None: ...
    @property
    def start(self) -> int: ...
    @property
    def end(self) -> int: ...
    @property
    def value(self) -> Any: ...
    def bounds(self) -> tuple[int, int]: ...
    def duration(self) -> int: ...
    def intersection(self, other: TimeIntervalEvent) -> TimeIntervalEvent | None:
        """The span where both are active; touching intervals do not intersect."""

    def merged(self, other: TimeIntervalEvent) -> TimeIntervalEvent | None:
        """The single span covering both, or `None` when a gap separates them."""

    def union(self, other: TimeIntervalEvent) -> list[TimeIntervalEvent]:
        """The spans covered by either: one when they combine, else two by start."""

class TimePointSequence:
    """A time-ordered sequence of point events, held in Rust.

    Events may arrive in any order; the sequence always reads oldest first, and
    within one instant in the order the events were added. Values are one of
    `float64`, `int64`, `bool` or `str`, inferred from `values` unless `dtype`
    names one.
    """

    def __init__(
        self,
        attribute: str,
        timestamps: ArrayLike | None = None,
        values: ArrayLike | None = None,
        *,
        dtype: ValueType | None = None,
    ) -> None: ...
    @property
    def attribute(self) -> str: ...
    @property
    def label(self) -> str: ...
    @property
    def dtype(self) -> ValueType: ...
    @property
    def timestamps(self) -> NDArray[np.int64]:
        """Every timestamp, as a read-only view of the sequence's memory."""

    @property
    def values(self) -> NDArray[Any]:
        """Every value: a read-only view, or a copy for `str` values."""

    def to_numpy(self) -> NDArray[np.void]:
        """A read-only structured view of `timestamp` and `value` fields.

        Raises `TypeError` for `str` values, which cannot be viewed in place.
        """

    def range(
        self, start: int | None = None, end: int | None = None
    ) -> tuple[NDArray[np.int64], NDArray[Any]]:
        """The timestamps and values in `[start, end)`; either bound may be omitted."""

    def get(self, timestamp: int) -> list[TimePointEvent]:
        """Every event at exactly `timestamp`."""

    def contains(self, timestamp: int) -> bool: ...
    def instant_count(self) -> int: ...
    def first(self) -> TimePointEvent | None: ...
    def last(self) -> TimePointEvent | None: ...
    def before(self, timestamp: int) -> TimePointEvent | None:
        """The last event at or before `timestamp`."""

    def after(self, timestamp: int) -> TimePointEvent | None:
        """The first event at or after `timestamp`."""

    def nearest(self, timestamp: int) -> TimePointEvent | None:
        """The closest event in either direction, the earlier one on a tie."""

    def insert(self, timestamp: int, value: Any) -> None: ...
    def extend(self, timestamps: ArrayLike, values: ArrayLike) -> None: ...
    def remove(self, timestamp: int) -> list[TimePointEvent]:
        """Take every event at `timestamp` out. Not available on a system member."""

    def clear(self) -> None:
        """Drop every event. Not available on a system member."""

    def set_attribute(self, attribute: str) -> None:
        """Change what the sequence records. Not available on a system member."""

    def __len__(self) -> int: ...
    def __getitem__(self, index: int) -> TimePointEvent: ...
    def __iter__(self) -> Iterator[TimePointEvent]: ...
    def __eq__(self, other: object) -> bool: ...

class TimeIntervalSequence:
    """The spans during which one state was active, held in Rust.

    Kept normalized: sorted, disjoint, and with touching spans merged, so two
    timelines of one state are equal exactly when they cover the same instants.
    Set operations and `transform` describe their result in `label`, such as
    `up[1, 2] ∪ busy`.
    """  # noqa: RUF002 -- the label really is written with the set symbol

    def __init__(
        self,
        attribute: str,
        starts: ArrayLike | None = None,
        ends: ArrayLike | None = None,
    ) -> None: ...
    @staticmethod
    def from_spans(attribute: str, spans: ArrayLike) -> TimeIntervalSequence:
        """Build from an `(n, 2)` array of `[start, end]` rows."""

    @property
    def attribute(self) -> str: ...
    @property
    def label(self) -> str: ...
    @property
    def transformation(self) -> tuple[int, int]: ...
    @property
    def spans(self) -> NDArray[np.int64]:
        """Every span as an `(n, 2)` read-only view of `[start, end)` rows."""

    def active_duration(self) -> int: ...
    def at(self, timestamp: int) -> TimeIntervalEvent | None: ...
    def contains(self, timestamp: int) -> bool: ...
    def covers_same(self, other: TimeIntervalSequence) -> bool: ...
    def transform(self, alpha: int, beta: int) -> TimeIntervalSequence:
        """Every `[s, e)` moved to `[s + alpha, e + beta)`, re-normalized."""

    def union(self, other: TimeIntervalSequence) -> TimeIntervalSequence: ...
    def intersection(self, other: TimeIntervalSequence) -> TimeIntervalSequence: ...
    def difference(self, other: TimeIntervalSequence) -> TimeIntervalSequence: ...
    def symmetric_difference(
        self, other: TimeIntervalSequence
    ) -> TimeIntervalSequence: ...
    def __or__(self, other: TimeIntervalSequence) -> TimeIntervalSequence: ...
    def __and__(self, other: TimeIntervalSequence) -> TimeIntervalSequence: ...
    def __sub__(self, other: TimeIntervalSequence) -> TimeIntervalSequence: ...
    def __xor__(self, other: TimeIntervalSequence) -> TimeIntervalSequence: ...
    def insert(self, start: int, end: int) -> None:
        """Add `[start, end)`, merging it into what is already covered."""

    def extend(self, starts: ArrayLike, ends: ArrayLike) -> None: ...
    def remove(self, index: int) -> TimeIntervalEvent:
        """Take the span at `index` out. Not available on a system member."""

    def clear(self) -> None:
        """Drop every span. Not available on a system member."""

    def set_attribute(self, attribute: str) -> None:
        """Describe the timeline by `attribute`, forgetting its derivation."""

    def __len__(self) -> int: ...
    def __getitem__(self, index: int) -> TimeIntervalEvent: ...
    def __iter__(self) -> Iterator[TimeIntervalEvent]: ...
    def __eq__(self, other: object) -> bool: ...

type AnySequence = TimePointSequence | TimeIntervalSequence

class SequenceSystem:
    """A set of sequences describing one subject, one per attribute.

    Indexing returns a handle onto a member, not a copy. Recording never
    creates a sequence: an attribute must be added before it is recorded into,
    or `RecordError` is raised.
    """

    def __init__(self) -> None: ...
    def insert(self, sequence: AnySequence) -> AnySequence | None:
        """Move `sequence` in under its label, returning what was filed there.

        `sequence` becomes a handle onto the member.
        """

    def add_points(
        self,
        attribute: str,
        timestamps: ArrayLike,
        values: ArrayLike,
        *,
        dtype: ValueType | None = None,
    ) -> TimePointSequence:
        """Build a point sequence in place, returning a handle onto it."""

    def add_spans(
        self, attribute: str, starts: ArrayLike, ends: ArrayLike
    ) -> TimeIntervalSequence:
        """Build a timeline in place, returning a handle onto it."""

    def remove(self, attribute: str) -> AnySequence | None:
        """Take the sequence out, as an object owning its events."""

    def clear(self) -> None: ...
    def get(self, attribute: str) -> AnySequence | None: ...
    def attributes(self) -> list[str]: ...
    def items(self) -> list[tuple[str, AnySequence]]: ...
    def record_point(self, attribute: str, timestamp: int, value: Any) -> None: ...
    def record_span(self, attribute: str, start: int, end: int) -> None: ...
    def record_batch(self, batch: Batch) -> None:
        """Record every event in `batch`, all or nothing."""

    def __getitem__(self, attribute: str) -> AnySequence: ...
    def __contains__(self, attribute: str) -> bool: ...
    def __len__(self) -> int: ...
    def __iter__(self) -> Iterator[str]: ...
    def __eq__(self, other: object) -> bool: ...

class Batch:
    """Events for many sequences of a system, recorded together.

    Each push is converted when the batch is recorded, to the value type of the
    sequence it is for.
    """

    def __init__(self) -> None: ...
    def points(
        self, attribute: str, timestamps: ArrayLike, values: ArrayLike
    ) -> Self: ...
    def spans(self, attribute: str, starts: ArrayLike, ends: ArrayLike) -> Self: ...
    def __len__(self) -> int: ...
