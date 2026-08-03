# Rust versions

Three different Rust versions apply to this project. Each one answers a different
question. Only one of them reaches a consumer of the generated code.

| Question                                    | Version  | Where the project declares it                             |
| ------------------------------------------- | -------- | --------------------------------------------------------- |
| What builds the generator from source?      | **1.97** | `rust-version` in the workspace `Cargo.toml`              |
| What runs the released generator binary?    | **none** | A binary needs no toolchain                               |
| What compiles the code the generator emits? | **1.88** | `[package.metadata.generated-code]` in the crate manifest |

A consumer needs the third row. The other two rows belong to a contributor and to
CI.

The first row has a second number next to it. `rust-toolchain.toml` pins the exact
channel that a contributor gets, and that channel is `1.97.1`. The `rust-version`
key states the floor. The pin states the one version that CI uses. A
`cargo install` from source reads the floor, so both numbers matter.

## The generated-code floor

The generator emits Rust source into your crate. Your toolchain compiles that
source, so your toolchain must be new enough for it. The floor is **Rust 1.88**.

The generator reports this number after each write, next to the list of crates that
the output references. Both facts come from the same place, because the crates set
the floor.

### The dependencies set the floor, and the syntax does not

The newest language construct that the emitters produce is `-> impl Future` in a
trait method. Rust made that construct stable in **1.75**. The emitted trait uses
this form and not `async fn` in a trait. This keeps the floor low, and it prevents
a `Send` bound that a caller cannot state.

Each other construct in the output is older than that one. The output has no
`let`-else, no `let` chain, no `gen` block, and no C string literal.

The syntax floor is therefore 1.75, and the true floor is 1.88. The crates that the
output depends on cause the difference. An axum server pulls `axum-extra`, which
pulls `cookie`, which pulls `time`. Version 0.3.54 of `time` declares 1.88. That one
transitive crate sets the number for the whole project.

The floor therefore moves when a dependency raises its own floor. It does not move
when the emitters change. Watch a dependency bump.

### The floor for each output kind

A models-only file needs a lower floor than a server. Each measured floor below is
the highest `rust-version` that any crate in the resolved graph declares.

| Output           | Crates that set it                         | Floor    |
| ---------------- | ------------------------------------------ | -------- |
| Models only      | `serde`, `serde_json`                      | **1.71** |
| `reqwest` client | `reqwest` and its TLS and ICU dependencies | **1.86** |
| `axum` server    | `axum-extra` → `cookie` → `time`           | **1.88** |

The project declares one number, and that number is the highest of the three. One
floor is more simple to state and to test than a table that a consumer must first
match against their own configuration. A consumer who generates models only can use
an older toolchain than the declared floor, and nothing prevents this.

### How the project measured the number

The dependency manifests alone do not give the floor. The project compiled each
output kind in a scratch crate on the candidate toolchain. Such a compile shows two
kinds of error that a manifest hides. A crate can declare a floor that it does not
need. A crate can also need a floor that it does not declare.

CI does the same compile for each pull request. Read the `generated-code-msrv` job
in `.github/workflows/ci.yml`. The job installs the declared toolchain and compiles
each golden file with it. A dependency bump that raises the true floor therefore
fails the build, and it does not reach a consumer.

### The crate that does the compile

The crate is `crates/oapi-codegen/tests/msrv-check`. It holds no test of its own,
because compiling is the assertion.

Two properties of it are deliberate. It is not a workspace member, and the
`[workspace]` table in its manifest is what detaches it. A member gets built by
every `cargo` command in the repository, on the pinned toolchain, which is the
opposite of the measurement. It also sits under `tests/`, next to the golden files
it compiles. Cargo skips a detached crate at any depth, does not treat the
directory as a test target, and leaves it out of `cargo package`.

Its manifest is generated, and the top line of the file says so. The dependency
list comes from `required_dependencies` in `crates/oapi-codegen/src/deps.rs`, which
is the same function that tells a consumer which crates to add. The crate therefore
resolves the graph a consumer resolves. Regenerate the manifest with
`make update-msrv-manifest`. The `msrv_manifest` test fails when the committed file
is stale, in the way that the `cli_docs` test guards `docs/cli.md`.

A hand-written list held three kinds of drift. A wrong version still compiles, so
it measures a floor for a dependency set that no test exercises. An absent crate or
feature usually stops the compile, but not when the report recommends a feature
that no golden file uses. Generation closes all three.

### The dependency-update bot

`update-deps-cargo.yml` bumps dependencies each week and merges when the tests
pass. It finds each `Cargo.toml` in the repository, so it reaches the `msrv-check`
manifest as well.

The bot only rewrites a version for a crate that the file already names, and it
never touches a feature. It therefore cannot keep a hand-written copy correct. The
generated manifest does not depend on the bot for this: the `msrv_manifest` test
compares the file against the report on each run.

When a dependency raises the true floor, the `generated-code-msrv` job fails and
blocks that merge. This is the correct result. A person must then decide to raise
the declared floor, because a higher floor is a breaking change for a consumer. No
bot makes that decision.

## How to raise the floor

A higher generated-code floor is a breaking change for a consumer, because this
project does not control a consumer's toolchain.

To raise the floor, change `rust-version` under
`[package.metadata.generated-code]` in `crates/oapi-codegen/Cargo.toml`. That one
edit updates the CI job, the terminal report, and this document, because all three
read that key.
