"""The interval event primitive: a value attached to a span of time."""

from typing import Any, Self

from pydantic import BaseModel, model_validator


class TimeIntervalEvent(BaseModel):
    """A value attached to the half-open span `[start, end)`.

    `start_timestamp` is included and `end_timestamp` is excluded, so two
    intervals that merely touch share no instant. The span is never empty:
    `end_timestamp` must be strictly after `start_timestamp`, which rules out
    both empty and inverted intervals.

    Mirrors `chronoloom::primitives::TimeIntervalEvent` on the Rust side.
    """

    start_timestamp: int
    end_timestamp: int
    value: Any | None = None

    @model_validator(mode="after")
    def _reject_empty_spans(self) -> Self:
        if self.end_timestamp <= self.start_timestamp:
            message = (
                f"interval end ({self.end_timestamp}) must be strictly after "
                f"its start ({self.start_timestamp})"
            )
            raise ValueError(message)
        return self
