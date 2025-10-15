# Repository Guidelines

Treat every contribution as a filesystem interaction and follow these conventions.

## Project Structure & Module Organization
- `src/` contains core Rust modules (`lib.rs`, `runtime/`, `srv/`) backing the 9P endpoints.
- `translators/` (e.g., `sqlite-wasm/`, `oneapi-wasm/`, `opencl-gpu/`) turns file calls into WASM or GPU work units.
- `tests/` hosts integration specs; align fixtures and helpers with the feature under test.
- `examples/` and `docs/` capture namespace patterns; review before adding new virtual files.
- `src-tauri/` plus `tauri-app/` provide the optional GUI; keep their dependencies isolated from the server crate.

## Build, Test, and Development Commands
- `cargo build` for local iterations; prefer `cargo build --release` when benchmarking or packaging.
- `cargo build --features sycl` activates the AdaptiveCpp flow; confirm AdaptiveCpp is installed first.
- `cd translators/sqlite-wasm && ./build.sh` rebuilds the SQLite WASM translator after schema changes.
- `cargo test` exercises the suite; append `--features testing proptest` to run property cases.
- `cargo fmt -- --check` and `cargo clippy -- -D warnings` gate pull requests and must pass before review.

## Coding Style & Naming Conventions
- Rely on `cargo fmt`, 4-space indentation, and a ~100-character line target.
- Order imports as standard library, external crates, then workspace modules; keep glob imports confined to preludes.
- Follow Rust naming norms (`snake_case`, `PascalCase`, `SCREAMING_SNAKE_CASE`) and return `anyhow::Result<T>` from fallible APIs.
- Prefer `tracing` spans/logs and attach `.context()` when propagating errors across module boundaries.

## Testing Guidelines
- Place lightweight unit tests inline under `#[cfg(test)]`; use `tempfile` builders for filesystem fixtures.
- Integration coverage lives in `tests/`; mirror file names to features (`namespace_smoke.rs`, `translator_cache.rs`).
- Record new behaviors through reproducible 9P interactions instead of bespoke shell scripts.

## Commit & Pull Request Guidelines
- Write imperative subject lines (“Add translator cache hooks”), matching the existing history.
- Reference issues with `Refs #123`, summarize the change, and call out migrations or configuration impacts.
- Before submission, run `cargo fmt -- --check && cargo clippy -- -D warnings && cargo test` and share the command block with results.

## Security & Configuration Tips
- Store secrets locally in `config.toml`; never commit credentials or production tokens.
- Review translator scripts in `translators/*/build.sh` before enabling them in shared namespaces.
- Keep functions small and focused
- Use `tracing` for logging instead of `println!`
- Prefer `clap` for command-line argument parsing
- Use `serde` for serialization/deserialization
- Follow RAII principles for resource management
