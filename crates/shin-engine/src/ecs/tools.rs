//! ECS tools (GE-015): `entity.spawn`, `entity.list`,
//! `entity.set_transform`, `entity.despawn`, `scene.clear`, `sim.step`.
//!
//! Entities are identified by [`Entity::to_bits`]. Rotations are Euler
//! angles in degrees `[x, y, z]`, applied in intrinsic X, then Y, then Z
//! order ([`EulerRot::XYZ`]).

use anyhow::Context as _;
use glam::EulerRot;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Entity, Time, World};
use crate::headless::Headless;
use crate::math::{Quat, Transform, Vec3};
use crate::render::{MeshHandle, MeshInstance, RenderState, TextureHandle};
use crate::tool::{ToolOutput, ToolRegistry, TypedTool};

/// Registers the entity tools, `scene.clear`, and `sim.step`.
pub fn register_tools(reg: &mut ToolRegistry) {
    reg.register(TypedTool::new(
        "entity.spawn",
        "Spawns an entity that draws a mesh (a handle from mesh.list, mesh.builtin, or \
         scene.load_gltf) with a transform, a linear RGBA colour, and optionally a \
         base-colour texture (a handle from asset.load). Returns its id.",
        entity_spawn,
    ));
    reg.register(TypedTool::new(
        "entity.list",
        "Lists entities that have a transform, in id order: id, translation, rotation \
         (Euler degrees, XYZ), scale, and mesh, colour, and texture (a handle or null) if \
         the entity draws one.",
        entity_list,
    ));
    reg.register(TypedTool::new(
        "entity.set_transform",
        "Changes an entity's translation, rotation (Euler degrees, XYZ), and/or scale. \
         Omitted fields keep their value. Returns the new transform.",
        entity_set_transform,
    ));
    reg.register(TypedTool::new(
        "entity.despawn",
        "Removes an entity. Fails if no entity has that id.",
        entity_despawn,
    ));
    reg.register(TypedTool::new(
        "scene.clear",
        "Despawns every entity. Uploaded meshes, the camera, and the light are kept. \
         Returns how many entities were removed.",
        scene_clear,
    ));
    reg.register(TypedTool::new(
        "sim.step",
        "Runs fixed simulation steps (each `fixed_dt` seconds) and returns the Time \
         resource afterwards.",
        sim_step,
    ));
}

/// Arguments of `entity.spawn`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SpawnArgs {
    /// Mesh handle to draw, from mesh.list, mesh.builtin, or scene.load_gltf.
    mesh: u32,
    /// World-space position `[x, y, z]`; default the origin.
    translation: Option<[f32; 3]>,
    /// Rotation as Euler angles in degrees `[x, y, z]`, intrinsic XYZ
    /// order; default no rotation.
    rotation_euler_deg: Option<[f32; 3]>,
    /// Per-axis scale `[x, y, z]`; default `[1, 1, 1]`.
    scale: Option<[f32; 3]>,
    /// Linear RGBA colour `[r, g, b, a]`; default white.
    color: Option<[f32; 4]>,
    /// Base-colour texture handle from asset.load (an image source),
    /// multiplied into `color`; default none (untextured).
    texture: Option<u32>,
}

fn entity_spawn(engine: &mut Headless, args: SpawnArgs) -> anyhow::Result<ToolOutput> {
    let mesh = MeshHandle(args.mesh);
    let texture = args.texture.map(TextureHandle);
    let state = engine
        .resources
        .get::<RenderState>()
        .context("no RenderState resource")?;
    anyhow::ensure!(
        state.registry.contains(mesh),
        "unknown mesh handle {}; {} meshes are uploaded (see mesh.list)",
        args.mesh,
        state.registry.len()
    );
    if let Some(handle) = texture {
        anyhow::ensure!(
            state.textures.contains(handle),
            "unknown texture handle {}; {} textures are uploaded (load one with asset.load)",
            handle.0,
            state.textures.len()
        );
    }
    let mut transform = Transform::IDENTITY;
    apply_transform(
        &mut transform,
        args.translation,
        args.rotation_euler_deg,
        args.scale,
    );
    let color = args.color.unwrap_or([1.0; 4]);
    let entity = engine.world.spawn((
        transform,
        MeshInstance {
            mesh,
            color,
            texture,
        },
    ));
    Ok(ToolOutput::Json(json!({ "entity": entity_id(entity) })))
}

