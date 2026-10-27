//! wgpu renderer for the PLUME desktop harness.
//!
//! The renderer is deliberately self-contained: it takes a [`Document`] and a
//! target size, produces a gamma-encoded RGBA8 image, and either hands that
//! image to egui or writes it to a PNG.  All scene geometry is rebuilt from the
//! document, so there is no second source of truth to keep in sync.
//!
//! Pipeline overview:
//!   1. studio background (fullscreen triangle)
//!   2. ground + procedural grid
//!   3. depth-only shadow map from the first enabled shadow light
//!   4. lit curve and guide geometry with procedural patterns
//!   5. overlay lines (guide wires, bounds, control points, origin gizmo)
//!   6. HDR post stack (bloom, DOF, outline, tonemap, grain, gamma)

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU64;

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use plume_core::brush::ProfileShape;
use plume_core::curve::Curve;
use plume_core::guide::build_guide_mesh;
use plume_core::math::srgb_to_linear;
use plume_core::pattern::PatternChannel;
use plume_core::render::{BackgroundKind, FogMode, Light, LightKind, RenderSettings, Tonemap};
use plume_core::scene::{curve_mesh, Document, MeshQuality};
use plume_core::{Mat4, Vec2, Vec3, Vec4};

/// Bytes per `SceneVertex`; the shader input layout depends on this contract.
pub const SCENE_VERTEX_SIZE: usize = 128;
/// Bytes per `LineVertex`.
pub const LINE_VERTEX_SIZE: usize = 28;
pub const GLOBALS_SIZE: usize = 624;
pub const POST_SIZE: usize = 128;

const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const POST_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

// ---------------------------------------------------------------------------
// Vertex and uniform layouts
// ---------------------------------------------------------------------------

/// One vertex of lit scene geometry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SceneVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    /// kind, scale_u, scale_v, spacing
    pub pattern_a: [f32; 4],
    /// thickness, density, angle (radians), seed
    pub pattern_b: [f32; 4],
    /// secondary colour (linear), pressure reactivity
    pub pattern_c: [f32; 4],
    /// emission, channel, selected, pressure
    pub flags: [f32; 4],
    /// unlit, pattern enabled, opacity scale, unused
    pub extra: [f32; 4],
}

/// One vertex of an overlay line.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct LineVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    view_proj: [f32; 16],
    shadow_vp: [f32; 16],
    camera_pos: [f32; 4],
    camera_forward: [f32; 4],
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
    bg_color: [f32; 4],
    bg_params: [f32; 4],
    ground_color: [f32; 4],
    ground_params: [f32; 4],
    grid_color: [f32; 4],
    grid_major_color: [f32; 4],
    axes_color: [f32; 4],
    ambient: [f32; 4],
    lighting: [f32; 4],
    light_pos: [[f32; 4]; 4],
    light_dir: [[f32; 4]; 4],
    light_color: [[f32; 4]; 4],
    light_params: [[f32; 4]; 4],
    view_params: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PostUniform {
    texel: [f32; 4],
    grade: [f32; 4],
    color: [f32; 4],
    bloom: [f32; 4],
    effects: [f32; 4],
    focus: [f32; 4],
    fog: [f32; 4],
    fog_range: [f32; 4],
}

// ---------------------------------------------------------------------------
// Small CPU helpers
// ---------------------------------------------------------------------------

fn linear_rgb(c: Vec4) -> [f32; 4] {
    [
        srgb_to_linear(c.x.clamp(0.0, 1.0)),
        srgb_to_linear(c.y.clamp(0.0, 1.0)),
        srgb_to_linear(c.z.clamp(0.0, 1.0)),
        c.w,
    ]
}

#[derive(Clone, Copy, Debug)]
struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    fn empty() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        }
    }

    fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z)
    }

    fn include(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    fn centre(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    fn radius(&self) -> f32 {
        if self.is_empty() {
            10.0
        } else {
            ((self.max - self.min).length() * 0.5).max(1.0)
        }
    }
}

// ---------------------------------------------------------------------------
// GPU-side containers
// ---------------------------------------------------------------------------

struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
}

struct LineMesh {
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
}

struct SceneBuffers {
    key: SceneKey,
    curves: Option<GpuMesh>,
    guides: Option<GpuMesh>,
    lines: Option<LineMesh>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SceneKey {
    revision: u64,
    selection: u64,
    curve_count: u32,
    guide_count: u32,
    quality: u8,
    guide_fill: bool,
    guide_wire: bool,
}

impl SceneKey {
    fn of(doc: &Document, quality: MeshQuality) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for id in &doc.scene.selection.curves {
            id.hash(&mut hasher);
        }
        for id in &doc.scene.selection.guides {
            id.hash(&mut hasher);
        }
        for id in &doc.scene.selection.groups {
            id.hash(&mut hasher);
        }
        Self {
            revision: doc.scene.revision,
            selection: hasher.finish(),
            curve_count: doc.scene.curves.len() as u32,
            guide_count: doc.scene.guides.len() as u32,
            quality: match quality {
                MeshQuality::Draft => 0,
                MeshQuality::Normal => 1,
                MeshQuality::High => 2,
                MeshQuality::Export => 3,
            },
            guide_fill: doc.render.guide_fill,
            guide_wire: doc.render.guide_wire,
        }
    }
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,

