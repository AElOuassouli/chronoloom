.DEFAULT_GOAL := help

fmt: ## Format Rust sources
	cargo fmt --all

lint: ## Lint with and without optional features, and assert the dependency tree
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings
	cargo clippy --all-targets --all-features -- -D warnings
	@# The entire point of this crate is that Rust consumers get it without
	@# dragging in pyo3. That is easy to regress by adding one convenience
	@# #[pyfunction], so assert it rather than trusting review.
	@if cargo tree --quiet --edges normal | grep -q pyo3; then \
		echo "error: pyo3 leaked into chronoloom's dependency tree"; \
		echo "hint: bindings belong in ../chronoloompy/rust, not here"; \
		exit 1; \
	fi
	@# The default build has no dependencies at all: what a plain user of the
	@# algebra does not need, such as serde, sits behind a feature.
	@if [ "$$(cargo tree --quiet --edges normal | grep -c .)" != "1" ]; then \
		echo "error: the default build of chronoloom has dependencies"; \
		echo "hint: put them behind an optional feature"; \
		cargo tree --edges normal; \
		exit 1; \
	fi

test: ## Run all tests, doc tests included, with all features
	@# Once, with all features: the default build is already compiled by
	@# `make lint` (clippy --all-targets), and a second run of every test
	@# with default features doubled the check for little gain.
	cargo test --all-features

check: fmt lint test ## Format, lint and test

check-publish: ## Validate crates.io packaging without burning a version
	cargo publish --dry-run

publish: ## Publish to crates.io (needs CARGO_REGISTRY_TOKEN)
	cargo publish

clean: ## Remove build artifacts
	cargo clean

release-fix: ## Bump the patch version, close the changelog, commit and tag
	@python3 ../.github/scripts/release.py chronoloom fix

release-minor: ## Bump the minor version, close the changelog, commit and tag
	@python3 ../.github/scripts/release.py chronoloom minor

release-major: ## Bump the major version, close the changelog, commit and tag
	@python3 ../.github/scripts/release.py chronoloom major

help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-25s\033[0m %s\n", $$1, $$2}'

.PHONY: fmt lint test check check-publish publish clean release-fix release-minor release-major help
