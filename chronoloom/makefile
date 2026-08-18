.DEFAULT_GOAL := help

fmt: ## Format Rust sources
	cargo fmt --all

lint: ## Lint, and assert the crate stays free of pyo3
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings
	@# The entire point of this crate is that Rust consumers get it without
	@# dragging in pyo3. That is easy to regress by adding one convenience
	@# #[pyfunction], so assert it rather than trusting review.
	@if cargo tree --quiet --edges normal | grep -q pyo3; then \
		echo "error: pyo3 leaked into chronoloom's dependency tree"; \
		echo "hint: bindings belong in ../chronoloompy/rust, not here"; \
		exit 1; \
	fi

test: ## Run unit and doc tests
	cargo test

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

.PHONY: fmt lint test check-publish publish clean release-fix release-minor release-major help
