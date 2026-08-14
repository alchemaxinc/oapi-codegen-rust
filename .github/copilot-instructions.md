# Copilot / Agent Instructions (pinned)

## Planning / “initial plan” commits

- **Do not create empty commits.**
- If you need to create a planning commit for any reason, the commit message **must be exactly**:
  `chore: initial plan`
- Never use the message `initial plan`.

## Commit message style

- Prefer Conventional Commits: `feat:`, `fix:`, `chore:`, `docs:`, `test:`, `refactor:`.

## CLI usage examples

- In every `oapi-codegen` command example (README, docs, `--help` text, the
  Makefile, example folders), always put options first and the positional
  `<SPEC_FILE>` last, matching the generated `--help` usage line
  (for example `oapi-codegen --config-file cfg.yaml --output-file out.rs spec.yaml`).

## CLI definition is the single source of truth

- The clap CLI lives in `crates/oapi-codegen/src/cli.rs`. After changing any
  flag, argument, help text, or the `EXAMPLES` block, run `make update-docs` to
  regenerate `docs/cli.md` (it is generated — never edit it by hand). CI's
  `make verify-generated` fails on drift.

## Documented command output is generated

- The `console` code blocks in `README.md`, `docs/*.md`, and `examples/*/README.md`
  are trycmd cases: their expected output is verified by the `documentation` test.
  The dependency report the CLI prints carries the versions declared in
  `crates/oapi-codegen/Cargo.toml`, so a dependency bump changes that output.
  After any dependency upgrade, run `make update-docs` and commit the result.

# Rust Coding Conventions and Best Practices

Follow idiomatic Rust practices and community standards when writing Rust code.

