//! Tests for [`super`]: every edit goes through [`ToolRegistry`] and
//! [`History`].

use serde_json::{Value, json};

use super::*;
use crate::math::Vec3;

fn setup() -> anyhow::Result<(ToolRegistry, Headless, u32)> {
    let reg = ToolRegistry::with_engine_tools();
    let mut engine = Headless::new(16, 16)?;
    let state = engine.resources.expect_mut::<RenderState>();
    let cube = state
        .registry
        .add(&engine.gpu.device, &crate::asset::cube());
    Ok((reg, engine, cube.0))
}

fn json_of(output: ToolOutput) -> Value {
    match output {
        ToolOutput::Json(value) => value,
        other => panic!("expected JSON, got {other:?}"),
    }
}

/// Spawns a named cube at `x` through `history` and returns its entity.
fn spawn(
    history: &mut History,
    reg: &ToolRegistry,
    engine: &mut Headless,
    cube: u32,
    x: f32,
) -> anyhow::Result<Entity> {
    let out = history.apply(
        reg,
        engine,
        "entity.spawn",
        json!({ "mesh": cube, "translation": [x, 0, 0], "name": format!("cube {x}") }),
    )?;
    let id = json_of(out)["entity"].as_u64().expect("entity id");
    Ok(Entity::from_bits(id).expect("valid id"))
}

fn translation(engine: &Headless, entity: Entity) -> Vec3 {
    engine
        .world
        .get::<&Transform>(entity)
        .expect("entity has a transform")
        .translation
}

#[test]
fn spawn_undo_removes_redo_restores_same_id() -> anyhow::Result<()> {
    let (reg, mut engine, cube) = setup()?;
    let mut history = History::default();
    let entity = spawn(&mut history, &reg, &mut engine, cube, 2.0)?;
    assert_eq!(engine.world.len(), 1);

    assert!(history.undo(&mut engine)?);
    assert_eq!(engine.world.len(), 0);
    assert!(!engine.world.contains(entity));

    assert!(history.redo(&mut engine)?);
    assert!(engine.world.contains(entity), "same id after redo");
    assert_eq!(engine.world.len(), 1);
    assert_eq!(translation(&engine, entity), Vec3::new(2.0, 0.0, 0.0));
    assert_eq!(
        engine.world.get::<&Name>(entity)?.0,
        "cube 2",
        "name is restored"
    );
    assert_eq!(engine.world.get::<&MeshInstance>(entity)?.mesh.0, cube);

    // The id stays usable by tools after redo.
    reg.call(
        &mut engine,
        "entity.set_transform",
        json!({ "entity": entity.to_bits().get(), "translation": [5, 0, 0] }),
    )?;
    assert_eq!(translation(&engine, entity), Vec3::new(5.0, 0.0, 0.0));
    Ok(())
}

