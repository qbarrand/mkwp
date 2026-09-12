# Repository Guidelines

## Project Structure & Module Organization

This is a Rust command-line application (`mkwp`) for reading JSON input that
describes time- or solar-based image variants. The current implementation and
its unit tests live together in `src/main.rs`. Keep reusable parsing logic
close to the data types it supports; move substantial independent functionality
into focused modules under `src/` as the program grows.

`sample/` contains checked-in JSON fixtures (`time.json` and `solar.json`) used
by tests and useful for manual CLI checks. `Cargo.toml` defines dependencies;
`Cargo.lock` should be committed when dependency versions change. Build output
belongs in `target/` and is intentionally ignored.

## Build, Test, and Development Commands

- `cargo run -- sample/time.json` — run the CLI against a sample input.
- `cargo run -- sample/solar.json --output output.heif --log-level debug` — run
  with explicit output and diagnostic logging options.
- `cargo check` — type-check quickly during development.
- `cargo test` — run all unit tests, including JSON parsing fixture tests.
- `cargo fmt --check` — verify Rust formatting; use `cargo fmt` to apply it.
- `cargo clippy --all-targets --all-features -- -D warnings` — catch common
  correctness and style issues before review.

## Coding Style & Naming Conventions

Use standard Rust formatting (four-space indentation) and run `cargo fmt`
before committing. Follow Rust naming: `snake_case` for functions, modules, and
fields; `PascalCase` for types and enum variants; `SCREAMING_SNAKE_CASE` for
constants. Prefer `Result` and `?` for fallible operations, and derive traits
only when needed. Preserve external JSON field names with explicit Serde
attributes, as in `fileName` mapped to `file_name`.

## Testing Guidelines

Add focused `#[test]` functions near the code they exercise. Name tests by
observable behavior, such as `parses_time_json` or `rejects_invalid_log_level`.
Use `include_str!("../sample/...json")` for representative fixture inputs;
add a fixture when it makes a format case easier to understand. Cover successful
parsing and meaningful malformed or edge cases. Run `cargo test` and formatting
checks before opening a pull request.

## Commit & Pull Request Guidelines

The repository has no existing commits yet, so no established message convention
can be inferred. Use short imperative subjects, e.g. `Add solar input validation`.
Keep each commit focused. Pull requests should explain the behavior change,
identify affected input/output formats, link relevant issues when available,
and include example CLI output or screenshots only when they clarify a user-
visible change. State the commands used to validate the change.