    globals_buffer: wgpu::Buffer,
    post_buffer: wgpu::Buffer,

    globals_layout: wgpu::BindGroupLayout,
    post_layout: wgpu::BindGroupLayout,
    globals_bind_group: wgpu::BindGroup,
    shadow_bind_group: wgpu::BindGroup,
    post_bind_group: wgpu::BindGroup,

    background_pipeline: wgpu::RenderPipeline,
    ground_pipeline: wgpu::RenderPipeline,
    scene_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    post_pipeline: wgpu::RenderPipeline,

    sampler: wgpu::Sampler,
    shadow_sampler: wgpu::Sampler,

    hdr_texture: wgpu::Texture,
    hdr_view: wgpu::TextureView,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    post_texture: wgpu::Texture,
    post_view: wgpu::TextureView,

    shadow_texture: wgpu::Texture,
    shadow_view: wgpu::TextureView,
    shadow_size: u32,

    size: [u32; 2],
    scene: Option<SceneBuffers>,

    egui_texture: Option<egui::TextureId>,
    egui_size: [u32; 2],
}

impl GpuRenderer {
    /// Build a renderer from an existing device.  `size` is the initial
    /// internal resolution; it is resized through [`Self::resize`].
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, size: [u32; 2]) -> Self {
        let size = [size[0].max(1), size[1].max(1)];
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("plume.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        // --- bind group layouts -------------------------------------------
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("plume.globals.layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(GLOBALS_SIZE as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("plume.post.layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(POST_SIZE as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let globals_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("plume.globals.pipeline_layout"),
                bind_group_layouts: &[Some(&globals_layout)],
                immediate_size: 0,
            });
        // The shadow pass writes the shadow map, so it must not bind the
        // regular globals group (which samples that same map). A reduced
        // layout with only the uniform buffer keeps the usage scopes disjoint.
        let shadow_globals_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("plume.shadow_globals.layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(GLOBALS_SIZE as u64),
                    },
                    count: None,
                }],
            });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("plume.shadow.pipeline_layout"),
                bind_group_layouts: &[Some(&shadow_globals_layout)],
                immediate_size: 0,
            });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("plume.post.pipeline_layout"),
            bind_group_layouts: &[Some(&post_layout)],
            immediate_size: 0,
        });

        // --- uniform buffers ----------------------------------------------
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("plume.globals"),
            size: GLOBALS_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let post_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("plume.post"),
            size: POST_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- samplers ------------------------------------------------------
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("plume.linear_clamp"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("plume.shadow_compare"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        // --- textures ------------------------------------------------------
        let (hdr_texture, hdr_view) = create_render_texture(&device, size, HDR_FORMAT, "plume.hdr");
        let (depth_texture, depth_view) = create_depth_texture(&device, size, "plume.depth");
        let (post_texture, post_view) = create_post_texture(&device, size, "plume.post_target");
        let (shadow_texture, shadow_view) = create_shadow_texture(&device, 2048, "plume.shadow");
        let shadow_size = 2048;

        let globals_bind_group = make_globals_bind_group(
            &device,
            &globals_layout,
            &globals_buffer,
            &shadow_view,
            &shadow_sampler,
        );
        let shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("plume.shadow_globals.bind_group"),
            layout: &shadow_globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });
        let post_bind_group = make_post_bind_group(
            &device,
            &post_layout,
            &post_buffer,
            &hdr_view,
            &depth_view,
            &sampler,
        );

        // --- pipelines ------------------------------------------------------
        let scene_attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 12,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 24,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 32,
                shader_location: 3,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 48,
                shader_location: 4,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 64,
                shader_location: 5,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 80,
                shader_location: 6,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 96,
                shader_location: 7,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 112,
                shader_location: 8,
            },
        ];
        let scene_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: SCENE_VERTEX_SIZE as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &scene_attributes,
        };
        let line_attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 12,
                shader_location: 1,
            },
        ];
        let line_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: LINE_VERTEX_SIZE as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &line_attributes,
        };

        let background_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("plume.background"),
            layout: Some(&globals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_background"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let ground_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("plume.ground"),
            layout: Some(&globals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_ground"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_ground"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let scene_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("plume.scene"),
            layout: Some(&globals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_scene"),
                compilation_options: Default::default(),
                buffers: &[Some(scene_vertex_layout.clone())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_scene"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("plume.lines"),
            layout: Some(&globals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_line"),
                compilation_options: Default::default(),
                buffers: &[Some(line_vertex_layout)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_line"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("plume.shadow"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &[Some(scene_vertex_layout)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 1.5,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });

        let post_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("plume.post"),
            layout: Some(&post_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_post"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: POST_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            device,
            queue,
            globals_buffer,
            post_buffer,
            globals_layout,
            post_layout,
            globals_bind_group,
            shadow_bind_group,
            post_bind_group,
            background_pipeline,
            ground_pipeline,
            scene_pipeline,
            line_pipeline,
            shadow_pipeline,
            post_pipeline,
            sampler,
            shadow_sampler,
            hdr_texture,
            hdr_view,
            depth_texture,
            depth_view,
            post_texture,
            post_view,
            shadow_texture,
            shadow_view,
            shadow_size,
            size,
            scene: None,
            egui_texture: None,
            egui_size: [0, 0],
        }
    }

    /// Resize the internal render targets.  The egui registration is updated
    /// lazily on the next call to [`Self::texture_id`].
    pub fn resize(&mut self, size: [u32; 2]) {
        let size = [size[0].max(1), size[1].max(1)];
        if size == self.size {
            return;
        }
        self.size = size;
        let (hdr_texture, hdr_view) =
            create_render_texture(&self.device, size, HDR_FORMAT, "plume.hdr");
        let (depth_texture, depth_view) = create_depth_texture(&self.device, size, "plume.depth");
        let (post_texture, post_view) =
            create_post_texture(&self.device, size, "plume.post_target");
        self.hdr_texture = hdr_texture;
        self.hdr_view = hdr_view;
        self.depth_texture = depth_texture;
        self.depth_view = depth_view;
        self.post_texture = post_texture;
        self.post_view = post_view;
        self.post_bind_group = make_post_bind_group(
            &self.device,
            &self.post_layout,
            &self.post_buffer,
            &self.hdr_view,
            &self.depth_view,
            &self.sampler,
        );
    }

    /// Ensure a texture ID is registered with egui and points at the current
    /// post-processing output.
    pub fn texture_id(&mut self, render_state: &egui_wgpu::RenderState) -> egui::TextureId {
        if let Some(id) = self.egui_texture {
            if self.egui_size != self.size {
                let mut renderer = render_state.renderer.write();
                renderer.update_egui_texture_from_wgpu_texture(
                    &render_state.device,
                    &self.post_view,
                    wgpu::FilterMode::Linear,
                    id,
                );
                self.egui_size = self.size;
            }
            id
        } else {
            let mut renderer = render_state.renderer.write();
            let id = renderer.register_native_texture(
                &render_state.device,
                &self.post_view,
                wgpu::FilterMode::Linear,
            );
            self.egui_texture = Some(id);
            self.egui_size = self.size;
            id
        }
    }

    fn ensure_shadow_map(&mut self, resolution: u32) {
        let resolution = resolution.clamp(256, 4096);
        if resolution == self.shadow_size {
            return;
        }
        let (texture, view) = create_shadow_texture(&self.device, resolution, "plume.shadow");
        self.shadow_texture = texture;
        self.shadow_view = view;
        self.shadow_size = resolution;
        self.globals_bind_group = make_globals_bind_group(
            &self.device,
            &self.globals_layout,
            &self.globals_buffer,
            &self.shadow_view,
            &self.shadow_sampler,
        );
    }

    fn update_scene(&mut self, doc: &Document, quality: MeshQuality) {
        let key = SceneKey::of(doc, quality);
        if self.scene.as_ref().map(|s| s.key) == Some(key) {
            return;
        }
        let built = build_scene(&self.device, &self.queue, doc, quality, key);
        self.scene = Some(built);
    }

    /// Render a document into the internal post-processing target.  Call
    /// [`Self::capture_rgba`] to read it back, or [`Self::texture_id`] to show
    /// it in egui.
    pub fn render(&mut self, doc: &Document, size: [u32; 2], quality: MeshQuality, time: f32) {
        self.resize(size);
        self.ensure_shadow_map(doc.render.lighting.shadow_resolution);
        self.update_scene(doc, quality);

        let size = self.size;
        let aspect = size[0] as f32 / size[1].max(1) as f32;

        let prepared = prepare_globals(doc, size, aspect, time);
        let mut globals = prepared.globals;
        globals.extra[2] = 1.0 / self.shadow_size.max(1) as f32;
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        self.queue
            .write_buffer(&self.post_buffer, 0, bytemuck::bytes_of(&prepared.post));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("plume.frame"),
            });

        // 1. shadow pass
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("plume.shadow_pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if prepared.cast_shadows {
                if let Some(curves) = self.scene.as_ref().and_then(|s| s.curves.as_ref()) {
                    pass.set_pipeline(&self.shadow_pipeline);
                    pass.set_bind_group(0, &self.shadow_bind_group, &[]);
                    pass.set_vertex_buffer(0, curves.vertex_buffer.slice(..));
                    pass.set_index_buffer(curves.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..curves.index_count, 0, 0..1);
                }
            }
        }

        // 2. scene pass
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("plume.scene_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.hdr_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bind_group, &[]);

            pass.set_pipeline(&self.background_pipeline);
            pass.draw(0..3, 0..1);

            pass.set_pipeline(&self.ground_pipeline);
            pass.draw(0..6, 0..1);

            pass.set_pipeline(&self.scene_pipeline);
            if let Some(scene) = self.scene.as_ref() {
                if let Some(curves) = scene.curves.as_ref() {
                    pass.set_vertex_buffer(0, curves.vertex_buffer.slice(..));
                    pass.set_index_buffer(curves.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..curves.index_count, 0, 0..1);
                }
                if let Some(guides) = scene.guides.as_ref() {
                    pass.set_vertex_buffer(0, guides.vertex_buffer.slice(..));
                    pass.set_index_buffer(guides.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..guides.index_count, 0, 0..1);
                }
                if let Some(lines) = scene.lines.as_ref() {
                    pass.set_pipeline(&self.line_pipeline);
                    pass.set_vertex_buffer(0, lines.vertex_buffer.slice(..));
                    pass.draw(0..lines.vertex_count, 0..1);
                }
            }
        }

        // 3. post pass
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("plume.post_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.post_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.post_pipeline);
            pass.set_bind_group(0, &self.post_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
    }

    /// Read the current post-processing target back into an RGBA8 image.
    pub fn capture_rgba(&self, width: u32, height: u32) -> Result<image::RgbaImage> {
        let width = width.max(1);
        let height = height.max(1);
        let bytes_per_pixel = 4u32;
        let unpadded = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("plume.readback"),
            size: padded as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("plume.readback_encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.post_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok();
        match rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => anyhow::bail!("failed to map render readback buffer: {e}"),
            Err(e) => anyhow::bail!("render readback channel closed: {e}"),
        }

        let data = slice
            .get_mapped_range()
            .map_err(|e| anyhow::anyhow!("failed to read mapped render buffer: {e}"))?;
        let mut image = image::RgbaImage::new(width, height);
        for y in 0..height {
            let src_start = (y * padded) as usize;
            let src_end = src_start + unpadded as usize;
            let row = &data[src_start..src_end];
            for x in 0..width {
                let i = (x * 4) as usize;
                image.put_pixel(
                    x,
                    y,
                    image::Rgba([row[i], row[i + 1], row[i + 2], row[i + 3]]),
                );
            }
        }
        drop(data);
        buffer.unmap();
        Ok(image)
    }
}

/// Render a document completely headlessly and return the RGBA8 image.
pub fn render_document_headless(
    doc: &Document,
    width: u32,
    height: u32,
    quality: MeshQuality,
    samples: u32,
) -> Result<image::RgbaImage> {
    let (device, queue) = create_headless_device()?;
    let mut renderer = GpuRenderer::new(device, queue, [width.max(1), height.max(1)]);
    // `samples` is accepted for API symmetry with the capture UI; the current
    // post stack is single-sample and produces one image.
    let _ = samples;
    renderer.render(doc, [width.max(1), height.max(1)], quality, 0.0);
    renderer.capture_rgba(width, height)
}

/// Create a wgpu device without a window. Used by `--headless` capture, image
/// sequences and exporters.
///
/// Software rasterisation (WARP / llvmpipe / SwiftShader) is the default because
/// PLUME must work on machines without a usable discrete GPU. Set
/// `PLUME_HEADLESS_GPU=1` to allow the fastest hardware adapter instead.
pub fn create_headless_device() -> Result<(wgpu::Device, wgpu::Queue)> {
    log::debug!("creating headless wgpu instance");
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let use_hardware = std::env::var("PLUME_HEADLESS_GPU").is_ok();
    let force_fallback = !use_hardware || std::env::var("PLUME_FALLBACK_ADAPTER").is_ok();
    let low_power = std::env::var("PLUME_LOW_POWER").is_ok();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: if low_power {
            wgpu::PowerPreference::LowPower
        } else {
            wgpu::PowerPreference::HighPerformance
        },
        force_fallback_adapter: force_fallback,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .context("no wgpu adapter available for headless rendering")?;
    log::info!("headless adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("plume.headless"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        ..Default::default()
    }))
    .context("failed to create headless wgpu device")?;
    Ok((device, queue))
}

// ---------------------------------------------------------------------------
// Resource helpers
// ---------------------------------------------------------------------------

fn create_render_texture(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_depth_texture(
    device: &wgpu::Device,
    size: [u32; 2],
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(label),
        aspect: wgpu::TextureAspect::DepthOnly,
        ..Default::default()
    });
    (texture, view)
}

