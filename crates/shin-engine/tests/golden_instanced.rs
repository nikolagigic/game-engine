//! GE-008: instanced rendering from the ECS.
//!
//! A 10x10 grid of cubes spaced 1.5 apart, alternating two colours in a
//! checkerboard, seen from a camera pitched 45° down. Drawn through
//! `Headless` + `render_world`, so this exercises the whole ECS-to-GPU path.

mod common;

use common::{GoldenOpts, assert_golden};
use image::{Rgba, RgbaImage};
use shin::asset;
use shin::ecs::Entity;
use shin::gpu::OFFSCREEN_COLOR_FORMAT;
use shin::headless::Headless;
use shin::math::{Camera, Projection, Transform, Vec3};
use shin::render::{LightUniform, MeshHandle, MeshInstance, RenderState, render_world};

const SIZE: u32 = 256;
/// Cubes per side.
const N: usize = 10;
/// Distance between neighbouring cube centres.
const SPACING: f32 = 1.5;
/// Linear RGBA checkerboard colours: orange where `(i + j)` is even, blue
/// where it is odd.
const ORANGE: [f32; 4] = [0.8, 0.3, 0.1, 1.0];
const BLUE: [f32; 4] = [0.1, 0.3, 0.8, 1.0];
const GREEN: [f32; 4] = [0.1, 0.8, 0.1, 1.0];
/// Linear clear colour; encodes to sRGB 39 per channel.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.02,
    b: 0.02,
    a: 1.0,
};

/// Centre of cube `(i, j)`: the grid is centred on the origin in the XZ
/// plane, `i` along +X and `j` along +Z (toward the camera).
fn cube_position(i: usize, j: usize) -> Vec3 {
    let offset = (N as f32 - 1.0) / 2.0;
    Vec3::new(
        (i as f32 - offset) * SPACING,
        0.0,
        (j as f32 - offset) * SPACING,
    )
}

/// Camera at `(0, 18, 18)` looking at the origin: pitched 45° down.
fn camera() -> Camera {
    let mut transform = Transform::from_translation(Vec3::new(0.0, 18.0, 18.0));
    transform.looking_at(Vec3::ZERO, Vec3::Y);
    Camera {
        transform,
        projection: Projection::Perspective {
            fov_y_radians: 45f32.to_radians(),
            near: 0.1,
            far: 100.0,
        },
    }
}

/// Pixel that world point `p` projects to.
fn to_pixel(p: Vec3) -> (u32, u32) {
    let ndc = camera().view_projection(1.0).project_point3(p);
    let to_px = |v: f32| ((v * 0.5 + 0.5) * SIZE as f32) as u32;
    (to_px(ndc.x), to_px(-ndc.y))
}

/// Pixel at the centre of cube `(i, j)`'s top face.
fn top_face_pixel(i: usize, j: usize) -> (u32, u32) {
    to_pixel(cube_position(i, j) + Vec3::Y * 0.5)
}

/// The checkerboard scene: a headless engine with `RenderState`, a cube
/// mesh, a `Camera`, and `render_world` installed. Returns the cube
/// entities as `entities[j][i]`.
struct Scene {
    engine: Headless,
    cube: MeshHandle,
    entities: Vec<Vec<Entity>>,
}

fn grid_scene() -> anyhow::Result<Scene> {
    let mut engine = Headless::new(SIZE, SIZE)?;
    engine.clear_color = CLEAR;

    let mut state = RenderState::new(&engine.gpu.device, OFFSCREEN_COLOR_FORMAT);
    state.light = LightUniform {
        direction: [1.0, -2.0, -1.0, 0.0],
        color: [1.0, 1.0, 1.0, 1.0],
        ambient: [0.1, 0.1, 0.1, 1.0],
    };
    let cube = state.registry.add(&engine.gpu.device, &asset::cube());
    engine.resources.insert(state);
    engine.resources.insert(camera());
    engine.render_fn = Some(Box::new(|world, res, ctx, color, depth| {
        let size = color.texture().size();
        let aspect = size.width as f32 / size.height as f32;
        render_world(world, res, ctx, color, depth, aspect);
    }));

    let entities = (0..N)
        .map(|j| {
            (0..N)
                .map(|i| {
                    let color = if (i + j) % 2 == 0 { ORANGE } else { BLUE };
                    engine.world.spawn((
                        Transform::from_translation(cube_position(i, j)),
                        MeshInstance { mesh: cube, color },
                    ))
                })
                .collect()
        })
        .collect();
    Ok(Scene {
        engine,
        cube,
        entities,
    })
}

fn draw_calls(engine: &Headless) -> u32 {
    engine
        .resources
        .expect::<RenderState>()
        .last_frame_draw_calls
}

