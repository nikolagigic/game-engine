# game-engine

[![CI](https://github.com/nikolagigic/game-engine/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/nikolagigic/game-engine/actions/workflows/ci.yml)

A small, readable, open-source 3D game engine written end-to-end in Rust.

The goal is an MVP that proves one architecture, not a feature list:

- **wgpu** renderer (Metal / Vulkan / DX12 / WebGPU from one code path)
- **winit** windowing and input
- **glam** math, **hecs** entity-component-system
- glTF meshes, instanced rendering, a fixed-timestep game loop
- Headless golden-image render tests so correctness is checked, not eyeballed
- An engine-native tool registry exposed over **MCP** (`engine-mcp`), so
  agents can build scenes, step the simulation, and look at rendered frames

## Status

Pre-alpha. The workspace builds; subsystems land ticket by ticket.

## Build

```sh
cargo build
cargo check-all   # clippy with warnings denied
cargo test-all
```

Requires Rust 1.91 (pinned in `rust-toolchain.toml`).

## Layout

```
crates/engine/        the engine library
  src/<subsystem>.rs  one module per subsystem (see src/lib.rs)
  examples/           runnable demos
  tests/              integration + golden-image tests
crates/engine-mcp/    MCP server (stdio) over the engine's tool registry
```

## Contributing

See `CLAUDE.md` for architecture, conventions, and the definition of done.
Work is organised as tickets; each PR implements exactly one.
