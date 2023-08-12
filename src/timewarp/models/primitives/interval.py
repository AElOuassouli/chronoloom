from typing import Any

from pydantic import BaseModel


class TimeInterval(BaseModel):
    start_timestamp: int
    end_timestamp: int
    attribute: Any
    left_open: bool = False
    right_open: bool = True

    ## TODO: add validator to ensure that start_timestamp < end_timestamp