/// Arguments of `entity.list`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    /// Maximum number of entities to return; default 100.
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    100
}

fn entity_list(engine: &mut Headless, args: ListArgs) -> anyhow::Result<ToolOutput> {
    let mut rows: Vec<(u64, Value)> = engine
        .world
        .query::<(Entity, &Transform, Option<&MeshInstance>)>()
        .iter()
        .map(|(entity, transform, instance)| {
            let mut row = transform_json(transform);
            row["entity"] = json!(entity_id(entity));
            if let Some(instance) = instance {
                row["mesh"] = json!(instance.mesh.0);
                row["color"] = json!(instance.color);
                row["texture"] = json!(instance.texture.map(|t| t.0));
            }
            (entity_id(entity), row)
        })
        .collect();
    rows.sort_by_key(|(id, _)| *id);
    let rows: Vec<Value> = rows
        .into_iter()
        .take(args.limit)
        .map(|(_, row)| row)
        .collect();
    Ok(ToolOutput::Json(Value::Array(rows)))
}

/// Arguments of `entity.set_transform`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SetTransformArgs {
    /// Entity id, as returned by entity.spawn or entity.list.
    entity: u64,
    /// New world-space position `[x, y, z]`.
    translation: Option<[f32; 3]>,
    /// New rotation as Euler angles in degrees `[x, y, z]`, intrinsic XYZ
    /// order.
    rotation_euler_deg: Option<[f32; 3]>,
    /// New per-axis scale `[x, y, z]`.
    scale: Option<[f32; 3]>,
}

fn entity_set_transform(
    engine: &mut Headless,
    args: SetTransformArgs,
) -> anyhow::Result<ToolOutput> {
    let entity = find_entity(&engine.world, args.entity)?;
    if !engine.world.satisfies::<&Transform>(entity) {
        engine.world.insert_one(entity, Transform::IDENTITY)?;
    }
    let transform = engine.world.query_one_mut::<&mut Transform>(entity)?;
    apply_transform(
        transform,
        args.translation,
        args.rotation_euler_deg,
        args.scale,
    );
    Ok(ToolOutput::Json(transform_json(transform)))
}

/// Arguments of `entity.despawn`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DespawnArgs {
    /// Entity id, as returned by entity.spawn or entity.list.
    entity: u64,
}

fn entity_despawn(engine: &mut Headless, args: DespawnArgs) -> anyhow::Result<ToolOutput> {
    let entity = find_entity(&engine.world, args.entity)?;
    engine.world.despawn(entity)?;
    Ok(ToolOutput::Json(json!({ "ok": true })))
}

/// `scene.clear` takes no arguments.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ClearArgs {}

fn scene_clear(engine: &mut Headless, _: ClearArgs) -> anyhow::Result<ToolOutput> {
    let removed = engine.world.len();
    engine.world.clear();
    Ok(ToolOutput::Json(json!({ "removed": removed })))
}

/// Arguments of `sim.step`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StepArgs {
    /// Number of fixed steps to run; default 1.
    #[serde(default = "default_steps")]
    steps: u32,
}

fn default_steps() -> u32 {
    1
}

fn sim_step(engine: &mut Headless, args: StepArgs) -> anyhow::Result<ToolOutput> {
    engine.step(args.steps);
    let time = engine.resources.get::<Time>().copied().unwrap_or_default();
    Ok(ToolOutput::Json(json!({
        "fixed_dt": time.fixed_dt,
        "elapsed": time.elapsed,
        "fixed_steps": time.fixed_steps,
        "frame_dt": time.frame_dt,
        "alpha": time.alpha,
    })))
}

/// The id tools use for `entity`.
fn entity_id(entity: Entity) -> u64 {
    entity.to_bits().get()
}

/// The live entity with tool id `id`, or an error naming the id.
fn find_entity(world: &World, id: u64) -> anyhow::Result<Entity> {
    Entity::from_bits(id)
        .filter(|&entity| world.contains(entity))
        .with_context(|| format!("no entity with id {id}"))
}

