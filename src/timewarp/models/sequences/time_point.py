"""Sequence models backed by NumPy structured arrays."""

from collections.abc import Sequence

import numpy as np
from pydantic import BaseModel, ConfigDict

from timewarp.models.primitives import TimePointEvent

SEQUENCE_DTYPE = np.dtype([("timestamp", np.int64), ("value", np.float64)])


class TimePointSequence(BaseModel):
    """A time-ordered sequence of point events, stored as a NumPy array."""

    model_config = ConfigDict(arbitrary_types_allowed=True, frozen=True)

    sequence: np.ndarray
    attribute: str | None = None

    @classmethod
    def from_events(
        cls,
        events: Sequence[TimePointEvent],
        attribute: str | None = None,
    ) -> "TimePointSequence":
        """Build a sequence from a list of point events."""
        return cls(
            sequence=np.array(
                [(event.timestamp, event.value) for event in events],
                dtype=SEQUENCE_DTYPE,
            ),
            attribute=attribute,
        )
