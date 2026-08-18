.DEFAULT_GOAL := help
CARGO_MANIFEST := rust/Cargo.toml

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

sdist: ## Build an sdist (exercises vendoring of the ../chronoloom path dep)
	uv run maturin sdist --out dist

publish: ## Publish to PyPI (normally done by CI on a tag)
	uv publish

help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-25s\033[0m %s\n", $$1, $$2}'

.PHONY: install clean fmt lint test build sdist publish help
