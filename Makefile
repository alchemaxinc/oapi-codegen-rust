SHELL := /bin/bash

.PHONY: help
help: ## Show this help text
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
	sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-35s\033[0m %s\n", $$1, $$2}'

.PHONY: clean
clean: ## Remove built files
	cargo clean

.PHONY: run
run: ## Run the CLI
	cargo run -p oapi-codegen

.PHONY: lint
lint: ## Run the linter
	cargo clippy \
		--all-targets \
		--workspace \
		--locked \
		-- -D warnings
	RUSTDOCFLAGS="-D warnings -D rustdoc::broken_intra_doc_links -D rustdoc::private_intra_doc_links" \
		cargo doc --no-deps --workspace --locked
	cargo +nightly fmt \
		-- --check
	npx prettier --check .

.PHONY: format
format: ## Format source files
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
test-e2e: ## Run the Docker end-to-end test for the generated server and client
	@compose="docker compose -f crates/oapi-codegen/tests/integration/docker-compose.yml"; \
	trap 'code=$$?; $$compose down --remove-orphans --volumes; exit $$code' EXIT; \
	$$compose up --build --exit-code-from client --abort-on-container-exit

.PHONY: update-generated
update-generated: ## Refresh generated files from the coverage fixtures
	UPDATE_GENERATED=1 cargo test -p oapi-codegen --test coverage

.PHONY: update-docs
update-docs: ## Refresh docs/cli.md from the clap CLI definition
	UPDATE_DOCS=1 cargo test -p oapi-codegen --test cli_docs

.PHONY: generate-example
generate-example: ## Regenerate the bookstore example from its OpenAPI specification
	cd examples/bookstore && \
		cargo run -q -p oapi-codegen -- --config-file oapi-codegen-common.yaml schemas/common.yaml && \
		cargo run -q -p oapi-codegen -- --config-file oapi-codegen-catalog.yaml schemas/catalog.yaml && \
		cargo run -q -p oapi-codegen -- --config-file oapi-codegen-server.yaml openapi.yaml && \
		cargo run -q -p oapi-codegen -- --config-file oapi-codegen-client.yaml openapi.yaml

.PHONY: verify-example
verify-example: ## Fail when the bookstore example is out of date, without writing to it
	cd examples/bookstore && \
		cargo run -q -p oapi-codegen -- --check --config-file oapi-codegen-common.yaml schemas/common.yaml && \
		cargo run -q -p oapi-codegen -- --check --config-file oapi-codegen-catalog.yaml schemas/catalog.yaml && \
		cargo run -q -p oapi-codegen -- --check --config-file oapi-codegen-server.yaml openapi.yaml && \
		cargo run -q -p oapi-codegen -- --check --config-file oapi-codegen-client.yaml openapi.yaml

.PHONY: verify-generated
verify-generated: ## Regenerate all generated files and fail when they differ from committed files
	$(MAKE) verify-example
	$(MAKE) update-generated
	$(MAKE) update-docs
	@if [ -n "$$(git status --porcelain -- examples/bookstore/generated crates/oapi-codegen/tests/generated docs/cli.md)" ]; then \
		echo "ERROR: generated files are out of date."; \
		echo "Run 'make generate-example', 'make update-generated' and 'make update-docs'. Commit the result."; \
		git status --porcelain -- examples/bookstore/generated crates/oapi-codegen/tests/generated docs/cli.md; \
		git --no-pager diff -- examples/bookstore/generated crates/oapi-codegen/tests/generated docs/cli.md; \
		exit 1; \
	fi
	@echo "Generated files are up to date."

.PHONY: verify-package
verify-package: ## Fail when the published package does not build or run, or when it carries the test suite
	@set -euo pipefail; \
	files="$$(cargo package -p oapi-codegen --locked --list)"; \
	if grep -q '^tests/' <<< "$$files"; then \
		echo "ERROR: the package carries the test suite."; \
		echo "Check 'include' in crates/oapi-codegen/Cargo.toml."; \
		exit 1; \
	fi
	cargo package -p oapi-codegen --locked
	@set -euo pipefail; \
	version="$$(cargo metadata --format-version 1 --no-deps \
		| sed -n 's/.*"name":"oapi-codegen","version":"\([^"]*\)".*/\1/p')"; \
	work="$$(mktemp -d)"; \
	trap 'rm -rf "$$work"' EXIT; \
	tar xzf target/package/oapi-codegen-$$version.crate -C "$$work"; \
	cd "$$work/oapi-codegen-$$version" && cargo build --locked -q; \
	cp -R $(CURDIR)/examples/bookstore "$$work/example"; \
	cd "$$work/example" && \
		"$$work/oapi-codegen-$$version/target/debug/oapi-codegen" \
			--config-file oapi-codegen-catalog.yaml schemas/catalog.yaml; \
	diff -u $(CURDIR)/examples/bookstore/generated/apimodel/catalog.rs \
		"$$work/example/generated/apimodel/catalog.rs"
	@echo "The package builds, runs, and carries no tests."

.PHONY: docs
docs: ## Generate and open Rust documentation
	cargo doc --no-deps --open
