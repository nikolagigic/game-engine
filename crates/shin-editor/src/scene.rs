//! The scene the editor shows until scene files exist (GE-021): the same
//! lit cube on a ground slab that `shin new` generates, built in code.

use std::path::Path;

use shin::asset::{AssetSource, Assets};
use shin::ecs::{Resources, World};
use shin::gpu::{GpuContext, OFFSCREEN_COLOR_FORMAT};
use shin::math::{Camera, Projection, Quat, Transform, Vec3};
use shin::render::{LightUniform, MeshInstance, RenderState};

/// Linear clear colour of the viewport: the starter game's dusky sky blue.
pub const SKY: wgpu::Color = wgpu::Color {
    r: 0.05,
    g: 0.08,
    b: 0.15,
    a: 1.0,
};

/// The project's cube mesh, relative to the project directory.
const CUBE: &str = "assets/cube.gltf";

/// Fills `world` and `res` with the starter scene of the project in
/// `project_dir`: a [`RenderState`] for offscreen targets with a
/// directional light, an [`Assets`] cache rooted at the project, a
/// perspective [`Camera`], a ground slab, and a cube resting on it.
///
/// The cube mesh is the project's `assets/cube.gltf`; if that does not load,
/// the built-in cube is used and a warning is logged.
pub fn build(
    world: &mut World,
    res: &mut Resources,
    gpu: &GpuContext,
    project_dir: &Path,
) -> anyhow::Result<()> {
    let mut state = RenderState::new(&gpu.device, OFFSCREEN_COLOR_FORMAT);
    state.light = LightUniform {
        direction: [-0.6, -1.0, -0.4, 0.0],
        color: [1.0, 0.95, 0.85, 1.0],
        ambient: [0.12, 0.13, 0.16, 1.0],
    };
    let mut assets = Assets::new(project_dir.to_path_buf());
    let cube = match assets.mesh(&gpu.device, &mut state.registry, &AssetSource::parse(CUBE)?) {
        Ok(handle) => handle,
        Err(err) => {
            log::warn!("{CUBE} did not load ({err:#}); using the built-in cube");
            assets.mesh(
                &gpu.device,
                &mut state.registry,
                &AssetSource::Builtin("cube".to_string()),
            )?
        }
    };
    res.insert(state);
    res.insert(assets);
    res.insert(Camera {
        transform: Transform::IDENTITY,
        projection: Projection::Perspective {
            fov_y_radians: 60f32.to_radians(),
            near: 0.1,
            far: 500.0,
        },
    });

    // Ground: the cube flattened into a slab whose top face is y = 0.
    world.spawn((
        Transform::from_trs(
            Vec3::new(0.0, -0.25, 0.0),
            Quat::IDENTITY,
            Vec3::new(20.0, 0.5, 20.0),
        ),
        MeshInstance {
            mesh: cube,
            color: [0.18, 0.2, 0.17, 1.0],
        },
    ));
    // The cube, turned a little so the light catches three faces.
    world.spawn((
        Transform::from_trs(
            Vec3::new(0.0, 0.5, 0.0),
            Quat::from_rotation_y(30f32.to_radians()),
            Vec3::ONE,
        ),
        MeshInstance {
            mesh: cube,
            color: [0.8, 0.25, 0.1, 1.0],
        },
    ));
    Ok(())
}