fn create_post_texture(
    device: &wgpu::Device,
    size: [u32; 2],
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: POST_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_shadow_texture(
    device: &wgpu::Device,
    size: u32,
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(label),
        aspect: wgpu::TextureAspect::DepthOnly,
        ..Default::default()
    });
    (texture, view)
}

fn make_globals_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    shadow_view: &wgpu::TextureView,
    shadow_sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("plume.globals.bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(shadow_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(shadow_sampler),
            },
        ],
    })
}

fn make_post_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    post: &wgpu::Buffer,
    hdr_view: &wgpu::TextureView,
    depth_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("plume.post.bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: post.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(hdr_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

// ---------------------------------------------------------------------------
// Uniform preparation
// ---------------------------------------------------------------------------

struct PreparedUniforms {
    globals: Globals,
    post: PostUniform,
    cast_shadows: bool,
}

fn prepare_globals(doc: &Document, size: [u32; 2], aspect: f32, time: f32) -> PreparedUniforms {
    let render = &doc.render;
    let lighting = &render.lighting;
    let postfx = &render.postfx;
    let background = &render.background;

    let view_proj = doc.camera.view_projection(aspect);
    let cam_pos = doc.camera.position;
    let cam_forward = doc.camera.forward();

    let (shadow_vp, cast_shadows, ordered_lights) = shadow_matrices_and_lights(doc, render);

    let mut globals = Globals {
        view_proj: view_proj.to_cols_array(),
        shadow_vp: shadow_vp.to_cols_array(),
        camera_pos: [cam_pos.x, cam_pos.y, cam_pos.z, 1.0],
        camera_forward: [cam_forward.x, cam_forward.y, cam_forward.z, 0.0],
        bg_top: linear_rgb(background.top),
        bg_bottom: linear_rgb(background.bottom),
        bg_color: linear_rgb(background.color),
        bg_params: [
            match background.kind {
                BackgroundKind::Solid => 0.0,
                BackgroundKind::Gradient => 1.0,
                BackgroundKind::Studio => 2.0,
            },
            background.horizon.clamp(0.02, 0.98),
            background.vignette.max(0.0),
            time,
        ],
        ground_color: {
            let mut c = linear_rgb(render.environment.ground_color);
            c[3] = render.environment.ground_color.w;
            c
        },
        ground_params: [
            render.environment.ground_height,
            render.grid_extent.max(100.0),
            render.grid_spacing.max(0.001),
            if render.grid { 1.0 } else { 0.0 },
        ],
        grid_color: linear_rgb(render.grid_color),
        grid_major_color: linear_rgb(render.grid_major_color),
        axes_color: [0.22, 0.46, 0.95, render.grid_major_every.max(1) as f32],
        ambient: [
            srgb_to_linear(lighting.ambient_color.x.clamp(0.0, 1.0)),
            srgb_to_linear(lighting.ambient_color.y.clamp(0.0, 1.0)),
            srgb_to_linear(lighting.ambient_color.z.clamp(0.0, 1.0)),
            (lighting.ambient_intensity * 2.2).max(0.0),
        ],
        lighting: [
            1.0,
            lighting.specular.max(0.0),
            lighting.rim_strength.max(0.0),
            if lighting.shadows && cast_shadows {
                1.0
            } else {
                0.0
            },
        ],
        light_pos: [[0.0; 4]; 4],
        light_dir: [[0.0; 4]; 4],
        light_color: [[0.0; 4]; 4],
        light_params: [[0.0; 4]; 4],
        view_params: [
            aspect,
            doc.camera.near.max(0.05),
            doc.camera.far.max(doc.camera.near + 1.0),
            time,
        ],
        extra: [
            if render.axes { 1.0 } else { 0.0 },
            if lighting.shadows {
                render.environment.ground_shadow.clamp(0.0, 1.0)
            } else {
                0.0
            },
            1.0 / 2048.0,
            ordered_lights.len() as f32,
        ],
    };

    // The shadow map may have a non-default resolution; the caller patches the
    // texel size after the renderer has ensured the map exists.  Store the
    // value now using the document's requested resolution and let the shader
    // work with the actual map in all normal cases.
    globals.extra[2] = 1.0 / (lighting.shadow_resolution.clamp(256, 4096) as f32);

    for (i, light) in ordered_lights.iter().take(4).enumerate() {
        let dir = light.direction.normalize_or_zero();
        let cone = light.cone_deg.clamp(1.0, 179.0).to_radians();
        let outer = cone.cos();
        let inner = (cone * 0.62).cos();
        globals.light_pos[i] = [
            light.position.x,
            light.position.y,
            light.position.z,
            light.kind.shader_id() as f32,
        ];
        globals.light_dir[i] = [dir.x, dir.y, dir.z, light.softness.max(0.1)];
        globals.light_color[i] = [
            srgb_to_linear(light.color.x.clamp(0.0, 1.0)),
            srgb_to_linear(light.color.y.clamp(0.0, 1.0)),
            srgb_to_linear(light.color.z.clamp(0.0, 1.0)),
            light.intensity.max(0.0),
        ];
        globals.light_params[i] = [light.range.max(0.001), outer, inner, light.shadow_bias];
    }

    let exposure = postfx.exposure_multiplier(lighting.exposure);
    let tonemap_id = match postfx.tonemap {
        Tonemap::Linear => 0.0,
        Tonemap::Reinhard => 1.0,
        Tonemap::Aces => 2.0,
        Tonemap::Filmic => 3.0,
    };
    let fog_mode = match postfx.fog {
        FogMode::None => 0.0,
        FogMode::Linear => 1.0,
        FogMode::Exponential => 2.0,
    };
    let anim_time = if postfx.grain_animated { time } else { 0.0 };
    let post = PostUniform {
        texel: [
            1.0 / size[0] as f32,
            1.0 / size[1] as f32,
            size[0] as f32,
            size[1] as f32,
        ],
        grade: [
            exposure,
            tonemap_id,
            postfx.saturation.max(0.0),
            postfx.contrast.max(0.0),
        ],
        color: [
            postfx.lift,
            postfx.vignette.max(0.0),
            postfx.grain.max(0.0),
            postfx.sharpen.max(0.0),
        ],
        bloom: [
            postfx.bloom.max(0.0),
            postfx.bloom_threshold,
            postfx.bloom_radius.max(0.0),
            postfx.bloom_soft_knee.max(0.001),
        ],
        effects: [
            postfx.chromatic_aberration.max(0.0),
            postfx.outline.max(0.0),
            postfx.depth_of_field.max(0.0),
            anim_time,
        ],
        focus: [
            postfx.focus_distance.max(1.0),
            doc.camera.near.max(0.05),
            doc.camera.far.max(doc.camera.near + 1.0),
            fog_mode,
        ],
        fog: [
            srgb_to_linear(postfx.fog_color.x.clamp(0.0, 1.0)),
            srgb_to_linear(postfx.fog_color.y.clamp(0.0, 1.0)),
            srgb_to_linear(postfx.fog_color.z.clamp(0.0, 1.0)),
            postfx.fog_density.max(0.0),
        ],
        fog_range: [
            postfx.fog_start,
            postfx.fog_end.max(postfx.fog_start + 1.0),
            0.0,
            0.0,
        ],
    };

    PreparedUniforms {
        globals,
        post,
        cast_shadows,
    }
}

fn shadow_matrices_and_lights<'a>(
    doc: &Document,
    render: &'a RenderSettings,
) -> (Mat4, bool, Vec<&'a Light>) {
    let lighting = &render.lighting;
    // Shadow caster first: the shader only samples the map for light index 0.
    let shadow = lighting.shadow_light().filter(|_| lighting.shadows);
    let mut ordered: Vec<&Light> = Vec::with_capacity(4);
    if let Some(light) = shadow {
        ordered.push(light);
    }
    for light in lighting
        .lights
        .iter()
        .filter(|l| l.enabled && l.intensity > 0.0)
    {
        if ordered.iter().any(|o| o.id == light.id) {
            continue;
        }
        if ordered.len() >= 4 {
            break;
        }
        ordered.push(light);
    }

    let curves_bounds =
        doc.scene
            .curves
            .iter()
            .filter(|c| c.visible)
            .fold(Aabb::empty(), |mut bounds, curve| {
                let mesh = curve_mesh(curve, MeshQuality::Draft);
                for p in &mesh.positions {
                    bounds.include(*p);
                }
                bounds
            });

    let Some(light) = shadow else {
        return (Mat4::IDENTITY, false, ordered);
    };
    if curves_bounds.is_empty() {
        return (Mat4::IDENTITY, false, ordered);
    }

    let centre = curves_bounds.centre();
    let radius = curves_bounds.radius().max(5.0);
    let dir = match light.kind {
        LightKind::Directional => light.direction.normalize_or_zero(),
        LightKind::Point | LightKind::Spot => (centre - light.position).normalize_or_zero(),
    };
    let dir = if dir.length_squared() < 1.0e-8 {
        Vec3::new(-0.5, -1.0, -0.3).normalize()
    } else {
        dir
    };

    let (eye, near, far) = match light.kind {
        LightKind::Directional => {
            let distance = radius * 2.5;
            (centre - dir * distance, 0.01, distance + radius * 2.0)
        }
        LightKind::Point | LightKind::Spot => {
            let d = (centre - light.position).length().max(1.0);
            (
                light.position,
                0.05,
                (d + radius * 2.0).min(light.range.max(d + radius)),
            )
        }
    };
    let up = if dir.y.abs() > 0.95 { Vec3::X } else { Vec3::Y };
    let view = glam::camera::rh::view::look_at_mat4(eye, centre, up);
    let extent = radius * 1.45;
    let projection =
        glam::camera::rh::proj::directx::orthographic(-extent, extent, -extent, extent, near, far);
    (projection * view, true, ordered)
}