These instructions are based
on [The Rust Book](https://doc.rust-lang.org/book/), [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/), [RFC 430 naming conventions](https://github.com/rust-lang/rfcs/blob/master/text/0430-finalizing-naming-conventions.md),
and the broader Rust community at [users.rust-lang.org](https://users.rust-lang.org).

## General Instructions

- Always prioritize readability, safety, and maintainability.
- Use strong typing and use Rust's ownership system for memory safety.
- Break down complex functions into smaller, more manageable functions.
- For algorithm-related code, include explanations of the approach used.
- Write code with good maintainability practices, including comments on why certain design decisions were made.
- Handle errors gracefully using `Result<T, E>` and provide meaningful error messages.
- For external dependencies, mention their usage and purpose in documentation.
- Try to use as few external dependencies as possible, unless they provide significant value or are widely adopted in
  the Rust ecosystem.
- Use consistent naming conventions
  following [RFC 430](https://github.com/rust-lang/rfcs/blob/master/text/0430-finalizing-naming-conventions.md).
- Write idiomatic, safe, and efficient Rust code that follows the borrow checker's rules.
- Ensure code compiles without warnings.

## Patterns to Follow

- Use modules (`mod`) and public interfaces (`pub`) to encapsulate logic.
- Handle errors properly using `?`, `match`, or `if let`.
- Use `serde` for serialization and `thiserror` or `anyhow` for custom errors.
- Implement traits to abstract services or external dependencies.
- Structure async code using `async/await` and `tokio` or `async-std`.
- Prefer enums over flags and states for type safety.
- Use builders for complex object creation.
- Split binary and library code (`main.rs` vs `lib.rs`) for testability and reuse.
- Use `rayon` for data parallelism and CPU-bound tasks.
- Use iterators instead of index-based loops as they're often faster and safer.
- Use `&str` instead of `String` for function parameters when you do not need ownership.
- Prefer borrowing and zero-copy operations to avoid unnecessary allocations.
- Always be biased towards blocking code for simplicity unless async is necessary for performance or responsiveness.
- **Return values:** Functions returning 3+ values must use a named struct. 2-tuples are acceptable only when the
  meaning is obvious from context (for example `(key, value)`). When in doubt, use a struct — named fields are always clearer
  than positional ones.

### Ownership, Borrowing, and Lifetimes

- Prefer borrowing (`&T`) over cloning unless ownership transfer is necessary.
- Use `&mut T` when you need to modify borrowed data.
- Explicitly annotate lifetimes when the compiler cannot infer them.
- Use `Rc<T>` for single-threaded reference counting and `Arc<T>` for thread-safe reference counting.
- Use `RefCell<T>` for interior mutability in single-threaded contexts and `Mutex<T>` or `RwLock<T>` for multi-threaded
  contexts.

## Patterns to Avoid

- Do not rely on global mutable state—use dependency injection or thread-safe containers.
- Avoid deeply nested logic—refactor with functions or combinators.
- Do not ignore warnings—treat them as errors during CI.
- Avoid `unsafe` unless required and fully documented.
- Do not overuse `clone()`, use borrowing instead of cloning unless ownership transfer is needed.
- Avoid premature `collect()`, keep iterators lazy until you actually need the collection.
- Avoid unnecessary allocations—prefer borrowing and zero-copy operations.

## Code Style and Formatting

- Follow the Rust Style Guide and use `rustfmt` for automatic formatting.
- Always separate item definitions (functions, structs, enums, impls, modules) with a single blank line. Never place two `fn`
  definitions on adjacent lines without a blank line between them.
- Place function and struct documentation immediately before the item using `///`.
- Use `cargo clippy` to catch common mistakes and enforce best practices.
- Only use comments for functions that are not overtly self-explanatory or for complex logic. Otherwise, prefer clear
  and descriptive code.
- Do not ever use comments to separate sections of code. Instead, use functions, modules, or other organizational
  structures to create clear boundaries. In particular, never write banner or divider comments such as
  `// ---- Schema kinds ----` or `// === Helpers ===`.
- Place global constants and statics (module-level `const` / `static` items) at the top of the file, immediately after
  the imports and before any other item definitions.
- Avoid side-effect / impure functions in favor of pure functions that take inputs and return outputs without modifying
  external state. Unless it will severely impact performance or usability, in which case side effects must be clearly
  documented.
- Avoid "magic numbers" and "magic strings"—use constants or enums instead.

## Error Handling

- Use `Result<T, E>` for recoverable errors and `panic!` only for unrecoverable errors.
- Prefer `?` operator over `unwrap()` or `expect()` for error propagation.
- **Never use `unwrap()` or `expect()` in request-handling code** (routes, client fetches, parsing). These run
  per-request and a panic will return a connection reset instead of a proper error response. Return `Result`, degrade
  gracefully, or log a warning and use a fallback.
- **`expect()` is acceptable only in:**
  - Startup/config validation (fail fast with a clear message before serving traffic).
  - Client/resource construction (for example `reqwest::Client::builder().build().expect(...)`) that runs once at init.
  - Test code (where panicking is the failure mechanism).
- **Never use bare `unwrap()`** anywhere — if a panic is truly justified, use `expect("reason")` so the message
  explains the invariant.
- For values that are compile-time known (for example static timezone strings), prefer compile-time constants over runtime
  parsing with `expect()`.
- Create custom error types using `thiserror` or implement `std::error::Error`.
- Use `Option<T>` for values that can or cannot exist.
- Provide meaningful error messages and context.
- Error types must be meaningful and well-behaved (implement standard traits).
- Validate function arguments and return appropriate errors for invalid input.

## API Design Guidelines

### Common Traits Implementation

Eagerly implement common traits where appropriate:

- `Copy`, `Clone`, `Eq`, `PartialEq`, `Ord`, `PartialOrd`, `Hash`, `Debug`, `Display`, `Default`
- Use standard conversion traits: `From`, `AsRef`, `AsMut`
- Collections must implement `FromIterator` and `Extend`
- Note: `Send` and `Sync` are auto-implemented by the compiler when safe. Avoid manual implementation unless using
  `unsafe` code

### Type Safety and Predictability

- Use newtypes to provide static distinctions
- Arguments must convey meaning through types. Prefer specific types over generic `bool` parameters
- Use `Option<T>` appropriately for truly optional values
- Functions with a clear receiver must be methods
- Only smart pointers must implement `Deref` and `DerefMut`

### Future Proofing

- Use sealed traits to protect against downstream implementations
- Structs must have private fields
- Functions must validate their arguments
- All public types must implement `Debug`

## Testing and Documentation

- Write comprehensive unit tests using `#[cfg(test)]` modules and `#[test]` annotations.
- Try to do table-driven tests for functions with multiple similar input/output cases where ever this makes sense to do.
- Use test modules alongside the code they test (`mod tests { ... }`).
- Write integration tests in `tests/` directory with descriptive filenames.
- Write clear and concise comments for each function, struct, enum, and complex logic.
- Ensure functions have descriptive names and include comprehensive documentation.
- Document all public APIs with rustdoc (`///` comments) following
  the [API Guidelines](https://rust-lang.github.io/api-guidelines/).
- Use `#[doc(hidden)]` to hide implementation details from public documentation.
- Document error conditions, panic scenarios, and safety considerations.
- Examples must use `?` operator, not `unwrap()` or deprecated `try!` macro.

## Project Organization

- Use semantic versioning in `Cargo.toml`.
- Include comprehensive metadata: `description`, `license`, `repository`, `keywords`, `categories`.
- Use feature flags for optional functionality.
- Organize code into modules using `mod.rs` or named files.
- Keep `main.rs` or `lib.rs` minimal - move logic to modules.
- When a single module file grows too large, split it into a directory module with a `mod.rs` re-exporting its sub-modules. This keeps the public API identical while improving internal organization.

## Quality Checklist

Before publishing or reviewing Rust code, make sure that:

### Core Requirements

- [ ] **Naming**: Follows RFC 430 naming conventions
- [ ] **Traits**: Implements `Debug`, `Clone`, `PartialEq` where appropriate
- [ ] **Error Handling**: Uses `Result<T, E>` and provides meaningful error types
- [ ] **Documentation**: All public items have rustdoc comments with examples
- [ ] **Testing**: Comprehensive test coverage including edge cases

### Safety and Quality

- [ ] **Safety**: No unnecessary `unsafe` code, proper error handling
- [ ] **Performance**: Efficient use of iterators, minimal allocations
- [ ] **API Design**: Functions are predictable, flexible, and type-safe
- [ ] **Future Proofing**: Private fields in structs, sealed traits where appropriate
- [ ] **Tooling**: Code passes all workflows executed in the CI/CD pipeline
