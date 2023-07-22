PYTHON_FILES = `(find . -iname "*.py" -not -path "./.venv/*")`

setup-dev-end: ## Setup development environment
	poetry install
	poetry run pre-commit install

install: ## Install dependencies
	poetry install 

black: ## Run Black
	poetry run black --check --quiet $(PYTHON_FILES)

black-fix: ## Run Black with automated fix
	poetry run black $(PYTHON_FILES)

ruff: ## Run Ruff
	poetry run ruff check .

ruff-fix: ## Run Ruff with automated fix
	poetry run ruff check --fix .

code-fix: ## Run all automated code fix
	make ruff-fix
	make black-fix

run-tests : ## Run all tests
	poetry run pytest tests/ --cov=timewarp

help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-25s\033[0m %s\n", $$1, $$2}'