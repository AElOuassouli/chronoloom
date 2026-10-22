"""Narrowing a system member to the shape a test knows it has."""

from chronoloompy import SequenceSystem, TimeIntervalSequence, TimePointSequence


def points(system: SequenceSystem, attribute: str) -> TimePointSequence:
    member = system[attribute]
    assert isinstance(member, TimePointSequence)
    return member


def timeline(system: SequenceSystem, attribute: str) -> TimeIntervalSequence:
    member = system[attribute]
    assert isinstance(member, TimeIntervalSequence)
    return member
