.DEFAULT_GOAL := help
CARGO_MANIFEST := rust/Cargo.toml

# See the note in the repo-root makefile: pin pyo3 to this package's venv so an
# unrelated or stale $VIRTUAL_ENV cannot hijack the binding build. Inherited
# from the root makefile when invoked via `make -C`, set here for direct use.
VENV_PYTHON := $(wildcard $(CURDIR)/.venv/bin/python)
ifneq ($(VENV_PYTHON),)
export PYO3_PYTHON := $(VENV_PYTHON)
endif

install: ## Sync the environment (rebuilds the Rust extension if needed)
	uv sync

clean: ## Remove build artifacts and caches
	rm -rf .venv rust/target dist .pytest_cache .ruff_cache .mypy_cache .coverage
	rm -f src/chronoloompy/*.so
	find . -name __pycache__ -type d -prune -exec rm -rf {} +

fmt: ## Format Python and the binding crate
	uv run ruff format .
	uv run ruff check --fix .
	cargo fmt --manifest-path $(CARGO_MANIFEST) --all

lint: ## Lint and type-check Python and the binding crate
	uv run ruff check .
	uv run ruff format --check .
	uv run mypy
	cargo fmt --manifest-path $(CARGO_MANIFEST) --all -- --check
	cargo clippy --manifest-path $(CARGO_MANIFEST) --all-targets -- -D warnings

test: ## Run Python and binding-crate tests
	uv run pytest --cov=chronoloompy
	cargo test --manifest-path $(CARGO_MANIFEST)

build: ## Build a release wheel into dist/
	uv build

# --manifest-path matches what CI passes. Without it maturin probes ./Cargo.toml
# first and prints a spurious "manifest path does not exist" error before
# falling back to the path configured in pyproject.toml.
sdist: ## Build an sdist (the install path for platforms without a wheel)
	uv run maturin sdist --out dist --manifest-path $(CARGO_MANIFEST)

publish: ## Publish to PyPI (normally done by CI on a tag)
	uv publish

help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-25s\033[0m %s\n", $$1, $$2}'

.PHONY: install clean fmt lint test build sdist publish help