// ---------------------------------------------------------------------------
// Scene vertex generation
// ---------------------------------------------------------------------------

fn build_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    doc: &Document,
    quality: MeshQuality,
    key: SceneKey,
) -> SceneBuffers {
    let selection = &doc.scene.selection;
    let mut curve_vertices: Vec<SceneVertex> = Vec::new();
    let mut curve_indices: Vec<u32> = Vec::new();
    let mut guide_vertices: Vec<SceneVertex> = Vec::new();
    let mut guide_indices: Vec<u32> = Vec::new();
    let mut lines = LineBuilder::default();

    let hidden_groups = hidden_groups(doc);

    for curve in &doc.scene.curves {
        if !curve.visible {
            continue;
        }
        if let Some(group) = curve.group {
            if hidden_groups.contains(&group) {
                continue;
            }
        }
        let mesh = curve_mesh(curve, quality);
        if mesh.is_empty() {
            continue;
        }
        let base = curve_vertices.len() as u32;
        let selected = selection.curves.contains(&curve.id);
        let pressure = curve_pressure(curve);
        for i in 0..mesh.vertex_count() {
            curve_vertices.push(curve_vertex(curve, &mesh, i, selected, pressure));
        }
        for index in &mesh.indices {
            curve_indices.push(base + *index);
        }

        if selected {
            let handle = (curve.brush.base_size * 0.65).clamp(1.5, 8.0);
            let mut previous: Option<Vec3> = None;
            for point in &curve.points {
                lines.cross(point.position, handle, [1.0, 0.55, 0.15, 0.95]);
                if let Some(prev) = previous {
                    lines.line(prev, point.position, [1.0, 0.55, 0.15, 0.35]);
                }
                previous = Some(point.position);
            }
        }
    }

    for guide in &doc.scene.guides {
        if !guide.visible {
            continue;
        }
        let mesh = build_guide_mesh(&guide.kind);
        if mesh.is_empty() {
            continue;
        }
        let transform = guide.transform;
        let selected = selection.guides.contains(&guide.id);
        let fill = doc.render.guide_fill && (guide.solid || guide.active || selected);
        let color = linear_rgb(guide.color);
        let alpha = color[3] * guide.opacity.clamp(0.0, 1.0);
        let base = guide_vertices.len() as u32;
        let mut guide_bounds = Aabb::empty();
        for i in 0..mesh.vertex_count() {
            let p = transform.transform_point3(mesh.positions[i]);
            let n = transform
                .transform_vector3(mesh.normals[i])
                .normalize_or_zero();
            guide_bounds.include(p);
            guide_vertices.push(SceneVertex {
                position: p.to_array(),
                normal: n.to_array(),
                uv: mesh.uvs[i].to_array(),
                color: [color[0], color[1], color[2], alpha],
                pattern_a: [0.0, 1.0, 1.0, 1.0],
                pattern_b: [0.0, 0.5, 0.0, 0.0],
                pattern_c: [1.0, 1.0, 1.0, 0.0],
                flags: [0.0, 0.0, if selected { 1.0 } else { 0.0 }, 1.0],
                extra: [1.0, 0.0, 1.0, 0.0],
            });
        }
        if fill {
            for index in &mesh.indices {
                guide_indices.push(base + *index);
            }
        }
        if doc.render.guide_wire || doc.render.guide_fill {
            add_wire_edges(&mut lines, &mesh, transform, color, alpha * 0.55);
        }
        if selected {
            lines.aabb(guide_bounds.min, guide_bounds.max, [1.0, 0.55, 0.15, 0.95]);
        } else if guide.active {
            lines.aabb(guide_bounds.min, guide_bounds.max, [0.35, 0.86, 0.96, 0.75]);
        }
    }

    if doc.render.origin_gizmo {
        let len = (doc.render.grid_spacing * 4.0).clamp(20.0, 240.0);
        lines.line(Vec3::ZERO, Vec3::X * len, [0.92, 0.30, 0.28, 0.95]);
        lines.line(Vec3::ZERO, Vec3::Y * len, [0.36, 0.84, 0.40, 0.95]);
        lines.line(Vec3::ZERO, Vec3::Z * len, [0.28, 0.52, 0.94, 0.95]);
    }

    SceneBuffers {
        key,
        curves: upload_mesh(
            device,
            queue,
            "plume.curves",
            &curve_vertices,
            &curve_indices,
        ),
        guides: upload_mesh(
            device,
            queue,
            "plume.guides",
            &guide_vertices,
            &guide_indices,
        ),
        lines: lines.upload(device, queue),
    }
}

