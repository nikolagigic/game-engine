//! Asset tools (GE-015): `scene.load_gltf` and `mesh.builtin`. Both create
//! CPU-side meshes here and upload them into the renderer's
//! [`MeshRegistry`](crate::render::MeshRegistry).

use anyhow::Context as _;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::headless::Headless;
use crate::render::{MeshInstance, RenderState};
use crate::tool::{ToolOutput, ToolRegistry, TypedTool};

/// Registers `scene.load_gltf` and `mesh.builtin`.
pub fn register_tools(reg: &mut ToolRegistry) {
    reg.register(TypedTool::new(
        "scene.load_gltf",
        "Loads a glTF 2.0 file (.gltf or .glb), uploads its meshes, and spawns one \
         entity per node with a mesh, at the node's world transform and tinted by its \
         material's base colour. Returns the new mesh handles and entity ids.",
        load_gltf,
    ));
    reg.register(TypedTool::new(
        "mesh.builtin",
        "Uploads a built-in mesh: a unit cube (extent -0.5..0.5) or a unit-radius UV \
         sphere. Returns its handle for entity.spawn.",
        mesh_builtin,
    ));
}

/// Arguments of `scene.load_gltf`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LoadGltfArgs {
    /// Path to a .gltf or .glb file, absolute or relative to the engine's
    /// working directory.
    path: String,
}

fn load_gltf(engine: &mut Headless, args: LoadGltfArgs) -> anyhow::Result<ToolOutput> {
    let scene = super::load_gltf(&args.path)?;
    let state = engine
        .resources
        .get_mut::<RenderState>()
        .context("no RenderState resource")?;
    let handles: Vec<_> = scene
        .meshes
        .iter()
        .map(|mesh| state.registry.add(&engine.gpu.device, mesh))
        .collect();

    let mut entities = Vec::new();
    for node in &scene.nodes {
        let Some(index) = node.mesh else { continue };
        let color = scene.meshes[index]
            .material
            .and_then(|m| scene.materials.get(m))
            .map_or([1.0; 4], |m| m.base_color);
        let mesh = handles[index];
        let entity = engine
            .world
            .spawn((node.transform, MeshInstance { mesh, color }));
        entities.push(entity.to_bits().get());
    }
    let meshes: Vec<u32> = handles.iter().map(|h| h.0).collect();
    Ok(ToolOutput::Json(
        json!({ "meshes": meshes, "entities": entities }),
    ))
}

/// Which built-in mesh to create.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum BuiltinKind {
    /// Unit cube centred at the origin.
    Cube,
    /// Unit-radius UV sphere centred at the origin.
    Sphere,
}

/// Arguments of `mesh.builtin`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BuiltinArgs {
    /// Which mesh: "cube" or "sphere".
    kind: BuiltinKind,
    /// Sphere only: divisions around the equator (at least 3); default 32.
    segments: Option<u32>,
    /// Sphere only: divisions from pole to pole (at least 2); default 16.
    rings: Option<u32>,
}

fn mesh_builtin(engine: &mut Headless, args: BuiltinArgs) -> anyhow::Result<ToolOutput> {
    let data = match args.kind {
        BuiltinKind::Cube => super::cube(),
        BuiltinKind::Sphere => {
            let segments = args.segments.unwrap_or(32);
            let rings = args.rings.unwrap_or(16);
            anyhow::ensure!(
                segments <= 1024 && rings <= 1024,
                "segments and rings must be at most 1024, got {segments} and {rings}"
            );
            super::uv_sphere(segments, rings)
        }
    };
    let state = engine
        .resources
        .get_mut::<RenderState>()
        .context("no RenderState resource")?;
    let handle = state.registry.add(&engine.gpu.device, &data);
    Ok(ToolOutput::Json(json!({ "handle": handle.0 })))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::Value;

    use super::*;
    use crate::tool::ToolError;

    fn call_json(
        reg: &ToolRegistry,
        engine: &mut Headless,
        name: &str,
        args: Value,
    ) -> anyhow::Result<Value> {
        match reg.call(engine, name, args)? {
            ToolOutput::Json(value) => Ok(value),
            other => anyhow::bail!("expected JSON from {name}, got {other:?}"),
        }
    }

    #[test]
    fn builtin_cube_and_sphere_get_new_handles() -> anyhow::Result<()> {
        let reg = ToolRegistry::with_engine_tools();
        let mut engine = Headless::new(8, 8)?;
        let cube = call_json(&reg, &mut engine, "mesh.builtin", json!({ "kind": "cube" }))?;
        let sphere = call_json(
            &reg,
            &mut engine,
            "mesh.builtin",
            json!({ "kind": "sphere", "segments": 8, "rings": 4 }),
        )?;
        assert_eq!(cube, json!({ "handle": 0 }));
        assert_eq!(sphere, json!({ "handle": 1 }));
        let registry = &engine.resources.expect::<RenderState>().registry;
        assert_eq!(registry.len(), 2);
        assert_eq!(
            registry.get(crate::render::MeshHandle(1)).vertex_count,
            9 * 5
        );

        let err = reg
            .call(&mut engine, "mesh.builtin", json!({ "kind": "torus" }))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs { .. }), "{err:?}");
        Ok(())
    }

    #[test]
    fn load_gltf_spawns_one_entity_per_mesh_node() -> anyhow::Result<()> {
        let reg = ToolRegistry::with_engine_tools();
        let mut engine = Headless::new(8, 8)?;
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/test/cube.gltf");
        let out = call_json(
            &reg,
            &mut engine,
            "scene.load_gltf",
            json!({ "path": path }),
        )?;
        let meshes = out["meshes"].as_array().expect("meshes");
        let entities = out["entities"].as_array().expect("entities");
        assert!(!meshes.is_empty(), "{out}");
        assert!(!entities.is_empty(), "{out}");
        assert_eq!(engine.world.len() as usize, entities.len());
        Ok(())
    }

    #[test]
    fn load_gltf_missing_path_reports_the_path() -> anyhow::Result<()> {
        let reg = ToolRegistry::with_engine_tools();
        let mut engine = Headless::new(8, 8)?;
        let path = "/definitely/not/here/scene.gltf";
        let err = reg
            .call(&mut engine, "scene.load_gltf", json!({ "path": path }))
            .unwrap_err();
        assert!(matches!(err, ToolError::Failed(_)), "{err:?}");
        // `{:#}` is how the MCP server and agents see the full error chain.
        let ToolError::Failed(inner) = &err else {
            unreachable!()
        };
        assert!(format!("{inner:#}").contains(path), "{inner:#}");
        Ok(())
    }
}
