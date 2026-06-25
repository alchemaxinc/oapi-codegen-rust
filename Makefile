SHELL := /bin/bash

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
	sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-35s\033[0m %s\n", $$1, $$2}'

.PHONY: clean
clean: ## Clean up built files
	cargo clean

.PHONY: run
run: ## Run the CLI
	cargo run -p oapi-codegen

.PHONY: lint
lint: ## Run linter
	cargo clippy \
		--all-targets \
		-- -D warnings
	cargo +nightly fmt \
		-- --check
	npx prettier --check .

.PHONY: format
format: ## Format files
	cargo clippy \
		--all-targets \
		--fix \
		--allow-dirty
	cargo +nightly fmt
	npx prettier --write .

.PHONY: build
build: ## Build all Rust crates
	cargo build --release

.PHONY: test-unit
test-unit: ## Run unit tests
	cargo test

.PHONY: test-integration
test-integration: ## Run integration tests
	cargo test --features integration --test '*_integration' -- --nocapture

.PHONY: update-generated
update-generated: ## Refresh generated files from the coverage fixtures
	UPDATE_GENERATED=1 cargo test -p oapi-codegen --test coverage

.PHONY: generate-example
generate-example: ## Regenerate the composed bookstore example from its OpenAPI spec
	cd examples/bookstore && \
		cargo run -q -p oapi-codegen -- schemas/common.yaml --config oapi-codegen-common.yaml && \
		cargo run -q -p oapi-codegen -- schemas/catalog.yaml --config oapi-codegen-catalog.yaml && \
		cargo run -q -p oapi-codegen -- openapi.yaml --config oapi-codegen-server.yaml

.PHONY: docs
docs: ## Generate and open Rust documentation
	cargo doc --no-deps --open
