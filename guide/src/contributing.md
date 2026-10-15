# Contributing

Shin is developed in the open at
<https://github.com/nikolagigic/shin-engine>, dual-licensed under MIT and
Apache 2.0. Contributions are accepted under the same terms. The
repository's `CLAUDE.md` is the engineering handbook: the conventions,
the module map, and the definition of done, in full. This page is the
short version.

## How work is organised

Work is organised as **tickets**, each with an ID (`GE-NNN`), a T-shirt
size, acceptance criteria, and a test command that proves them. Each
branch implements exactly one ticket, in its own worktree, and is
reviewed and merged by the tech lead. Tickets map to modules, so parallel
branches rarely touch the same file.

Some conventions you will meet in the code:

- Edition 2024, Rust 1.99, pinned in `rust-toolchain.toml`.
- `cargo fmt` with default settings; `cargo clippy` with warnings denied.
- `anyhow::Result` at API boundaries and in examples and tests;
  `thiserror` enums inside a module when callers need to match on them.
- No new dependencies unless a ticket lists them; versions are pinned in
  the workspace `Cargo.toml`.
- No `unsafe` without a `// SAFETY:` comment and a ticket that asks for
  it.
- Every public item has a doc comment. Readability beats cleverness, and
  small public APIs beat flexible ones.
- Every user-facing capability registers a tool and is tested through
  the registry; every rendering change updates or adds an example and is
  proven by a golden image.

## Running the suites

From the repository root:

```sh
cargo fmt --check        # formatting
cargo check-all          # clippy on every target, warnings denied
cargo test-all           # unit, integration, and golden-image tests
cargo doc --no-deps      # rustdoc, must be warning-free
```

`check-all` and `test-all` are Cargo aliases defined in
`.cargo/config.toml`; CI runs exactly these.

Golden-image tests need a GPU or a software driver. When one fails, look
at `crates/shin-engine/tests/golden/_diff/`; if the new output is the
intended one, accept it with:

```sh
UPDATE_GOLDEN=1 cargo test -p shin-engine
```

Do not share a `CARGO_TARGET_DIR` between checkouts or worktrees: Cargo
judges freshness by file timestamps, and a shared target directory can
hand one checkout a stale library.

## The user guide

This guide is an [mdBook](https://rust-lang.github.io/mdBook/) in
`guide/` (source in `guide/src`, output in `guide/book`, which is
ignored). Build and preview it with:

```sh
mdbook build guide
mdbook serve guide
```

The [Tools reference](reference/tools.md) is generated from the tool
registry and checked by a test. Whenever a tool or its arguments change,
regenerate it and commit the result:

```sh
cargo run -p shin-cli -- docs tools > guide/src/reference/tools.md
cargo test -p shin-cli docs
```

CI builds the guide, runs that test, and publishes the book to GitHub
Pages on every push to `main`.

## Definition of done

Every ticket, no exceptions:

- Every acceptance criterion in the ticket is met and demonstrated.
- `cargo test-all`, `cargo check-all`, `cargo fmt --check`, and
  `cargo doc --no-deps` pass.
- New public items have a doc comment.
- A user-facing capability registers a tool and tests it through
  `ToolRegistry::call`.
- A user-facing change updates or adds the relevant page of this guide
  in the same branch, and regenerates the tools reference if any tool
  changed.
- The review request maps each acceptance criterion to its evidence.

Commits are plain descriptions of the change.
