.DEFAULT_GOAL := help

# Two independent libraries, each with its own makefile. Targets here fan out
# to both so `make lint` / `make test` still cover the whole repo.
PROJECTS := chronoloom chronoloompy

CORE_MANIFEST := chronoloom/Cargo.toml
EXT_MANIFEST := chronoloompy/rust/Cargo.toml

# pyo3's build script locates its interpreter from $VIRTUAL_ENV, so whatever
# venv happens to be active in the caller's shell hijacks the binding build --
# including a stale one still pointing at a path that no longer exists (an
# activated shell keeps its old export across a directory rename). PYO3_PYTHON
# takes precedence over $VIRTUAL_ENV, so pin it to the Python library's own
# venv. Empty when there is no venv -- the Rust-only CI jobs never create one
# and must keep using pyo3's own discovery.
EXT_VENV_PYTHON := $(wildcard $(CURDIR)/chronoloompy/.venv/bin/python)
ifneq ($(EXT_VENV_PYTHON),)
export PYO3_PYTHON := $(EXT_VENV_PYTHON)
endif

define for_each
	@for p in $(PROJECTS); do \
		printf '\033[1m==> %s\033[0m\n' "$$p"; \
		$(MAKE) --no-print-directory -C "$$p" $(1) || exit 1; \
	done
endef

setup: ## Create the dev environment and install git hooks
	$(MAKE) -C chronoloompy install
	uv run --project chronoloompy pre-commit install

fmt: ## Format both projects
	$(call for_each,fmt)

lint: ## Lint and type-check both projects
	$(call for_each,lint)

test: ## Test both projects
	$(call for_each,test)

clean: ## Remove build artifacts and caches from both projects
	$(call for_each,clean)

# Rust-only slices. The pre-commit hooks fire on .rs changes and use these so
# they do not pay for the Python toolchain.
fmt-rust: ## Format Rust in both crates
	cargo fmt --manifest-path $(CORE_MANIFEST) --all
	cargo fmt --manifest-path $(EXT_MANIFEST) --all

lint-rust: ## Check formatting and run clippy on both crates
	cargo fmt --manifest-path $(CORE_MANIFEST) --all -- --check
	cargo clippy --manifest-path $(CORE_MANIFEST) --all-targets -- -D warnings
	cargo fmt --manifest-path $(EXT_MANIFEST) --all -- --check
	cargo clippy --manifest-path $(EXT_MANIFEST) --all-targets -- -D warnings

test-rust: ## Run both crates' Rust tests
	cargo test --manifest-path $(CORE_MANIFEST)
	cargo test --manifest-path $(EXT_MANIFEST)

help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-25s\033[0m %s\n", $$1, $$2}'

.PHONY: setup fmt lint test clean fmt-rust lint-rust test-rust help
