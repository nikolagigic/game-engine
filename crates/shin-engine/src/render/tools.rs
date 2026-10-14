//! Render tools (GE-015): `mesh.list`, `camera.set`, `light.set`,
//! `render.frame`. They work on the [`RenderState`] and [`Camera`]
//! resources that [`Headless::new`] installs.

use anyhow::Context as _;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{LightUniform, RenderState};
use crate::command::Affected;
use crate::headless::Headless;
use crate::math::{Camera, Projection, Vec3};
use crate::tool::{ToolOutput, ToolRegistry, TypedTool};

/// Registers `mesh.list`, `camera.set`, `light.set`, and `render.frame`.
pub fn register_tools(reg: &mut ToolRegistry) {
    reg.register(TypedTool::new(
        "mesh.list",
        "Lists every mesh uploaded to the renderer: its handle (pass it to entity.spawn), \
         name, vertex and index counts, and source (an asset source string such as \
         \"builtin://cube\" or \"assets/x.gltf#1\", or null for meshes added from code).",
        mesh_list,
    ));
    reg.register(
        TypedTool::new(
            "camera.set",
            "Moves and aims the camera. Omitted fields keep their current value; setting only \
         `position` keeps the camera's orientation. Returns the camera.",
            camera_set,
        )
        .with_affected(|_, _| Affected::Camera),
    );
    reg.register(
        TypedTool::new(
            "light.set",
            "Sets the directional light and ambient term (linear RGB). Omitted fields keep \
         their current value. Returns the light.",
            light_set,
        )
        .with_affected(|_, _| Affected::Light),
    );
    reg.register(TypedTool::new(
        "render.frame",
        "Renders one frame of the scene and returns it as a PNG. With `width` and/or \
         `height` the offscreen target is resized first and keeps that size.",
        render_frame,
    ));
}

/// `mesh.list` takes no arguments.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MeshListArgs {}

fn mesh_list(engine: &mut Headless, _: MeshListArgs) -> anyhow::Result<ToolOutput> {
    let state = render_state(engine)?;
    let meshes: Vec<Value> = state
        .registry
        .iter()
        .map(|(handle, mesh)| {
            json!({
                "handle": handle.0,
                "name": mesh.name,
                "vertices": mesh.vertex_count,
                "indices": mesh.index_count,
                "source": state.registry.source(handle).map(|s| s.to_string()),
            })
        })
        .collect();
    Ok(ToolOutput::Json(Value::Array(meshes)))
}

/// Arguments of `camera.set`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CameraSetArgs {
    /// New world-space camera position `[x, y, z]`.
    position: Option<[f32; 3]>,
    /// World-space point `[x, y, z]` to aim at, with +Y as up. Applied
    /// after `position`.
    look_at: Option<[f32; 3]>,
    /// Vertical field of view in degrees, between 1 and 179. Makes the
    /// projection perspective.
    fov_y_deg: Option<f32>,
}

fn camera_set(engine: &mut Headless, args: CameraSetArgs) -> anyhow::Result<ToolOutput> {
    if let Some(fov) = args.fov_y_deg {
        anyhow::ensure!(
            (1.0..=179.0).contains(&fov),
            "fov_y_deg must be between 1 and 179, got {fov}"
        );
    }
    let camera = engine
        .resources
        .get_mut::<Camera>()
        .context("no Camera resource")?;
    if let Some(position) = args.position {
        camera.transform.translation = Vec3::from(position);
    }
    if let Some(target) = args.look_at {
        camera.transform.looking_at(Vec3::from(target), Vec3::Y);
    }
    if let Some(fov) = args.fov_y_deg {
        let (near, far) = match camera.projection {
            Projection::Perspective { near, far, .. }
            | Projection::Orthographic { near, far, .. } => (near, far),
        };
        camera.projection = Projection::Perspective {
            fov_y_radians: fov.to_radians(),
            near,
            far,
        };
    }
    Ok(ToolOutput::Json(camera_json(camera)))
}

