.DEFAULT_GOAL := help
CARGO_MANIFEST := rust/Cargo.toml

setup: ## Create the dev environment and install git hooks
	uv sync
	uv run pre-commit install

install: ## Sync the environment (rebuilds the Rust extension if needed)
	uv sync

clean: ## Remove build artifacts and caches
	rm -rf .venv rust/target dist .pytest_cache .ruff_cache .mypy_cache .coverage
	rm -f src/timewarp/*.so
	find . -name __pycache__ -type d -prune -exec rm -rf {} +

fmt: ## Format Python and Rust
	uv run ruff format .
	uv run ruff check --fix .
	cargo fmt --manifest-path $(CARGO_MANIFEST) --all

lint: ## Lint and type-check everything
	uv run ruff check .
	uv run ruff format --check .
	uv run mypy
	cargo fmt --manifest-path $(CARGO_MANIFEST) --all -- --check
	cargo clippy --manifest-path $(CARGO_MANIFEST) --all-targets -- -D warnings

test: ## Run Python and Rust tests
	uv run pytest --cov=timewarp
	cargo test --manifest-path $(CARGO_MANIFEST)

build: ## Build a release wheel into dist/
	uv build

help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-25s\033[0m %s\n", $$1, $$2}'

.PHONY: setup install clean fmt lint test build help
