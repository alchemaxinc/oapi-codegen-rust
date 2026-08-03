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

.PHONY: update-msrv-manifest
update-msrv-manifest: ## Refresh the msrv-check manifest from the dependency report
	UPDATE_MSRV_MANIFEST=1 cargo test -p oapi-codegen --test msrv_manifest

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
	$(MAKE) update-msrv-manifest
	@if [ -n "$$(git status --porcelain -- examples/bookstore/generated crates/oapi-codegen/tests/generated docs/cli.md crates/oapi-codegen/tests/msrv-check/Cargo.toml)" ]; then \
		echo "ERROR: generated files are out of date."; \
		echo "Run 'make generate-example', 'make update-generated', 'make update-docs' and 'make update-msrv-manifest'. Commit the result."; \
		git status --porcelain -- examples/bookstore/generated crates/oapi-codegen/tests/generated docs/cli.md crates/oapi-codegen/tests/msrv-check/Cargo.toml; \
		git --no-pager diff -- examples/bookstore/generated crates/oapi-codegen/tests/generated docs/cli.md crates/oapi-codegen/tests/msrv-check/Cargo.toml; \
		exit 1; \
	fi
	@echo "Generated files are up to date."

# The Rust version a consumer needs to compile the emitted code. It is declared
# once, in the crate manifest, and read here rather than repeated. The generator
# reports the same number to every consumer after a write, so it has to be true.
GENERATED_MSRV := $(shell sed -nE '/^\[package\.metadata\.generated-code\]/,/^\[/{s/^[[:space:]]*rust-version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p;}' crates/oapi-codegen/Cargo.toml)

.PHONY: verify-msrv
verify-msrv: ## Compile every generated file on the Rust version a consumer needs
	@test -n "$(GENERATED_MSRV)" || { \
		echo "ERROR: no rust-version under [package.metadata.generated-code] in crates/oapi-codegen/Cargo.toml."; \
		exit 1; \
	}
	rustup toolchain install $(GENERATED_MSRV) --profile minimal
	@echo "Compiling every generated file on Rust $(GENERATED_MSRV)."
	# `msrv-check` is not a workspace member, so it needs its own manifest path.
	# Compiling is the whole assertion: the crate holds no test of its own.
	cargo +$(GENERATED_MSRV) build --manifest-path crates/oapi-codegen/tests/msrv-check/Cargo.toml
	@echo "Generated code compiles on Rust $(GENERATED_MSRV)."

.PHONY: docs
docs: ## Generate and open Rust documentation
	cargo doc --no-deps --open
