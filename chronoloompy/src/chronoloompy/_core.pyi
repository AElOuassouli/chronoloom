"""Type stubs for the Rust extension module `chronoloompy._core`.

Implemented in ../../rust, which is a thin pyo3 adapter over the `chronoloom`
crate. Keep these signatures in sync with the `#[pyfunction]`s there.
"""

def intersection(a: tuple[int, int], b: tuple[int, int]) -> tuple[int, int] | None: ...