#[test]
fn set_transform_undo_restores_old_transform() -> anyhow::Result<()> {
    let (reg, mut engine, cube) = setup()?;
    let mut history = History::default();
    let entity = spawn(&mut history, &reg, &mut engine, cube, 1.0)?;
    history.apply(
        &reg,
        &mut engine,
        "entity.set_transform",
        json!({ "entity": entity.to_bits().get(), "translation": [9, 8, 7], "scale": [2, 2, 2] }),
    )?;
    assert_eq!(translation(&engine, entity), Vec3::new(9.0, 8.0, 7.0));

    assert!(history.undo(&mut engine)?);
    let t = *engine.world.get::<&Transform>(entity)?;
    assert_eq!(t.translation, Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(t.scale, Vec3::ONE);
    assert_eq!(engine.world.len(), 1, "undoing an edit keeps the entity");

    assert!(history.redo(&mut engine)?);
    assert_eq!(translation(&engine, entity), Vec3::new(9.0, 8.0, 7.0));
    Ok(())
}

#[test]
fn despawn_undo_respawns_with_same_id() -> anyhow::Result<()> {
    let (reg, mut engine, cube) = setup()?;
    let mut history = History::default();
    let entity = spawn(&mut history, &reg, &mut engine, cube, 3.0)?;
    history.apply(
        &reg,
        &mut engine,
        "entity.despawn",
        json!({ "entity": entity.to_bits().get() }),
    )?;
    assert!(!engine.world.contains(entity));

    assert!(history.undo(&mut engine)?);
    assert!(engine.world.contains(entity));
    assert_eq!(translation(&engine, entity), Vec3::new(3.0, 0.0, 0.0));

    assert!(history.redo(&mut engine)?);
    assert!(!engine.world.contains(entity));
    Ok(())
}

#[test]
fn scene_clear_undo_restores_all() -> anyhow::Result<()> {
    let (reg, mut engine, cube) = setup()?;
    let mut history = History::default();
    let entities: Vec<Entity> = (0..3)
        .map(|i| spawn(&mut history, &reg, &mut engine, cube, i as f32))
        .collect::<anyhow::Result<_>>()?;
    history.apply(&reg, &mut engine, "scene.clear", json!({}))?;
    assert_eq!(engine.world.len(), 0);

    assert!(history.undo(&mut engine)?);
    assert_eq!(engine.world.len(), 3);
    for (i, &entity) in entities.iter().enumerate() {
        assert!(engine.world.contains(entity), "entity {i} keeps its id");
        assert_eq!(translation(&engine, entity), Vec3::new(i as f32, 0.0, 0.0));
        assert_eq!(engine.world.get::<&Name>(entity)?.0, format!("cube {i}"));
    }

    assert!(history.redo(&mut engine)?);
    assert_eq!(engine.world.len(), 0);
    Ok(())
}

#[test]
fn new_apply_clears_redo() -> anyhow::Result<()> {
    let (reg, mut engine, cube) = setup()?;
    let mut history = History::default();
    spawn(&mut history, &reg, &mut engine, cube, 0.0)?;
    spawn(&mut history, &reg, &mut engine, cube, 1.0)?;
    assert!(history.undo(&mut engine)?);
    assert_eq!(history.redo_len(), 1);

    spawn(&mut history, &reg, &mut engine, cube, 2.0)?;
    assert_eq!(history.redo_len(), 0);
    assert!(!history.redo(&mut engine)?, "nothing to redo");
    assert_eq!(history.undo_len(), 2);
    Ok(())
}

#[test]
fn limit_drops_oldest_entries() -> anyhow::Result<()> {
    let (reg, mut engine, cube) = setup()?;
    let mut history = History::with_limit(2);
    for i in 0..4 {
        spawn(&mut history, &reg, &mut engine, cube, i as f32)?;
    }
    assert_eq!(history.undo_len(), 2);
    let xs: Vec<Value> = history
        .undo_entries()
        .map(|(_, args)| args["translation"][0].clone())
        .collect();
    assert_eq!(xs, [json!(2.0), json!(3.0)], "the newest two are kept");

    assert!(history.undo(&mut engine)?);
    assert!(history.undo(&mut engine)?);
    assert!(!history.undo(&mut engine)?, "older entries were dropped");
    assert_eq!(engine.world.len(), 2);
    Ok(())
}

#[test]
fn queries_and_failures_are_not_recorded() -> anyhow::Result<()> {
    let (reg, mut engine, cube) = setup()?;
    let mut history = History::default();
    spawn(&mut history, &reg, &mut engine, cube, 0.0)?;
    history.undo(&mut engine)?;

    history.apply(&reg, &mut engine, "entity.list", json!({}))?;
    assert!(
        history
            .apply(&reg, &mut engine, "entity.spawn", json!({ "mesh": 99 }))
            .is_err()
    );
    assert!(matches!(
        history.apply(&reg, &mut engine, "no.such_tool", json!({})),
        Err(ToolError::Unknown(_))
    ));
    assert_eq!(history.undo_len(), 0);
    assert_eq!(history.redo_len(), 1, "redo survives non-edits");
    Ok(())
}

#[test]
fn camera_and_light_undo() -> anyhow::Result<()> {
    let (reg, mut engine, _) = setup()?;
    let mut history = History::default();
    let camera_before = engine.resources.expect::<Camera>().transform;
    let light_before = engine.resources.expect::<RenderState>().light.direction;

    history.apply(
        &reg,
        &mut engine,
        "camera.set",
        json!({ "position": [10, 10, 10] }),
    )?;
    history.apply(
        &reg,
        &mut engine,
        "light.set",
        json!({ "direction": [0, -1, 0] }),
    )?;
    assert_ne!(engine.resources.expect::<Camera>().transform, camera_before);

    history.undo(&mut engine)?;
    assert_eq!(
        engine.resources.expect::<RenderState>().light.direction,
        light_before
    );
    assert_ne!(engine.resources.expect::<Camera>().transform, camera_before);
    history.undo(&mut engine)?;
    assert_eq!(engine.resources.expect::<Camera>().transform, camera_before);
    Ok(())
}

#[test]
fn history_tools_are_listed() {
    let names: Vec<String> = ToolRegistry::with_engine_tools()
        .list()
        .into_iter()
        .map(|t| t.name)
        .collect();
    for tool in ["history.undo", "history.redo"] {
        assert!(names.iter().any(|n| n == tool), "{tool} is listed");
    }
}

#[test]
fn history_tools_undo_and_redo_through_the_registry() -> anyhow::Result<()> {
    let (reg, mut engine, cube) = setup()?;
    for x in [0, 1] {
        apply(
            &reg,
            &mut engine,
            "entity.spawn",
            json!({ "mesh": cube, "translation": [x, 0, 0] }),
        )?;
    }
    assert_eq!(engine.world.len(), 2);

    let out = json_of(apply(&reg, &mut engine, "history.undo", json!({}))?);
    assert_eq!(out, json!({ "ok": true, "remaining": 1 }));
    assert_eq!(engine.world.len(), 1);

    // Plain registry calls reach the same history.
    let out = json_of(reg.call(&mut engine, "history.undo", json!({}))?);
    assert_eq!(out, json!({ "ok": true, "remaining": 0 }));
    let out = json_of(reg.call(&mut engine, "history.undo", json!({}))?);
    assert_eq!(out, json!({ "ok": false, "remaining": 0 }));
    assert_eq!(engine.world.len(), 0);

    let out = json_of(reg.call(&mut engine, "history.redo", json!({}))?);
    assert_eq!(out, json!({ "ok": true, "remaining": 1 }));
    let out = json_of(apply(&reg, &mut engine, "history.redo", json!({}))?);
    assert_eq!(out, json!({ "ok": true, "remaining": 0 }));
    let out = json_of(reg.call(&mut engine, "history.redo", json!({}))?);
    assert_eq!(out, json!({ "ok": false, "remaining": 0 }));
    assert_eq!(engine.world.len(), 2);

    let history = engine.resources.expect::<History>();
    assert_eq!(history.undo_len(), 2);
    assert!(
        history
            .undo_entries()
            .all(|(tool, _)| tool == "entity.spawn")
    );
    Ok(())
}
