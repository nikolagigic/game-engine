# CLAUDE.md — engineering handbook

You are a developer on this project. This file is the complete context you
need. The planning docs (`docs/`, `tickets/`) are deliberately not in the
repository; `.worktreeinclude` copies them into Claude Code worktrees, and
the tech lead also pastes your ticket into your brief when needed.

## What we are building

A small, readable, open-source 3D game engine, end-to-end in Rust. The MVP
proves one architecture: a wgpu renderer, a winit window, a fixed-timestep
loop, a hecs ECS, glTF meshes, instanced drawing, and headless golden-image
tests. Readability beats cleverness. Small public APIs beat flexible ones.

## Roles

- **CEO / Product owner**: sets direction, merges PRs.
- **Tech lead**: writes tickets, reviews PRs, owns architecture decisions.
- **Developers (you)**: implement exactly one ticket per PR, in a worktree.

## Architecture decisions (summary of the ADRs)

1. **Rust end-to-end.** No scripting layer in milestone 1. Rhai is planned
   for milestone 2 as the "gameplay without recompiling" layer.
2. **wgpu for all GPU work.** One code path for Metal/Vulkan/DX12/WebGPU.
   Shaders are WGSL, stored as `.wgsl` files next to the Rust that uses them
   and included with `include_str!`.
3. **hecs for the ECS.** Minimal, no app framework. Systems are plain
   functions `fn(&mut World, &Resources)`; a schedule runs them in order.
4. **glam for math.** `Vec3`, `Mat4`, `Quat` are used directly; we do not
   wrap them. Right-handed, Y-up, column-major, depth 0..1 (wgpu convention).
5. **Headless golden-image tests are the E2E tests.** A scene is rendered to
   an offscreen texture, read back, and compared against a PNG in
   `crates/engine/tests/golden/`. Correctness is checked, not eyeballed.
6. **Single `engine` crate, one module per subsystem.** Tickets map to
   modules so parallel PRs rarely touch the same file. `lib.rs` is module
   declarations only. All dependencies are pinned in the workspace
   `Cargo.toml`; crate manifests use `{ workspace = true }`.
7. **Fixed timestep for simulation, variable for rendering.** Simulation
   ticks at a fixed `dt` (default 1/60 s) with an accumulator; rendering
   runs once per frame with an interpolation alpha available.
8. **Engine-native tool registry; MCP is one transport.** `engine::tool`
   holds named, JSON-schema'd operations on a `Headless` engine. The
   `engine-mcp` crate exposes that registry over stdio with `rmcp`; the
   engine lib never depends on tokio or rmcp. Every ticket that adds a
   user-facing capability registers a tool for it (`<subsystem>.<verb>`)
   in the same PR, with a test through the registry. This is how agents,
   including you, drive the engine and see rendered frames.

## Module map

| Module   | Owns                                                      | Ticket(s)      |
|----------|-----------------------------------------------------------|----------------|
| `app`    | window, event loop, fixed-timestep loop, `App` builder    | GE-001         |
| `gpu`    | `GpuContext` (instance/adapter/device/queue/surface), offscreen target + readback | GE-001, GE-002 |
| `math`   | glam re-exports, `Transform`, `Camera`                     | GE-003         |
| `ecs`    | `World` wrapper, `Schedule`, `Time`, `Resources`          | GE-004         |
| `asset`  | glTF + texture loading into CPU-side `MeshData`/`ImageData` | GE-005       |
| `render` | GPU mesh upload, material, shaders, instanced draw         | GE-006, GE-008 |
| `input`  | `InputState`, free-fly camera controller                   | GE-007         |
| `stats`  | frame-time ring buffer and reporting                       | GE-009         |
| `tool`   | `Tool` trait, `ToolRegistry`, `ToolOutput`                 | GE-013, GE-015 |
| `headless` | window-less engine handle for tools, tests, benchmarks   | GE-013         |
| crate `engine-mcp` | stdio MCP server over the registry               | GE-014         |

Public API surface should stay small. Prefer `pub(crate)` until a ticket
explicitly asks for a public type.

## Conventions

- Edition 2024, Rust 1.91 (pinned in `rust-toolchain.toml`).
- `cargo fmt` default settings. `cargo clippy -D warnings` must be clean.
- Errors: `anyhow::Result` at API boundaries and in examples/tests;
  `thiserror` enums inside a module when callers need to match on them.
- Logging: `log` macros. Examples call `env_logger::init()`.
- No `unsafe` without a `// SAFETY:` comment and a ticket that asks for it.
- No new dependencies unless the ticket lists them. If you believe one is
  needed, stop and say so in the PR rather than adding it.
- GPU data structs are `#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]`.
- Coordinate system: right-handed, +Y up, -Z forward (glTF convention).
- Tests: unit tests in-module under `#[cfg(test)]`; integration and golden
  tests in `crates/engine/tests/`. Every ticket says which test command
  proves its acceptance criteria.
- Examples are the manual smoke test. Each rendering ticket adds or updates
  one example under `crates/engine/examples/`.

## Build cache (important for parallel worktrees)

Each worktree would otherwise rebuild wgpu from scratch. Before building,
export a shared target directory:

```sh
export CARGO_TARGET_DIR="$HOME/.cache/game-engine/target"
```

Cargo locks the directory, so concurrent builds serialise safely but share
artifacts.

## Workflow

1. You receive a ticket (ID `GE-NNN`, a T-shirt size, acceptance criteria,
   and a test command). Read it fully. Read the modules it names.
2. Work in your own worktree. The branch name is whatever the worktree
   created (`worktree-GE-NNN`) or `GE-NNN-short-slug`; the PR title, not
   the branch, carries the ticket ID.
3. Implement only what the ticket's **In scope** lists. If something in
   **Out of scope** turns out to be required, stop and report it; do not
   expand the ticket.
4. Run the ticket's test command and the full definition of done.
5. Commit with a message starting `GE-NNN: ` and open a PR against `main`
   whose description lists each acceptance criterion with a checkbox and how
   you verified it.

## Definition of done (every ticket, no exceptions)

- [ ] Every acceptance criterion in the ticket is met and demonstrated.
- [ ] `cargo test-all` passes.
- [ ] `cargo check-all` (clippy, warnings denied) passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo doc --no-deps` builds without warnings.
- [ ] New public items have a doc comment.
- [ ] If the ticket adds a user-facing capability and GE-013 is merged: a
      tool is registered for it and tested through `ToolRegistry::call`.
- [ ] PR opened against `main`, description maps AC to evidence.

## T-shirt sizing (for reference)

| Size | Scope                               | Design decisions                                | Rule                                  |
|------|-------------------------------------|-------------------------------------------------|---------------------------------------|
| XS   | One file, one function              | None                                            | CI is the review                      |
| S    | One module                          | None; the ticket specifies the design           | Standard                              |
| M    | Several modules in one crate        | Minor; record them in the PR description        | Standard                              |
| L    | New subsystem or public API         | Yes; the ticket includes a design section       | Split if any part can land alone      |
| XL   | Epic                                | Many                                            | Never assigned; always decomposed     |