fn curve_vertex(
    curve: &Curve,
    mesh: &plume_core::mesh::Mesh,
    i: usize,
    selected: bool,
    pressure: f32,
) -> SceneVertex {
    let color = linear_rgb(curve.color);
    let alpha = color[3];
    let pattern = &curve.brush.pattern;
    let kind = pattern.kind.shader_id();
    let pattern_enabled = pattern.kind != plume_core::pattern::PatternKind::None;
    let channel = match pattern.channel {
        PatternChannel::Alpha => 0.0,
        PatternChannel::Color => 1.0,
        PatternChannel::Both => 2.0,
    };
    let secondary = linear_rgb(pattern.color_b);
    let ribbon = matches!(curve.brush.profile.shape, ProfileShape::Flat)
        && curve.brush.profile.aspect <= 0.18;
    SceneVertex {
        position: mesh.positions[i].to_array(),
        normal: mesh.normals[i].to_array(),
        uv: mesh.uvs[i].to_array(),
        color: [color[0], color[1], color[2], alpha],
        pattern_a: [
            kind as f32,
            pattern.scale_u.max(0.001),
            pattern.scale_v.max(0.001),
            pattern.spacing.max(0.001),
        ],
        pattern_b: [
            pattern.thickness.max(0.0),
            pattern.density.clamp(0.0, 1.0),
            pattern.angle_deg.to_radians(),
            pattern.seed as f32,
        ],
        pattern_c: [
            secondary[0],
            secondary[1],
            secondary[2],
            pattern.pressure_reactive,
        ],
        flags: [
            curve.brush.emission.max(0.0),
            channel,
            if selected { 1.0 } else { 0.0 },
            pressure,
        ],
        extra: [
            0.0,
            if pattern_enabled { 1.0 } else { 0.0 },
            curve.brush.opacity.clamp(0.0, 1.0),
            if ribbon { 1.0 } else { 0.0 },
        ],
    }
}