/// Overwrites the parts of `t` that are given.
fn apply_transform(
    t: &mut Transform,
    translation: Option<[f32; 3]>,
    rotation_euler_deg: Option<[f32; 3]>,
    scale: Option<[f32; 3]>,
) {
    if let Some(translation) = translation {
        t.translation = Vec3::from(translation);
    }
    if let Some([x, y, z]) = rotation_euler_deg {
        t.rotation = Quat::from_euler(
            EulerRot::XYZ,
            x.to_radians(),
            y.to_radians(),
            z.to_radians(),
        );
    }
    if let Some(scale) = scale {
        t.scale = Vec3::from(scale);
    }
}

/// `{translation, rotation_euler_deg, scale}` for `t`.
fn transform_json(t: &Transform) -> Value {
    let (x, y, z) = t.rotation.to_euler(EulerRot::XYZ);
    json!({
        "translation": t.translation.to_array(),
        "rotation_euler_deg": [x.to_degrees(), y.to_degrees(), z.to_degrees()],
        "scale": t.scale.to_array(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolError;

    fn setup() -> anyhow::Result<(ToolRegistry, Headless, u32)> {
        let reg = ToolRegistry::with_engine_tools();
        let mut engine = Headless::new(16, 16)?;
        let state = engine.resources.expect_mut::<RenderState>();
        let cube = state
            .registry
            .add(&engine.gpu.device, &crate::asset::cube());
        Ok((reg, engine, cube.0))
    }

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

    fn assert_close(value: &Value, expected: [f64; 3]) {
        let got: Vec<f64> = serde_json::from_value(value.clone()).unwrap();
        for (g, e) in got.iter().zip(expected) {
            assert!((g - e).abs() < 1e-3, "{got:?} != {expected:?}");
        }
    }

    #[test]
    fn spawn_then_list_round_trips() -> anyhow::Result<()> {
        let (reg, mut engine, cube) = setup()?;
        let spawned = call_json(
            &reg,
            &mut engine,
            "entity.spawn",
            json!({
                "mesh": cube,
                "translation": [1, 2, 3],
                "rotation_euler_deg": [0, 90, 0],
                "scale": [2, 2, 2],
                "color": [1, 0, 0, 1],
            }),
        )?;
        let id = spawned["entity"].as_u64().expect("entity id");

        let list = call_json(&reg, &mut engine, "entity.list", json!({}))?;
        assert_eq!(list.as_array().unwrap().len(), 1);
        let row = &list[0];
        assert_eq!(row["entity"], id);
        assert_eq!(row["mesh"], cube);
        assert_eq!(row["color"], json!([1.0, 0.0, 0.0, 1.0]));
        assert_close(&row["translation"], [1.0, 2.0, 3.0]);
        assert_close(&row["rotation_euler_deg"], [0.0, 90.0, 0.0]);
        assert_close(&row["scale"], [2.0, 2.0, 2.0]);
        Ok(())
    }

    #[test]
    fn spawn_rejects_unknown_mesh() -> anyhow::Result<()> {
        let (reg, mut engine, _) = setup()?;
        let err = reg
            .call(&mut engine, "entity.spawn", json!({ "mesh": 7 }))
            .unwrap_err();
        assert!(matches!(err, ToolError::Failed(_)), "{err:?}");
        assert!(err.to_string().contains("unknown mesh handle 7"), "{err}");
        Ok(())
    }

    #[test]
    fn spawn_takes_a_texture_and_rejects_unknown_ones() -> anyhow::Result<()> {
        let (reg, mut engine, cube) = setup()?;
        let err = reg
            .call(
                &mut engine,
                "entity.spawn",
                json!({ "mesh": cube, "texture": 0 }),
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Failed(_)), "{err:?}");
        assert!(
            err.to_string().contains("unknown texture handle 0"),
            "{err}"
        );
        assert_eq!(engine.world.len(), 0);

        let state = engine.resources.expect_mut::<RenderState>();
        let image = crate::asset::ImageData {
            name: "grey".into(),
            width: 1,
            height: 1,
            rgba: vec![128, 128, 128, 255],
        };
        let texture = state
            .textures
            .add(&engine.gpu.device, &engine.gpu.queue, &image);
        call_json(
            &reg,
            &mut engine,
            "entity.spawn",
            json!({ "mesh": cube, "texture": texture.0 }),
        )?;
        call_json(&reg, &mut engine, "entity.spawn", json!({ "mesh": cube }))?;
        let list = call_json(&reg, &mut engine, "entity.list", json!({}))?;
        let textures: Vec<&Value> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|row| &row["texture"])
            .collect();
        assert_eq!(textures, [&json!(texture.0), &Value::Null]);
        Ok(())
    }

    #[test]
    fn list_respects_limit() -> anyhow::Result<()> {
        let (reg, mut engine, cube) = setup()?;
        for _ in 0..5 {
            call_json(&reg, &mut engine, "entity.spawn", json!({ "mesh": cube }))?;
        }
        let all = call_json(&reg, &mut engine, "entity.list", json!({}))?;
        assert_eq!(all.as_array().unwrap().len(), 5);
        let two = call_json(&reg, &mut engine, "entity.list", json!({ "limit": 2 }))?;
        assert_eq!(two.as_array().unwrap().len(), 2);
        assert_eq!(two[0], all[0]);
        Ok(())
    }

    #[test]
    fn set_transform_changes_only_given_fields() -> anyhow::Result<()> {
        let (reg, mut engine, cube) = setup()?;
        let id = call_json(
            &reg,
            &mut engine,
            "entity.spawn",
            json!({ "mesh": cube, "translation": [1, 1, 1] }),
        )?["entity"]
            .clone();
        let t = call_json(
            &reg,
            &mut engine,
            "entity.set_transform",
            json!({ "entity": id, "scale": [3, 3, 3] }),
        )?;
        assert_close(&t["translation"], [1.0, 1.0, 1.0]);
        assert_close(&t["scale"], [3.0, 3.0, 3.0]);
        Ok(())
    }

    #[test]
    fn set_transform_unknown_entity_names_the_id() -> anyhow::Result<()> {
        let (reg, mut engine, _) = setup()?;
        let id = 0x1_0000_002a_u64;
        let err = reg
            .call(
                &mut engine,
                "entity.set_transform",
                json!({ "entity": id, "translation": [0, 0, 0] }),
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Failed(_)), "{err:?}");
        assert!(err.to_string().contains(&id.to_string()), "{err}");
        Ok(())
    }

    #[test]
    fn despawn_removes_and_rejects_unknown() -> anyhow::Result<()> {
        let (reg, mut engine, cube) = setup()?;
        let id = call_json(&reg, &mut engine, "entity.spawn", json!({ "mesh": cube }))?["entity"]
            .clone();
        let out = call_json(&reg, &mut engine, "entity.despawn", json!({ "entity": id }))?;
        assert_eq!(out, json!({ "ok": true }));
        assert_eq!(engine.world.len(), 0);
        let err = reg
            .call(&mut engine, "entity.despawn", json!({ "entity": id }))
            .unwrap_err();
        assert!(err.to_string().contains(&id.to_string()), "{err}");
        Ok(())
    }

    #[test]
    fn scene_clear_counts_removed() -> anyhow::Result<()> {
        let (reg, mut engine, cube) = setup()?;
        for _ in 0..3 {
            call_json(&reg, &mut engine, "entity.spawn", json!({ "mesh": cube }))?;
        }
        let out = call_json(&reg, &mut engine, "scene.clear", json!({}))?;
        assert_eq!(out, json!({ "removed": 3 }));
        assert_eq!(engine.world.len(), 0);
        assert_eq!(
            engine.resources.expect::<RenderState>().registry.len(),
            1,
            "meshes are kept"
        );
        Ok(())
    }

    #[test]
    fn sim_step_advances_time() -> anyhow::Result<()> {
        let (reg, mut engine, _) = setup()?;
        let time = call_json(&reg, &mut engine, "sim.step", json!({}))?;
        assert_eq!(time["fixed_steps"], 1);
        let time = call_json(&reg, &mut engine, "sim.step", json!({ "steps": 9 }))?;
        assert_eq!(time["fixed_steps"], 10);
        assert!((time["elapsed"].as_f64().unwrap() - 10.0 / 60.0).abs() < 1e-9);
        Ok(())
    }
}
