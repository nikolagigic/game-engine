//! Mesh upload, materials, WGSL shaders, instanced drawing.
//! Owned by GE-006 (single mesh) and GE-008 (instancing).
//!
//! [`GpuMesh::upload`] copies a CPU-side [`MeshData`] into vertex and index
//! buffers. [`MeshRenderer`] owns the lit pipeline (`render/mesh.wgsl`):
//! a camera, one directional light, a base colour, and an instance buffer
//! of [`InstanceRaw`] model matrices. Every draw is instanced, even when
//! there is only one instance.
//!
//! The pipeline renders into any colour format (use an sRGB one; the shader
//! outputs linear colour) with a [`DEPTH_FORMAT`] depth attachment.
//! [`begin_mesh_pass`] opens a pass with the matching attachments.

use wgpu::util::DeviceExt as _;

use crate::asset::{MeshData, Vertex};
use crate::gpu::DEPTH_FORMAT;
use crate::math::{Camera, Mat3, Transform};

/// A mesh living in GPU buffers, ready to draw with [`MeshRenderer`].
pub struct GpuMesh {
    /// [`Vertex`] data (position, normal, uv).
    pub vertex_buffer: wgpu::Buffer,
    /// `u32` triangle-list indices.
    pub index_buffer: wgpu::Buffer,
    /// Number of indices in [`Self::index_buffer`].
    pub index_count: u32,
}

impl GpuMesh {
    /// Uploads `mesh`'s vertices and indices into new GPU buffers.
    pub fn upload(device: &wgpu::Device, mesh: &MeshData) -> Self {
        let label = |kind: &str| format!("mesh '{}' {kind}", mesh.name);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&label("vertices")),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&label("indices")),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Self {
            vertex_buffer,
            index_buffer,
            index_count: mesh.indices.len() as u32,
        }
    }
}

/// Camera data for the shader (`@group(0) @binding(0)`).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    /// World-to-clip matrix, column-major.
    pub view_proj: [[f32; 4]; 4],
    /// World-space eye position in `xyz`; `w` is unused.
    pub camera_pos: [f32; 4],
}

impl CameraUniform {
    /// The uniform for `cam` rendering into a viewport of `aspect` (width / height).
    pub fn new(cam: &Camera, aspect: f32) -> Self {
        Self {
            view_proj: cam.view_projection(aspect).to_cols_array_2d(),
            camera_pos: cam.transform.translation.extend(1.0).to_array(),
        }
    }
}

/// One directional light plus ambient (`@group(0) @binding(1)`). Linear RGB.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightUniform {
    /// Direction the light travels, from the light toward the scene, in
    /// `xyz` (need not be normalised); `w` is unused. The shader lights a
    /// surface by `N · -direction`, so `[0, -1, 0, 0]` shines straight down.
    pub direction: [f32; 4],
    /// Light colour in `rgb`; `a` is unused.
    pub color: [f32; 4],
    /// Ambient light in `rgb`, added regardless of orientation; `a` is unused.
    pub ambient: [f32; 4],
}

impl Default for LightUniform {
    /// White light shining straight down, with a dim ambient term.
    fn default() -> Self {
        Self {
            direction: [0.0, -1.0, 0.0, 0.0],
            color: [1.0, 1.0, 1.0, 1.0],
            ambient: [0.1, 0.1, 0.1, 1.0],
        }
    }
}

/// Material data (`@group(0) @binding(2)`). Only a base colour for now.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
}

/// Per-instance data, read from vertex buffer slot 1.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceRaw {
    /// Model (local-to-world) matrix, column-major.
    pub model: [[f32; 4]; 4],
    /// Normal matrix (inverse-transpose of the model's upper 3x3): three
    /// columns, each padded to four floats.
    pub normal_mat: [[f32; 4]; 3],
    /// Linear RGBA colour, multiplied into the material base colour.
    pub color: [f32; 4],
}

/// Instance attributes: model matrix at locations 3..=6, normal matrix at
/// 7..=9, colour at 10. Locations 0..=2 are the [`Vertex`] attributes.
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
    3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
    7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
    10 => Float32x4,
];

/// [`Vertex`] attributes: position, normal, uv.
const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];

/// Layout of a [`Vertex`] buffer (slot 0).
const VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &VERTEX_ATTRIBUTES,
};

