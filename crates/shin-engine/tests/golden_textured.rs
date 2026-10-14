//! GE-025: base-colour textures, driven through the tool registry.
//!
//! `assets/test/checker.png` is 64x64 with 4x4 cells of 16 px: a cell
//! (col, row), row 0 at the top of the image, is white when col + row is
//! even, otherwise red in rows 0-1 and blue in rows 2-3. Any flip or
//! rotation of the texture moves those colours, so sampling known cells on
//! the screen checks orientation, not just that something was drawn.
//!
//! The cube is turned -30° about +Y so its +Z face (the one the samples are
//! on) faces both the default camera at `(0, 2, 5)` and the default light.

mod common;

use std::path::Path;

use common::{GoldenOpts, assert_golden};
use image::{Rgba, RgbaImage};
use serde_json::{Value, json};
use shin::headless::{Headless, default_camera};
use shin::math::{Quat, Vec3};
use shin::render::{RenderState, TextureHandle};
use shin::tool::{ToolOutput, ToolRegistry};

const SIZE: u32 = 128;

/// Y rotation of the cube, in degrees.
const TURN_DEG: f32 = -30.0;

fn asset(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/test")
        .join(name)
        .to_str()
        .expect("UTF-8 path")
        .to_string()
}

fn call_json(reg: &ToolRegistry, engine: &mut Headless, name: &str, args: Value) -> Value {
    match reg.call(engine, name, args) {
        Ok(ToolOutput::Json(value)) => value,
        other => panic!("`{name}` did not return JSON: {other:?}"),
    }
}

/// Calls `render.frame` at `SIZE` x `SIZE` and decodes the PNG.
fn render_frame(reg: &ToolRegistry, engine: &mut Headless) -> RgbaImage {
    let args = json!({ "width": SIZE, "height": SIZE });
    match reg.call(engine, "render.frame", args) {
        Ok(ToolOutput::Image { png, .. }) => {
            image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                .expect("render.frame returns a valid PNG")
                .to_rgba8()
        }
        other => panic!("render.frame did not return an image: {other:?}"),
    }
}

/// The screen pixel showing the centre of checker cell (`col`, `row`) on the
/// cube's +Z face.
///
/// The built-in cube maps the +Z face's corner `(-x, +y)` to uv `(0, 0)`,
/// the top-left of the image, and `(+x, -y)` to `(1, 1)`.
fn cell_pixel(col: u32, row: u32) -> (u32, u32) {
    let u = (col as f32 + 0.5) / 4.0;
    let v = (row as f32 + 0.5) / 4.0;
    let local = 0.5 * Vec3::new(2.0 * u - 1.0, 1.0 - 2.0 * v, 1.0);
    let world = Quat::from_rotation_y(TURN_DEG.to_radians()) * local;
    let ndc = default_camera().view_projection(1.0).project_point3(world);
    let to_px = |c: f32| ((c * 0.5 + 0.5) * SIZE as f32) as u32;
    (to_px(ndc.x), to_px(-ndc.y))
}

#[derive(Debug, PartialEq)]
enum Hue {
    White,
    Red,
    Blue,
}

/// Classifies a lit checker pixel; panics if it is none of the three.
fn hue(p: Rgba<u8>) -> Hue {
    let [r, g, b, _] = p.0;
    let bright = |c: u8| c > 80;
    let dark = |c: u8| c < 16;
    match (r, g, b) {
        _ if bright(r) && bright(g) && bright(b) && r.abs_diff(b) < 8 && g.abs_diff(b) < 8 => {
            Hue::White
        }
        _ if bright(r) && dark(g) && dark(b) => Hue::Red,
        _ if dark(r) && dark(g) && bright(b) => Hue::Blue,
        _ => panic!("pixel {p:?} is not white, red, or blue"),
    }
}

/// Checks the colour of three cells that together pin the orientation.
fn assert_checker_orientation(image: &RgbaImage) {
    for ((col, row), expected) in [
        ((0, 0), Hue::White),
        ((1, 0), Hue::Red),
        ((0, 3), Hue::Blue),
        ((3, 3), Hue::White),
    ] {
        let (x, y) = cell_pixel(col, row);
        let pixel = *image.get_pixel(x, y);
        eprintln!("cell ({col}, {row}) at ({x}, {y}): {pixel:?}");
        assert_eq!(hue(pixel), expected, "cell ({col}, {row}) at ({x}, {y})");
    }
}