/// The camera as `camera.set` reports it.
fn camera_json(camera: &Camera) -> Value {
    let fov_y_deg = match camera.projection {
        Projection::Perspective { fov_y_radians, .. } => Some(fov_y_radians.to_degrees()),
        Projection::Orthographic { .. } => None,
    };
    json!({
        "position": camera.transform.translation.to_array(),
        "forward": camera.transform.forward().to_array(),
        "fov_y_deg": fov_y_deg,
    })
}

/// Arguments of `light.set`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LightSetArgs {
    /// Direction the light travels `[x, y, z]`, from the light toward the
    /// scene; `[0, -1, 0]` shines straight down. Must not be zero.
    direction: Option<[f32; 3]>,
    /// Light colour, linear RGB `[r, g, b]`.
    color: Option<[f32; 3]>,
    /// Ambient light added to every surface, linear RGB `[r, g, b]`.
    ambient: Option<[f32; 3]>,
}

fn light_set(engine: &mut Headless, args: LightSetArgs) -> anyhow::Result<ToolOutput> {
    if let Some(direction) = args.direction {
        anyhow::ensure!(
            Vec3::from(direction).length_squared() > 0.0,
            "light direction must not be zero"
        );
    }
    let light = &mut render_state_mut(engine)?.light;
    if let Some(direction) = args.direction {
        light.direction = Vec3::from(direction).extend(0.0).to_array();
    }
    if let Some(color) = args.color {
        light.color = Vec3::from(color).extend(1.0).to_array();
    }
    if let Some(ambient) = args.ambient {
        light.ambient = Vec3::from(ambient).extend(1.0).to_array();
    }
    Ok(ToolOutput::Json(light_json(light)))
}

/// The light as `light.set` reports it.
fn light_json(light: &LightUniform) -> Value {
    let xyz = |v: [f32; 4]| [v[0], v[1], v[2]];
    json!({
        "direction": xyz(light.direction),
        "color": xyz(light.color),
        "ambient": xyz(light.ambient),
    })
}

/// Arguments of `render.frame`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RenderFrameArgs {
    /// New target width in pixels; omitted keeps the current width.
    width: Option<u32>,
    /// New target height in pixels; omitted keeps the current height.
    height: Option<u32>,
}

fn render_frame(engine: &mut Headless, args: RenderFrameArgs) -> anyhow::Result<ToolOutput> {
    let width = args.width.unwrap_or(engine.target.width);
    let height = args.height.unwrap_or(engine.target.height);
    let max = engine.gpu.device.limits().max_texture_dimension_2d;
    anyhow::ensure!(
        (1..=max).contains(&width) && (1..=max).contains(&height),
        "frame size must be between 1x1 and {max}x{max}, got {width}x{height}"
    );
    engine.resize(width, height);
    let png = engine.render_png()?;
    Ok(ToolOutput::Image { png, width, height })
}

fn render_state(engine: &Headless) -> anyhow::Result<&RenderState> {
    engine
        .resources
        .get::<RenderState>()
        .context("no RenderState resource")
}

