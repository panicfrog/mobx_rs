# Repository Guidelines

## Project Structure & Module Organization
The crate currently exposes its API from `src/lib.rs`. Organize new features into nested modules (e.g., `observable`, `reaction`, `core`) and keep shared helpers under `src/internal` or `src/util`. Lean on tight module boundaries and prefer small, composable functions. Inline unit tests already exist; add broader scenarios in `tests/` with filenames mirroring the feature. Update `rust_mobx.md` whenever architecture decisions or the MobX parity plan evolves. use module-named file instead of mod.rs

## Build, Test, and Development Commands
- `cargo check` – fast borrow-checking and type validation during iteration.
- `cargo fmt --all` – format the workspace with the Rust 2024 defaults.
- `cargo clippy --all-targets --all-features -D warnings` – lint and fail build on any warning.
- `cargo test` – run the full suite; add filters like `cargo test reaction::` for focused runs.
- `cargo doc --no-deps --open` – generate local API docs to verify public comments.

## Coding Style & Naming Conventions
Stick to rustfmt output, 4-space indentation, `snake_case` for functions/files, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for constants. Prefer `pub(crate)` until APIs are stable. Add concise rustdoc for every public item and order `mod` declarations alphabetically to keep diffs predictable.

## Testing Guidelines
Use `#[cfg(test)]` modules for fast unit coverage and place integration stories under `tests/`. Name tests with the pattern `fn test_describes_behavior()`. Exercise both happy-path and error-path logic, and add regression tests whenever fixing a bug. Run `cargo test --lib` before pushing and record the command in the PR when sharing results.

## Commit & Pull Request Guidelines
Follow Conventional Commit syntax (e.g., `feat: add autorun reaction`, `fix: resolve stale observer tracking`). Keep commits focused so reverts stay surgical. PR descriptions should summarize scope, link tracking issues, and list validation commands run. Attach screenshots or logs for user-visible changes. Await green CI, then prefer rebase + fast-forward merges.
