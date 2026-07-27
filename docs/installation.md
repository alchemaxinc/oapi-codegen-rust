# Installation

## Install

```sh
cargo install oapi-codegen
```

Verify:

```console
$ oapi-codegen --version
oapi-codegen [VERSION]

```

## Prebuilt binaries

Each release publishes standalone binaries on the
[GitHub releases page](https://github.com/alchemaxinc/oapi-codegen-rust/releases),
one per target, each with a `.sha256` checksum:

| Platform     | glibc (dynamic)                          | musl (static)                             |
| ------------ | ---------------------------------------- | ----------------------------------------- |
| Linux x86-64 | `oapi-codegen-x86_64-unknown-linux-gnu`  | `oapi-codegen-x86_64-unknown-linux-musl`  |
| Linux arm64  | `oapi-codegen-aarch64-unknown-linux-gnu` | `oapi-codegen-aarch64-unknown-linux-musl` |
| macOS x86-64 | `oapi-codegen-x86_64-apple-darwin`       | —                                         |
| macOS arm64  | `oapi-codegen-aarch64-apple-darwin`      | —                                         |

The `musl` builds are statically linked with no glibc version floor, so they run
on any Linux — including minimal images such as Alpine or distroless. Prefer them
when a `gnu` binary reports a glibc-version error.

Download, verify, and install (example: Linux x86-64, static musl):

```sh
base=https://github.com/alchemaxinc/oapi-codegen-rust/releases/latest/download
curl -LO "$base/oapi-codegen-x86_64-unknown-linux-musl"
curl -LO "$base/oapi-codegen-x86_64-unknown-linux-musl.sha256"
sha256sum -c oapi-codegen-x86_64-unknown-linux-musl.sha256
install -m 0755 oapi-codegen-x86_64-unknown-linux-musl /usr/local/bin/oapi-codegen
```

## From source

```sh
git clone https://github.com/alchemaxinc/oapi-codegen-rust
cd oapi-codegen-rust
cargo install --path crates/oapi-codegen
```

Or run it in-tree without installing:

```sh
cargo run -p oapi-codegen -- --help
```