impl InstanceRaw {
    /// Instance data for an object placed at `t` with linear RGBA `color`.
    pub fn from_transform(t: &Transform, color: [f32; 4]) -> Self {
        let model = t.to_matrix();
        let normal = Mat3::from_mat4(model).inverse().transpose();
        let col = |v: crate::math::Vec3| v.extend(0.0).to_array();
        Self {
            model: model.to_cols_array_2d(),
            normal_mat: [col(normal.x_axis), col(normal.y_axis), col(normal.z_axis)],
            color,
        }
    }

    /// Vertex buffer layout of an instance buffer (slot 1, stepped per instance).
    pub const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<InstanceRaw>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &INSTANCE_ATTRIBUTES,
    };
}

/// Draws lit, instanced meshes with one pipeline.
pub struct MeshRenderer {
    pipeline: wgpu::RenderPipeline,
    camera_buf: wgpu::Buffer,
    light_buf: wgpu::Buffer,
    material_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instance_buf: wgpu::Buffer,
    instance_cap: usize,
}

impl MeshRenderer {
    /// Builds the pipeline for colour attachments of `color_format`. The
    /// depth attachment is always [`DEPTH_FORMAT`] (`Depth32Float`).
    ///
    /// Starts with an identity camera matrix, [`LightUniform::default`],
    /// a white base colour, and room for one instance.
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("render/mesh.wgsl").into()),
        });

        let uniform_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh bind group layout"),
            entries: &[uniform_entry(0), uniform_entry(1), uniform_entry(2)],
        });

        let uniform = |label, contents: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        };
        let camera = CameraUniform {
            view_proj: glam::Mat4::IDENTITY.to_cols_array_2d(),
            camera_pos: [0.0, 0.0, 0.0, 1.0],
        };
        let camera_buf = uniform("camera uniform", bytemuck::bytes_of(&camera));
        let light_buf = uniform(
            "light uniform",
            bytemuck::bytes_of(&LightUniform::default()),
        );
        let material = MaterialUniform {
            base_color: [1.0; 4],
        };
        let material_buf = uniform("material uniform", bytemuck::bytes_of(&material));

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: light_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: material_buf.as_entire_binding(),
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(VERTEX_LAYOUT), Some(InstanceRaw::LAYOUT)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let instance_cap = 1;
        Self {
            pipeline,
            camera_buf,
            light_buf,
            material_buf,
            bind_group,
            instance_buf: create_instance_buffer(device, instance_cap),
            instance_cap,
        }
    }

    /// Uploads `cam`'s view-projection (for a viewport of `aspect` =
    /// width / height) and eye position.
    pub fn set_camera(&self, queue: &wgpu::Queue, cam: &Camera, aspect: f32) {
        let uniform = CameraUniform::new(cam, aspect);
        queue.write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&uniform));
    }

    /// Uploads the directional light and ambient term.
    pub fn set_light(&self, queue: &wgpu::Queue, light: &LightUniform) {
        queue.write_buffer(&self.light_buf, 0, bytemuck::bytes_of(light));
    }

    /// Sets the material base colour (linear RGBA), multiplied with each
    /// instance colour. Defaults to white.
    pub fn set_base_color(&self, queue: &wgpu::Queue, base_color: [f32; 4]) {
        let uniform = MaterialUniform { base_color };
        queue.write_buffer(&self.material_buf, 0, bytemuck::bytes_of(&uniform));
    }

    /// How many instances the instance buffer holds before it must grow.
    pub fn instance_capacity(&self) -> usize {
        self.instance_cap
    }

    /// Draws `mesh` once per instance. Grows the instance buffer if needed.
    ///
    /// Instance data is written with `queue.write_buffer`, which takes
    /// effect at the next submit. The renderer has one instance buffer, so
    /// call `draw` at most once per submitted command buffer; a second call
    /// before submitting overwrites the first call's instances.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        mesh: &GpuMesh,
        instances: &[InstanceRaw],
    ) {
        if instances.is_empty() {
            return;
        }
        if instances.len() > self.instance_cap {
            self.instance_cap = instances.len().next_power_of_two();
            self.instance_buf = create_instance_buffer(device, self.instance_cap);
            log::debug!("instance buffer grown to {} instances", self.instance_cap);
        }
        let bytes: &[u8] = bytemuck::cast_slice(instances);
        queue.write_buffer(&self.instance_buf, 0, bytes);

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buf.slice(..bytes.len() as u64));
        pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, 0..instances.len() as u32);
    }
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instance buffer"),
        size: (capacity * size_of::<InstanceRaw>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Begins a render pass suited to [`MeshRenderer`]: colour cleared to
/// `clear` (linear), depth cleared to 1.0, both stored.
pub fn begin_mesh_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    color_view: &wgpu::TextureView,
    depth_view: &wgpu::TextureView,
    clear: wgpu::Color,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("mesh pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::{GpuContext, OFFSCREEN_COLOR_FORMAT, OffscreenTarget};
    use crate::math::{Projection, Quat, Vec3};

    #[test]
    fn layouts_match_struct_sizes() {
        assert_eq!(size_of::<InstanceRaw>(), 128);
        assert_eq!(size_of::<CameraUniform>(), 80);
        assert_eq!(size_of::<LightUniform>(), 48);
        let last = INSTANCE_ATTRIBUTES.last().unwrap();
        assert_eq!(last.offset as usize + 16, size_of::<InstanceRaw>());
        let last = VERTEX_ATTRIBUTES.last().unwrap();
        assert_eq!(last.offset as usize + 8, size_of::<Vertex>());
    }

    #[test]
    fn normal_matrix_undoes_non_uniform_scale() {
        let t = Transform::from_trs(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_rotation_y(0.7),
            Vec3::new(2.0, 1.0, 0.5),
        );
        let raw = InstanceRaw::from_transform(&t, [1.0; 4]);
        let n = Mat3::from_cols(
            Vec3::from_slice(&raw.normal_mat[0]),
            Vec3::from_slice(&raw.normal_mat[1]),
            Vec3::from_slice(&raw.normal_mat[2]),
        );
        // A transformed normal stays perpendicular to transformed tangents.
        let model = t.to_matrix();
        let normal = Vec3::new(1.0, 1.0, 0.0).normalize();
        let tangent = Vec3::new(1.0, -1.0, 0.0);
        let dot = (n * normal).dot(model.transform_vector3(tangent));
        assert!(dot.abs() < 1e-5, "{dot}");
        assert!(raw.normal_mat.iter().all(|c| c[3] == 0.0));
    }

    /// Renders `count` cube instances offscreen with `renderer`.
    fn render_instances(
        ctx: &GpuContext,
        target: &OffscreenTarget,
        renderer: &mut MeshRenderer,
        mesh: &GpuMesh,
        count: usize,
    ) {
        let instances: Vec<_> = (0..count)
            .map(|i| {
                let t = Transform::from_translation(Vec3::new(i as f32 * 0.01, 0.0, 0.0));
                InstanceRaw::from_transform(&t, [1.0, 0.5, 0.25, 1.0])
            })
            .collect();
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = begin_mesh_pass(
                &mut encoder,
                &target.color_view,
                &target.depth_view,
                wgpu::Color::BLACK,
            );
            renderer.draw(&ctx.device, &ctx.queue, &mut pass, mesh, &instances);
        }
        ctx.queue.submit([encoder.finish()]);
    }

    #[test]
    fn draw_grows_instance_buffer_without_validation_errors() -> anyhow::Result<()> {
        let ctx = GpuContext::new(None)?;
        let target = OffscreenTarget::new(&ctx.device, 32, 32);
        let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);

        let mut renderer = MeshRenderer::new(&ctx.device, OFFSCREEN_COLOR_FORMAT);
        let mut camera = Camera {
            transform: Transform::from_translation(Vec3::new(0.0, 2.0, 5.0)),
            projection: Projection::Perspective {
                fov_y_radians: 45f32.to_radians(),
                near: 0.1,
                far: 100.0,
            },
        };
        camera.transform.looking_at(Vec3::ZERO, Vec3::Y);
        renderer.set_camera(&ctx.queue, &camera, 1.0);
        let mesh = GpuMesh::upload(&ctx.device, &crate::asset::cube());

        render_instances(&ctx, &target, &mut renderer, &mesh, 1);
        assert_eq!(renderer.instance_capacity(), 1);
        render_instances(&ctx, &target, &mut renderer, &mesh, 1000);
        assert!(renderer.instance_capacity() >= 1000);

        let image = target.read_pixels(&ctx)?;
        let error = pollster::block_on(scope.pop());
        assert!(error.is_none(), "validation error: {error:?}");
        // The cube really was drawn: the centre is not the black clear colour.
        assert_ne!(image.get_pixel(16, 16).0, [0, 0, 0, 255]);
        Ok(())
    }
}
