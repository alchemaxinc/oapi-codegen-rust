# Installation

## Install the CLI

```sh
cargo install oapi-codegen
```

Verify the installation:

```console
$ oapi-codegen --version
oapi-codegen [VERSION]

```

## Prebuilt binaries

Each release publishes standalone binaries on the
[GitHub releases page](https://github.com/alchemaxinc/oapi-codegen-rust/releases).
Each target has one binary and one `.sha256` checksum file.

| Platform     | glibc (dynamic)                          | musl (static)                             |
| ------------ | ---------------------------------------- | ----------------------------------------- |
| Linux x86-64 | `oapi-codegen-x86_64-unknown-linux-gnu`  | `oapi-codegen-x86_64-unknown-linux-musl`  |
| Linux arm64  | `oapi-codegen-aarch64-unknown-linux-gnu` | `oapi-codegen-aarch64-unknown-linux-musl` |
| macOS x86-64 | `oapi-codegen-x86_64-apple-darwin`       | —                                         |
| macOS arm64  | `oapi-codegen-aarch64-apple-darwin`      | —                                         |

`musl` builds are static. They run on systems without matching glibc versions.
This includes Alpine and distroless images.

Download, confirm, and install a Linux x86-64 static musl binary:

```sh
base=https://github.com/alchemaxinc/oapi-codegen-rust/releases/latest/download
curl -LO "$base/oapi-codegen-x86_64-unknown-linux-musl"
curl -LO "$base/oapi-codegen-x86_64-unknown-linux-musl.sha256"
sha256sum -c oapi-codegen-x86_64-unknown-linux-musl.sha256
install -m 0755 oapi-codegen-x86_64-unknown-linux-musl /usr/local/bin/oapi-codegen
```

## Install from source

```sh
git clone https://github.com/alchemaxinc/oapi-codegen-rust
cd oapi-codegen-rust
cargo install --path crates/oapi-codegen
```

Run the CLI from the repository without installation:

```sh
cargo run -p oapi-codegen -- --help
```