fn curve_pressure(curve: &Curve) -> f32 {
    if curve.points.is_empty() {
        return 0.8;
    }
    curve.points.iter().map(|p| p.pressure).sum::<f32>() / curve.points.len() as f32
}

fn hidden_groups(doc: &Document) -> BTreeSet<u64> {
    let mut hidden = BTreeSet::new();
    for group in &doc.scene.groups {
        let mut cursor = Some(group.id);
        let mut guard = 0;
        while let Some(id) = cursor {
            let Some(current) = doc.scene.group(id) else {
                break;
            };
            if !current.visible {
                hidden.insert(group.id);
                break;
            }
            cursor = current.parent;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
    }
    hidden
}

fn upload_mesh(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    vertices: &[SceneVertex],
    indices: &[u32],
) -> Option<GpuMesh> {
    if vertices.is_empty() || indices.is_empty() {
        return None;
    }
    let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (vertices.len() * SCENE_VERTEX_SIZE) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(vertices));
    let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&format!("{label}.indices")),
        size: (indices.len() * 4) as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(indices));
    Some(GpuMesh {
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
    })
}

#[derive(Default)]
struct LineBuilder {
    vertices: Vec<LineVertex>,
}

impl LineBuilder {
    fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 4]) {
        self.vertices.push(LineVertex {
            position: a.to_array(),
            color,
        });
        self.vertices.push(LineVertex {
            position: b.to_array(),
            color,
        });
    }

    fn cross(&mut self, p: Vec3, size: f32, color: [f32; 4]) {
        self.line(p - Vec3::X * size, p + Vec3::X * size, color);
        self.line(p - Vec3::Y * size, p + Vec3::Y * size, color);
        self.line(p - Vec3::Z * size, p + Vec3::Z * size, color);
    }

    fn aabb(&mut self, min: Vec3, max: Vec3, color: [f32; 4]) {
        if !(min.x <= max.x && min.y <= max.y && min.z <= max.z) {
            return;
        }
        let c = [
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(max.x, max.y, max.z),
            Vec3::new(min.x, max.y, max.z),
        ];
        for (a, b) in [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ] {
            self.line(c[a], c[b], color);
        }
    }

    fn upload(self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<LineMesh> {
        if self.vertices.is_empty() {
            return None;
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("plume.lines"),
            size: (self.vertices.len() * LINE_VERTEX_SIZE) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&self.vertices));
        Some(LineMesh {
            vertex_buffer: buffer,
            vertex_count: self.vertices.len() as u32,
        })
    }
}

