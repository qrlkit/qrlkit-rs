# AGENTS.md

## Project overview

qrlkit is a Rust CLI for defining and opening named resources from TOML, YAML,
and JSON configuration files.

## Development commands

Run the standard checks before completing a change:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

On macOS or Linux, also run the terminal integration tests when the change
affects shell integration, terminal behavior, setup, hooks, or command output:

```sh
python3 scripts/test-terminal.py
```

## Continuous integration

- Prefer standard CLI tools in CI; keep workflow commands short and avoid long custom scripts.
- `.github/workflows/ci.yml` runs formatting, Clippy, tests, the build, and
  terminal integration tests for pushes to `main` and pull requests.
- `.github/workflows/tag-and-release.yml` publishes versioned crates and Linux
  binaries when a version change is merged.
- `.github/workflows/dependabot-changelog-pr.yml` prepares release pull
  requests after Dependabot updates are merged.

## Development conventions

- Use Rust edition 2024 conventions and the existing project patterns.
- Keep changes focused and preserve existing CLI behavior unless the task asks
  for a behavior change.
- Add or update tests for behavior changes.
- Prefer clear, idiomatic Rust and the existing error-handling approach.
- Do not perform unrelated refactors, dependency upgrades, renames, or cleanup.
- Inspect relevant tests and adjacent modules before changing behavior.

## Documentation

- `README.md` is human-written project documentation.
- Do not modify `README.md` unless the user explicitly asks for a README change
  or directly asks for documentation of a specific change there.
- Do not rewrite, reformat, or "improve" README wording as part of unrelated
  code changes.
- Do not modify `CHANGELOG.md` unless the user explicitly asks for a changelog
  change.

## Important files

- `src/main.rs` — CLI entry point
- `src/setup.rs` — interactive setup and shell integration
- `src/import.rs` — configuration importing
- `src/store.rs` — persisted state
- `tests/` — integration tests
- `scripts/test-terminal.py` — terminal integration tests

## Completion checklist

- Keep the change within the requested scope.
- Run the relevant formatting, lint, and test commands.
- Report any checks that could not be run and why.
