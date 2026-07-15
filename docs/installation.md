# Installation

## Requirements

- Rust 1.85+ (2024 edition) to build the generator.
- The *generated* code depends on the target crates you enable: `serde`, and
  `axum` 0.8 (server) and/or `reqwest` 0.12 (client).

## Install

```sh
cargo install oapi-codegen
```

Verify:

```sh
oapi-codegen --version
```

## From source

```sh
git clone https://github.com/alchemaxinc/oapi-codegen-rust
cargo install --path oapi-codegen-rust/crates/oapi-codegen
```

Or run it in-tree without installing:

```sh
cargo run -p oapi-codegen -- --help
```