#[test]
fn textured_cube_matches_golden() -> anyhow::Result<()> {
    let reg = ToolRegistry::with_engine_tools();
    let mut engine = Headless::new(SIZE, SIZE)?;
    let texture = call_json(
        &reg,
        &mut engine,
        "asset.load",
        json!({ "source": asset("checker.png") }),
    );
    assert_eq!(texture, json!({ "texture": 0 }));
    let cube = call_json(&reg, &mut engine, "mesh.builtin", json!({ "kind": "cube" }));
    let spawned = call_json(
        &reg,
        &mut engine,
        "entity.spawn",
        json!({
            "mesh": cube["handle"],
            "texture": texture["texture"],
            "rotation_euler_deg": [0, TURN_DEG, 0],
        }),
    );

    let list = call_json(&reg, &mut engine, "entity.list", json!({}));
    assert_eq!(list[0]["entity"], spawned["entity"]);
    assert_eq!(list[0]["texture"], 0);

    let image = render_frame(&reg, &mut engine);
    assert_checker_orientation(&image);
    assert_golden("textured_cube", &image, GoldenOpts::default());
    Ok(())
}

/// The same cube and checker loaded from a glTF file whose material names
/// the checker as its base-colour texture.
#[test]
fn gltf_base_color_texture_matches_golden() -> anyhow::Result<()> {
    let reg = ToolRegistry::with_engine_tools();
    let mut engine = Headless::new(SIZE, SIZE)?;
    let path = asset("textured_cube.gltf");
    let out = call_json(
        &reg,
        &mut engine,
        "scene.load_gltf",
        json!({ "path": path }),
    );
    assert_eq!(out["textures"], json!([0]), "{out}");
    let entity = out["entities"][0].clone();

    // The texture is cached under its stable `path#image=N` source, which
    // asset.load shares.
    let source = format!("{path}#image=0");
    let state = engine.resources.expect::<RenderState>();
    assert_eq!(
        state
            .textures
            .source(TextureHandle(0))
            .map(|s| s.to_string()),
        Some(source.clone())
    );
    assert_eq!(
        call_json(&reg, &mut engine, "asset.load", json!({ "source": source })),
        json!({ "texture": 0 })
    );

    call_json(
        &reg,
        &mut engine,
        "entity.set_transform",
        json!({ "entity": entity, "rotation_euler_deg": [0, TURN_DEG, 0] }),
    );
    let image = render_frame(&reg, &mut engine);
    assert_checker_orientation(&image);
    assert_golden("textured_cube", &image, GoldenOpts::default());
    Ok(())
}

#[test]
fn draw_calls_equal_distinct_mesh_texture_pairs() -> anyhow::Result<()> {
    let reg = ToolRegistry::with_engine_tools();
    let mut engine = Headless::new(32, 32)?;
    let load = |engine: &mut Headless, source: String| {
        call_json(&reg, engine, "asset.load", json!({ "source": source }))
    };
    let cube = load(&mut engine, "builtin://cube".into())["handle"].clone();
    let sphere = load(&mut engine, "builtin://sphere-8x4".into())["handle"].clone();
    // Same pixels, different sources: two textures.
    let a = load(&mut engine, asset("checker.png"))["texture"].clone();
    let b = load(
        &mut engine,
        format!("{}#image=0", asset("textured_cube.gltf")),
    )["texture"]
        .clone();
    assert_ne!(a, b);

    let spawn = |engine: &mut Headless, mesh: &Value, texture: Option<&Value>, count: usize| {
        for i in 0..count {
            let mut args = json!({ "mesh": mesh, "translation": [i as f32, 0, 0] });
            if let Some(texture) = texture {
                args["texture"] = texture.clone();
            }
            call_json(&reg, engine, "entity.spawn", args);
        }
    };
    spawn(&mut engine, &cube, None, 2);
    spawn(&mut engine, &cube, Some(&a), 3);
    spawn(&mut engine, &cube, Some(&b), 1);
    spawn(&mut engine, &sphere, Some(&a), 2);
    spawn(&mut engine, &sphere, None, 1);

    let draw_calls = |engine: &mut Headless| -> anyhow::Result<u32> {
        engine.render()?;
        Ok(engine
            .resources
            .expect::<RenderState>()
            .last_frame_draw_calls)
    };
    assert_eq!(draw_calls(&mut engine)?, 5, "five (mesh, texture) pairs");

    // More instances of existing pairs add no draw calls; a new pair adds one.
    spawn(&mut engine, &cube, Some(&a), 4);
    assert_eq!(draw_calls(&mut engine)?, 5);
    spawn(&mut engine, &sphere, Some(&b), 1);
    assert_eq!(draw_calls(&mut engine)?, 6);

    // Removing every instance of a pair removes its draw call.
    call_json(&reg, &mut engine, "scene.clear", json!({}));
    spawn(&mut engine, &cube, Some(&b), 2);
    assert_eq!(draw_calls(&mut engine)?, 1);
    Ok(())
}