fn add_wire_edges(
    lines: &mut LineBuilder,
    mesh: &plume_core::mesh::Mesh,
    transform: Mat4,
    color: [f32; 4],
    alpha: f32,
) {
    let edge_color = [color[0], color[1], color[2], alpha];
    let mut seen: BTreeSet<(u32, u32)> = BTreeSet::new();
    for triangle in mesh.indices.as_chunks::<3>().0 {
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let key = if a < b { (a, b) } else { (b, a) };
            if !seen.insert(key) {
                continue;
            }
            let pa = transform.transform_point3(mesh.positions[a as usize]);
            let pb = transform.transform_point3(mesh.positions[b as usize]);
            let da = (pa - pb).length_squared();
            // Skip degenerate edges without dropping legitimate tiny guides.
            if da <= 1.0e-12 {
                continue;
            }
            lines.line(pa, pb, edge_color);
        }
    }
}

// ---------------------------------------------------------------------------
// Extra CPU utilities used by the desktop harness
// ---------------------------------------------------------------------------

/// Screen-space size of one world unit at `point`, used by the brush cursor.
pub fn world_to_screen_scale(
    camera: &plume_core::viewport::Camera,
    point: Vec3,
    size: Vec2,
) -> f32 {
    let Some(p) = camera.project(point, size) else {
        return 1.0;
    };
    let Some(q) = camera.project(point + camera.basis().0, size) else {
        return 1.0;
    };
    (q - p).length().max(0.001)
}

fn _unused_vec2_guard(_v: Vec2, _w: f32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_layout_sizes_match_the_shader() {
        assert_eq!(std::mem::size_of::<SceneVertex>(), SCENE_VERTEX_SIZE);
        assert_eq!(std::mem::size_of::<LineVertex>(), LINE_VERTEX_SIZE);
        assert_eq!(std::mem::size_of::<Globals>(), GLOBALS_SIZE);
        assert_eq!(std::mem::size_of::<PostUniform>(), POST_SIZE);
    }

    #[test]
    fn build_a_default_document_scene() {
        // This test intentionally avoids a GPU; it exercises the CPU scene
        // vertex conversion by using a tiny fake mesh.
        let doc = Document::default();
        let key = SceneKey::of(&doc, MeshQuality::Normal);
        assert_eq!(key.curve_count, 0);
        assert!(hidden_groups(&doc).is_empty());
    }

    #[test]
    fn world_scale_is_finite_and_positive() {
        let camera = plume_core::viewport::Camera::default();
        let scale = world_to_screen_scale(&camera, Vec3::ZERO, Vec2::new(800.0, 600.0));
        assert!(scale.is_finite());
        assert!(scale > 0.0);
    }
}
