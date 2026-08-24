"""The point event primitive: a value observed at a single instant."""

from typing import Any

from pydantic import BaseModel


class TimePointEvent(BaseModel):
    """A value observed at a single instant, with no duration.

    Any timestamp is valid, including zero and negative ones — the epoch is the
    caller's to choose.

    Mirrors `chronoloom::primitives::TimePointEvent` on the Rust side.
    """

    timestamp: int
    value: Any
