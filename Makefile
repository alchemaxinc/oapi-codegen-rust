SHELL := /bin/bash

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
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

.PHONY: test-e2e
test-e2e: ## Run the Docker end-to-end test (generated server + client over HTTP)
	@compose="docker compose -f crates/oapi-codegen/tests/integration/docker-compose.yml"; \
	trap 'code=$$?; $$compose down --remove-orphans --volumes; exit $$code' EXIT; \
	$$compose up --build --exit-code-from client --abort-on-container-exit

.PHONY: update-generated
update-generated: ## Refresh generated files from the coverage fixtures
	UPDATE_GENERATED=1 cargo test -p oapi-codegen --test coverage

.PHONY: generate-example
generate-example: ## Regenerate the composed bookstore example from its OpenAPI spec
	cd examples/bookstore && \
		cargo run -q -p oapi-codegen -- schemas/common.yaml --config-file oapi-codegen-common.yaml && \
		cargo run -q -p oapi-codegen -- schemas/catalog.yaml --config-file oapi-codegen-catalog.yaml && \
		cargo run -q -p oapi-codegen -- openapi.yaml --config-file oapi-codegen-server.yaml && \
		cargo run -q -p oapi-codegen -- openapi.yaml --config-file oapi-codegen-client.yaml

.PHONY: verify-generated
verify-generated: ## Regenerate all generated code and fail if it drifts from what is committed
	$(MAKE) generate-example
	$(MAKE) update-generated
	@if [ -n "$$(git status --porcelain -- examples/bookstore/generated crates/oapi-codegen/tests/generated)" ]; then \
		echo "ERROR: generated code is out of date."; \
		echo "Run 'make generate-example' and 'make update-generated', then commit the result."; \
		git status --porcelain -- examples/bookstore/generated crates/oapi-codegen/tests/generated; \
		git --no-pager diff -- examples/bookstore/generated crates/oapi-codegen/tests/generated; \
		exit 1; \
	fi
	@echo "Generated code is up to date."

.PHONY: docs
docs: ## Generate and open Rust documentation
	cargo doc --no-deps --open
