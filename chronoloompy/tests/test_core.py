"""The compiled extension loads, and the package exposes exactly its classes."""

import chronoloompy
from chronoloompy import _core


def test_package_reexports_the_extension():
    for name in chronoloompy.__all__:
        assert getattr(chronoloompy, name) is getattr(_core, name)


def test_classes_report_the_public_module():
    assert chronoloompy.TimePointSequence.__module__ == "chronoloompy"
