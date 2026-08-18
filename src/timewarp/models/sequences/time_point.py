import numpy as np
from pydantic import BaseModel

from timewarp.models import TimePointEvent


class TimePointSequence(BaseModel):
    sequence: np.ndarray
    attribute: str | None = None

    def __init__(self, sequence: list[TimePointEvent], attribute: str):
        self.sequence = np.array(
            [(point.timestamp, point.value) for point in sequence],
            dtype=[("timestamp", int), ("value", float)],
        )
        self.attribute = attribute
