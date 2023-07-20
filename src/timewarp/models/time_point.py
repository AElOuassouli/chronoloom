from pydantic import BaseModel, PositiveInt


class TimePoint(BaseModel):
    timestamp: PositiveInt
