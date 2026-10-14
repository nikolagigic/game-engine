// Lit mesh shader: one directional light, Blinn-Phong, per-instance model
// matrices. All colours are linear; the sRGB target encodes on write.

struct Camera {
    view_proj: mat4x4<f32>,
    // xyz = world-space eye position, w unused.
    camera_pos: vec4<f32>,
};

struct Light {
    // xyz = direction the light travels (from the light into the scene).
    direction: vec4<f32>,
    // rgb = light colour, a unused.
    color: vec4<f32>,
    // rgb = ambient term, a unused.
    ambient: vec4<f32>,
};

struct Material {
    base_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> light: Light;
@group(0) @binding(2) var<uniform> material: Material;

// Base-colour texture (sRGB format, so samples are linear). Untextured
// draws bind a 1x1 white texture, which samples to exactly 1.0.
@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(1) var base_color_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct InstanceIn {
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) normal_0: vec4<f32>,
    @location(8) normal_1: vec4<f32>,
    @location(9) normal_2: vec4<f32>,
    @location(10) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

@vertex
fn vs_main(v: VertexIn, i: InstanceIn) -> VertexOut {
    let model = mat4x4<f32>(i.model_0, i.model_1, i.model_2, i.model_3);
    let normal_mat = mat3x3<f32>(i.normal_0.xyz, i.normal_1.xyz, i.normal_2.xyz);
    let world = model * vec4<f32>(v.position, 1.0);

    var out: VertexOut;
    out.clip_position = camera.view_proj * world;
    out.world_pos = world.xyz;
    out.world_normal = normal_mat * v.normal;
    out.color = i.color;
    out.uv = v.uv;
    return out;
}

const SHININESS: f32 = 32.0;
const SPECULAR_STRENGTH: f32 = 0.2;

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let l = normalize(-light.direction.xyz);
    let v = normalize(camera.camera_pos.xyz - in.world_pos);
    let h = normalize(l + v);

    let n_dot_l = max(dot(n, l), 0.0);
    let diffuse = n_dot_l * light.color.rgb;
    // No highlight on faces turned away from the light.
    let spec = select(0.0, pow(max(dot(n, h), 0.0), SHININESS), n_dot_l > 0.0);
    let specular = spec * light.color.rgb;

    let lit = light.ambient.rgb + diffuse + SPECULAR_STRENGTH * specular;
    let texel = textureSample(base_color_texture, base_color_sampler, in.uv);
    let albedo = texel * in.color * material.base_color;
    return vec4<f32>(lit * albedo.rgb, albedo.a);
}