fn render_state_mut(engine: &mut Headless) -> anyhow::Result<&mut RenderState> {
    engine
        .resources
        .get_mut::<RenderState>()
        .context("no RenderState resource")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolError;

    fn setup() -> anyhow::Result<(ToolRegistry, Headless)> {
        Ok((ToolRegistry::with_engine_tools(), Headless::new(16, 16)?))
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

    #[test]
    fn mesh_list_reports_uploaded_meshes() -> anyhow::Result<()> {
        let (reg, mut engine) = setup()?;
        assert_eq!(
            call_json(&reg, &mut engine, "mesh.list", json!({}))?,
            json!([])
        );
        let state = engine.resources.expect_mut::<RenderState>();
        state
            .registry
            .add(&engine.gpu.device, &crate::asset::cube());
        let list = call_json(&reg, &mut engine, "mesh.list", json!({}))?;
        assert_eq!(
            list,
            json!([{ "handle": 0, "name": "cube", "vertices": 24, "indices": 36, "source": null }])
        );
        let source = crate::asset::AssetSource::parse("builtin://cube")?;
        let state = engine.resources.expect_mut::<RenderState>();
        state
            .registry
            .add_with_source(&engine.gpu.device, &crate::asset::cube(), source);
        let list = call_json(&reg, &mut engine, "mesh.list", json!({}))?;
        assert_eq!(list[1]["source"], "builtin://cube");
        Ok(())
    }

    #[test]
    fn camera_set_moves_and_aims() -> anyhow::Result<()> {
        let (reg, mut engine) = setup()?;
        let cam = call_json(
            &reg,
            &mut engine,
            "camera.set",
            json!({ "position": [0, 0, 10], "look_at": [0, 0, 0], "fov_y_deg": 60 }),
        )?;
        assert_eq!(cam["position"], json!([0.0, 0.0, 10.0]));
        let forward: Vec<f64> = serde_json::from_value(cam["forward"].clone())?;
        assert!((forward[2] + 1.0).abs() < 1e-5, "{forward:?}");
        assert!((cam["fov_y_deg"].as_f64().unwrap() - 60.0).abs() < 1e-3);

        // Only position: orientation is kept.
        let cam = call_json(
            &reg,
            &mut engine,
            "camera.set",
            json!({ "position": [1, 0, 10] }),
        )?;
        let forward: Vec<f64> = serde_json::from_value(cam["forward"].clone())?;
        assert!((forward[2] + 1.0).abs() < 1e-5, "{forward:?}");

        let err = reg
            .call(&mut engine, "camera.set", json!({ "fov_y_deg": 0 }))
            .unwrap_err();
        assert!(matches!(err, ToolError::Failed(_)), "{err:?}");
        Ok(())
    }

    #[test]
    fn light_set_updates_render_state() -> anyhow::Result<()> {
        let (reg, mut engine) = setup()?;
        let light = call_json(
            &reg,
            &mut engine,
            "light.set",
            json!({ "direction": [0, -1, 0], "ambient": [0.5, 0.5, 0.5] }),
        )?;
        assert_eq!(light["direction"], json!([0.0, -1.0, 0.0]));
        assert_eq!(light["ambient"], json!([0.5, 0.5, 0.5]));
        let state = engine.resources.expect::<RenderState>();
        assert_eq!(state.light.direction, [0.0, -1.0, 0.0, 0.0]);
        assert_eq!(state.light.ambient, [0.5, 0.5, 0.5, 1.0]);

        let err = reg
            .call(&mut engine, "light.set", json!({ "direction": [0, 0, 0] }))
            .unwrap_err();
        assert!(matches!(err, ToolError::Failed(_)), "{err:?}");
        Ok(())
    }

    #[test]
    fn render_frame_resizes_and_returns_png() -> anyhow::Result<()> {
        let (reg, mut engine) = setup()?;
        let ToolOutput::Image { png, width, height } = reg.call(
            &mut engine,
            "render.frame",
            json!({ "width": 24, "height": 8 }),
        )?
        else {
            panic!("expected an image");
        };
        assert_eq!((width, height), (24, 8));
        let image = image::load_from_memory(&png)?;
        assert_eq!((image.width(), image.height()), (24, 8));
        assert_eq!((engine.target.width, engine.target.height), (24, 8));

        // No size: keeps the current one.
        let ToolOutput::Image { width, height, .. } =
            reg.call(&mut engine, "render.frame", json!({}))?
        else {
            panic!("expected an image");
        };
        assert_eq!((width, height), (24, 8));

        let err = reg
            .call(&mut engine, "render.frame", json!({ "width": 0 }))
            .unwrap_err();
        assert!(err.to_string().contains("0x8"), "{err}");
        Ok(())
    }
}
