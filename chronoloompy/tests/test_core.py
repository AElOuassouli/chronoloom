"""Smoke test for the compiled pyo3 extension.

`_core` exports nothing while the binding waits on a `chronoloom` release
carrying the new API, but importing it still proves the Rust extension built
and loads under the name the wheel expects.
"""

import importlib


def test_extension_module_imports():
    assert importlib.import_module("chronoloompy._core") is not None
