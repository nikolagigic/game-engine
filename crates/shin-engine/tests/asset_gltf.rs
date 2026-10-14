//! Integration tests for `shin::asset` against files on disk.

use shin::asset::{self, Vertex};
use shin::math::Transform;

const CUBE_GLTF: &str = "assets/test/cube.gltf";

#[test]
fn load_cube_gltf() -> anyhow::Result<()> {
    let scene = asset::load_gltf(CUBE_GLTF)?;

    // The asset stores the 24-vertex cube from `asset::cube()` with u16
    // indices, so this also exercises index widening.
    assert_eq!(scene.meshes.len(), 1);
    let mesh = &scene.meshes[0];
    assert_eq!(mesh.vertices.len(), 24);
    assert_eq!(mesh.indices.len(), 36);
    assert_eq!(mesh.indices, asset::cube().indices);
    assert_eq!(mesh.material, Some(0));

    assert_eq!(scene.nodes.len(), 1);
    assert_eq!(scene.nodes[0].mesh, Some(0));
    assert_eq!(scene.nodes[0].transform, Transform::IDENTITY);

    assert_eq!(scene.materials.len(), 1);
    assert_eq!(scene.materials[0].base_color, [0.8, 0.2, 0.2, 1.0]);
    assert_eq!(scene.materials[0].base_color_texture, None);
    Ok(())
}

#[test]
fn load_image_2x2_png() -> anyhow::Result<()> {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("asset_gltf_2x2.png");
    let pixels: Vec<u8> = (0..16).collect();
    image::RgbaImage::from_raw(2, 2, pixels.clone())
        .unwrap()
        .save(&path)?;

    let image = asset::load_image(&path)?;
    assert_eq!(image.width, 2);
    assert_eq!(image.height, 2);
    assert_eq!(image.rgba.len(), 16);
    assert_eq!(image.rgba, pixels);
    assert_eq!(image.name, "asset_gltf_2x2");
    Ok(())
}

#[test]
fn missing_files_report_their_path() {
    let path = "assets/test/does-not-exist.gltf";
    let err = asset::load_gltf(path).unwrap_err();
    assert!(format!("{err:#}").contains(path), "{err:#}");

    let path = "assets/test/does-not-exist.png";
    let err = asset::load_image(path).unwrap_err();
    assert!(format!("{err:#}").contains(path), "{err:#}");
}

/// Regenerates `assets/test/cube.gltf` and `cube.bin` from `asset::cube()`.
///
/// ```sh
/// cargo test -p shin-engine --test asset_gltf -- --ignored regenerate_cube_asset
/// ```
#[test]
#[ignore = "writes the committed test asset; run by hand"]
fn regenerate_cube_asset() -> anyhow::Result<()> {
    let cube = asset::cube();
    let vertex_bytes: &[u8] = bytemuck::cast_slice(&cube.vertices);
    let indices: Vec<u16> = cube.indices.iter().map(|&i| i as u16).collect();
    let index_bytes: &[u8] = bytemuck::cast_slice(&indices);
    let bin = [vertex_bytes, index_bytes].concat();

    let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in &cube.vertices {
        for k in 0..3 {
            min[k] = min[k].min(v.position[k]);
            max[k] = max[k].max(v.position[k]);
        }
    }
    let stride = size_of::<Vertex>();
    let json = serde_json::json!({
        "asset": { "version": "2.0", "generator": "engine tests/asset_gltf.rs regenerate_cube_asset" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "name": "cube", "mesh": 0 }],
        "meshes": [{
            "name": "cube",
            "primitives": [{
                "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 },
                "indices": 3,
                "material": 0
            }]
        }],
        "materials": [{
            "name": "red",
            "pbrMetallicRoughness": { "baseColorFactor": [0.8, 0.2, 0.2, 1.0] }
        }],
        "buffers": [{ "uri": "cube.bin", "byteLength": bin.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": vertex_bytes.len(), "byteStride": stride, "target": 34962 },
            { "buffer": 0, "byteOffset": vertex_bytes.len(), "byteLength": index_bytes.len(), "target": 34963 }
        ],
        "accessors": [
            { "bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": cube.vertices.len(), "type": "VEC3", "min": min, "max": max },
            { "bufferView": 0, "byteOffset": 12, "componentType": 5126, "count": cube.vertices.len(), "type": "VEC3" },
            { "bufferView": 0, "byteOffset": 24, "componentType": 5126, "count": cube.vertices.len(), "type": "VEC2" },
            { "bufferView": 1, "byteOffset": 0, "componentType": 5123, "count": indices.len(), "type": "SCALAR" }
        ]
    });

    std::fs::create_dir_all("assets/test")?;
    std::fs::write("assets/test/cube.bin", bin)?;
    std::fs::write(CUBE_GLTF, serde_json::to_string_pretty(&json)? + "\n")?;
    Ok(())
}
