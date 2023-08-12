from typing import Any

from pydantic import BaseModel, PositiveInt


class TimePointEvent(BaseModel):
    timestamp: PositiveInt
    value: Any
    attribute: str | None = None