fn pixel(image: &RgbaImage, (x, y): (u32, u32)) -> Rgba<u8> {
    *image.get_pixel(x, y)
}

fn is_orange(p: Rgba<u8>) -> bool {
    p[0] > p[1] && p[1] > p[2]
}

fn is_blue(p: Rgba<u8>) -> bool {
    p[2] > p[1] && p[1] > p[0]
}

fn is_green(p: Rgba<u8>) -> bool {
    p[1] > p[0] + 50 && p[1] > p[2] + 50
}

/// sRGB encoding of the linear 0.02 clear colour.
const CLEAR_PIXEL: Rgba<u8> = Rgba([39, 39, 39, 255]);

#[test]
fn instanced_grid_matches_golden_with_one_draw_call() -> anyhow::Result<()> {
    let mut scene = grid_scene()?;
    let image = scene.engine.render()?;
    assert_eq!(draw_calls(&scene.engine), 1, "one mesh, one draw call");

    // Checkerboard at every corner and in the middle, and perspective: the
    // near row (j = 9) is wider on screen than the far row (j = 0).
    for (i, j) in [(0, 0), (9, 0), (0, 9), (9, 9), (4, 4), (5, 4)] {
        let p = pixel(&image, top_face_pixel(i, j));
        let expect_orange = (i + j) % 2 == 0;
        let ok = if expect_orange {
            is_orange(p)
        } else {
            is_blue(p)
        };
        assert!(ok, "cube ({i}, {j}) top face is {p:?}");
    }
    let width = |j| top_face_pixel(9, j).0 - top_face_pixel(0, j).0;
    assert!(
        width(9) > width(0) + 20,
        "near row {} px wide, far row {} px",
        width(9),
        width(0)
    );

    assert_golden("instanced_grid", &image, GoldenOpts::default());
    Ok(())
}

#[test]
fn second_mesh_adds_one_draw_call_and_both_appear() -> anyhow::Result<()> {
    let mut scene = grid_scene()?;
    let sphere = {
        let state = scene.engine.resources.expect_mut::<RenderState>();
        state
            .registry
            .add(&scene.engine.gpu.device, &asset::uv_sphere(32, 16))
    };
    assert_ne!(sphere, scene.cube);
    // Floats above the middle of the grid, between the four centre cubes.
    let sphere_centre = Vec3::new(0.0, 2.0, 0.0);
    scene.engine.world.spawn((
        Transform::from_translation(sphere_centre),
        MeshInstance {
            mesh: sphere,
            color: GREEN,
        },
    ));

    let image = scene.engine.render()?;
    assert_eq!(draw_calls(&scene.engine), 2, "two meshes, two draw calls");

    // Both draws of the one pass are visible: the sphere in the middle and
    // cubes in the far corners. (With one shared instance buffer slice per
    // pass, the second draw's data would replace the first's.)
    let p = pixel(&image, to_pixel(sphere_centre));
    assert!(is_green(p), "sphere centre is {p:?}");
    let p = pixel(&image, top_face_pixel(0, 0));
    assert!(is_orange(p), "cube (0, 0) is {p:?}");
    let p = pixel(&image, top_face_pixel(9, 9));
    assert!(is_orange(p), "cube (9, 9) is {p:?}");
    let p = pixel(&image, top_face_pixel(9, 0));
    assert!(is_blue(p), "cube (9, 0) is {p:?}");
    Ok(())
}

#[test]
fn despawned_entities_disappear_next_frame() -> anyhow::Result<()> {
    let mut scene = grid_scene()?;
    let before = scene.engine.render()?;

    // Despawn the far half of the grid (rows j = 0..5).
    for row in &scene.entities[..N / 2] {
        for &entity in row {
            scene.engine.world.despawn(entity)?;
        }
    }
    let after = scene.engine.render()?;
    assert_eq!(draw_calls(&scene.engine), 1);

    for j in 0..N {
        for i in 0..N {
            let at = top_face_pixel(i, j);
            let (b, a) = (pixel(&before, at), pixel(&after, at));
            if j < N / 2 {
                assert_ne!(b, CLEAR_PIXEL, "cube ({i}, {j}) was drawn before");
                assert_eq!(a, CLEAR_PIXEL, "cube ({i}, {j}) is gone after despawn");
            } else {
                assert_eq!(a, b, "cube ({i}, {j}) is unchanged");
            }
        }
    }

    // Despawning everything leaves just the clear colour and no draws.
    for row in &scene.entities[N / 2..] {
        for &entity in row {
            scene.engine.world.despawn(entity)?;
        }
    }
    let empty = scene.engine.render()?;
    assert_eq!(draw_calls(&scene.engine), 0);
    assert!(empty.pixels().all(|p| *p == CLEAR_PIXEL));
    Ok(())
}
