from pydantic import BaseModel


class TimeInterval(BaseModel):
    start_timestamp: int
    end_timestamp: int

    ## TODO: add validator to ensure that start_timestamp < end_timestamp    ## TODO: add validator to ensure that start_timestamp < end_timestamp
