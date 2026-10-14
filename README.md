# Shin Engine

[![CI](https://github.com/nikolagigic/shin-engine/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/nikolagigic/shin-engine/actions/workflows/ci.yml)

A small, fast, cross-platform 3D engine that humans, scripts, and AI agents
all drive through the same API.

Shin is open source and written end-to-end in Rust. The crates.io package is
`shin-engine`; the library is `shin` (`use shin::...`).

The goal is an MVP that proves one architecture, not a feature list:

- **wgpu** renderer (Metal / Vulkan / DX12 / WebGPU from one code path)
- **winit** windowing and input
- **glam** math, **hecs** entity-component-system
- glTF meshes, instanced rendering, a fixed-timestep game loop
- Headless golden-image render tests so correctness is checked, not eyeballed
- An engine-native tool registry exposed over **MCP** (`shin-mcp`), so
  agents can build scenes, step the simulation, and look at rendered frames

## Demo

![The demo scene: a field of coloured cubes and spheres around a white pedestal with orbiting satellites](crates/shin-engine/examples/demo.png)

```sh
cargo run --release --example demo
```

A 20×20 field of cubes and spheres, a glTF-loaded pedestal with satellites
orbiting on the fixed-timestep simulation, a directional light, and frame
stats in the window title. Fly with W/A/S/D and E/Q; hold the right mouse
button to look around; Escape quits.

## Status

Pre-alpha. The workspace builds; subsystems land ticket by ticket.

## Build

```sh
cargo build
cargo check-all   # clippy with warnings denied
cargo test-all
```

Requires Rust 1.99 (pinned in `rust-toolchain.toml`).

## Layout

```
crates/shin-engine/   the engine library (package `shin-engine`, lib `shin`)
  src/<subsystem>.rs  one module per subsystem (see src/lib.rs)
  examples/           runnable demos
  tests/              integration + golden-image tests
crates/shin-mcp/      MCP server (stdio) over the engine's tool registry
```

## Using the engine from an agent

`shin-mcp` is an [MCP](https://modelcontextprotocol.io) server over stdio.
It runs one headless engine (default 512×288 offscreen target) and exposes
every tool in the engine's tool registry, named `<subsystem>.<verb>`
(`engine.info`, `engine.list_tools`, ...). New engine tools appear
automatically.

Register it with Claude Code from the repo root, either with the CLI:

```sh
claude mcp add shin -- cargo run -q -p shin-mcp
```

or by copying `.mcp.json.example` to `.mcp.json`. Then ask the agent to call
`engine.info`.

Options: `shin-mcp [--width W] [--height H] [--log-level L]`.
`--log-level` takes an `env_logger` filter and overrides `RUST_LOG`. Logs go
to stderr; stdout carries only JSON-RPC. To try it by hand:

```sh
cargo run -q -p shin-mcp <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"me","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"engine.info","arguments":{}}}
EOF
```

### Tools

These are the tools `shin-mcp` currently serves (`ToolRegistry::with_engine_tools`),
one per line with their arguments (`?` = optional). Call `engine.list_tools`
for the full descriptions and JSON schemas.

```
engine.info           {}
engine.list_tools     {}
scene.load_gltf       { path }
mesh.builtin          { kind: "cube" | "sphere", segments?, rings? }
entity.spawn          { mesh, translation?, rotation_euler_deg?, scale?, color? }
entity.list           { limit? }
entity.set_transform  { entity, translation?, rotation_euler_deg?, scale? }
entity.despawn        { entity }
scene.clear           {}
sim.step              { steps? }
mesh.list             {}
camera.set            { position?, look_at?, fov_y_deg? }
light.set             { direction?, color?, ambient? }
render.frame          { width?, height? }
stats.summary         {}
```

Vectors are JSON arrays (`[x, y, z]`; colours are linear `[r, g, b, a]`),
rotations are Euler angles in degrees applied in X, Y, Z order, and entity
ids are the numbers `entity.spawn` returns. The headless engine starts with
a camera at `(0, 2, 5)` looking at the origin and a white directional
light. A minimal session:

```
mesh.builtin  { "kind": "cube" }                       -> { "handle": 0 }
entity.spawn  { "mesh": 0, "color": [0.8, 0.1, 0.1, 1] } -> { "entity": 4294967296 }
render.frame  { "width": 256, "height": 256 }          -> PNG image
```

## Contributing

See `CLAUDE.md` for architecture, conventions, and the definition of done.
Work is organised as tickets; each branch implements exactly one and is
reviewed and merged by the tech lead.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this project by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions. See [AUTHORS.md](AUTHORS.md).
