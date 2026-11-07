//! 3D Selection Box and Gizmo Renderer implementing Horizon's RenderHooks.
//! Renders selection box wireframe lines, translucent fills, and coordinate Gizmo handles
//! directly into the world pass using wgpu.

use std::sync::Arc;
use parking_lot::Mutex;

use horizon_client::mod_api::{
    RenderHooks, RenderNeedsEvent, WorldDrawnEvent, wgpu,
};

use crate::ghost::GhostMode;
use crate::selection::{style::*, SelectionManager};

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct ShaderVertex {
    pos: [f32; 3],
    color: [f32; 4],
}

struct GpuPipelines {
    line_pipeline: wgpu::RenderPipeline,
    tri_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

pub struct YefiraRenderer {
    selection_manager: Arc<SelectionManager>,
    ghost_mode: Arc<GhostMode>,
    pipelines: Mutex<Option<GpuPipelines>>,
}

impl YefiraRenderer {
    pub fn new(selection_manager: Arc<SelectionManager>, ghost_mode: Arc<GhostMode>) -> Self {
        Self {
            selection_manager,
            ghost_mode,
            pipelines: Mutex::new(None),
        }
    }

    fn init_pipelines(&self, device: &wgpu::Device, format: wgpu::TextureFormat) -> GpuPipelines {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yefira_selection_shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
                r#"
                struct Uniforms {
                    view_proj: mat4x4<f32>,
                };
                @group(0) @binding(0) var<uniform> uniforms: Uniforms;

                struct VertexInput {
                    @location(0) pos: vec3<f32>,
                    @location(1) color: vec4<f32>,
                };

                struct VertexOutput {
                    @builtin(position) clip_pos: vec4<f32>,
                    @location(0) color: vec4<f32>,
                };

                @vertex
                fn vs_main(in: VertexInput) -> VertexOutput {
                    var out: VertexOutput;
                    out.clip_pos = uniforms.view_proj * vec4<f32>(in.pos, 1.0);
                    out.color = in.color;
                    return out;
                }

                @fragment
                fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
                    return in.color;
                }
                "#,
            )),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yefira_uniform_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yefira_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            ..Default::default()
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ShaderVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
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
            ],
        };

        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);

        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yefira_line_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(vertex_layout.clone())],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let tri_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yefira_tri_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(vertex_layout)],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        GpuPipelines {
            line_pipeline,
            tri_pipeline,
            bind_group_layout,
        }
    }
}

impl RenderHooks for YefiraRenderer {
    fn render_needs(&self, _event: &mut RenderNeedsEvent<'_>) {}

    fn world_drawn(&self, event: &mut WorldDrawnEvent<'_, '_>) {
        let world = &mut event.world;
        let camera = world.camera;

        let mut line_verts = Vec::<ShaderVertex>::new();
        let mut tri_verts = Vec::<ShaderVertex>::new();

        // 1. Draw Committed Selection Box
        if let Some(sel) = self.selection_manager.current_selection() {
            let min = [sel.min()[0] as f64, sel.min()[1] as f64, sel.min()[2] as f64];
            let max = [
                (sel.max()[0] + 1) as f64,
                (sel.max()[1] + 1) as f64,
                (sel.max()[2] + 1) as f64,
            ];

            let (stroke_color, fill_color) = if sel.is_oversized_default() {
                (
                    argb_to_f32_rgba(COLOR_OVERSIZED_STROKE),
                    argb_to_f32_rgba(COLOR_OVERSIZED_FILL),
                )
            } else {
                (
                    argb_to_f32_rgba(COLOR_NORMAL_STROKE),
                    argb_to_f32_rgba(COLOR_NORMAL_FILL),
                )
            };

            append_box_lines(&mut line_verts, min, max, camera, stroke_color);
            append_box_triangles(&mut tri_verts, min, max, camera, fill_color);
        } else {
            // Draw pos1 or pos2 cubes even before both are set
            if let Some(p1) = self.selection_manager.pos1() {
                let min = [p1[0] as f64, p1[1] as f64, p1[2] as f64];
                let max = [(p1[0] + 1) as f64, (p1[1] + 1) as f64, (p1[2] + 1) as f64];
                append_box_lines(&mut line_verts, min, max, camera, argb_to_f32_rgba(COLOR_HANDLE_POS1));
            }
            if let Some(p2) = self.selection_manager.pos2() {
                let min = [p2[0] as f64, p2[1] as f64, p2[2] as f64];
                let max = [(p2[0] + 1) as f64, (p2[1] + 1) as f64, (p2[2] + 1) as f64];
                append_box_lines(&mut line_verts, min, max, camera, argb_to_f32_rgba(COLOR_HANDLE_POS2));
            }
        }

        // 2. Draw Drag Preview Selection (Ghost Mode active drag)
        let ghost = self.ghost_mode.state();
        if let Some(preview_sel) = ghost.drag_preview_selection {
            let min = [
                preview_sel.min()[0] as f64,
                preview_sel.min()[1] as f64,
                preview_sel.min()[2] as f64,
            ];
            let max = [
                (preview_sel.max()[0] + 1) as f64,
                (preview_sel.max()[1] + 1) as f64,
                (preview_sel.max()[2] + 1) as f64,
            ];
            let (p_stroke, p_fill) = if preview_sel.is_oversized_default() {
                (
                    argb_to_f32_rgba(COLOR_PREVIEW_OVERSIZED_STROKE),
                    argb_to_f32_rgba(COLOR_PREVIEW_OVERSIZED_FILL),
                )
            } else {
                (
                    argb_to_f32_rgba(COLOR_PREVIEW_NORMAL_STROKE),
                    argb_to_f32_rgba(COLOR_PREVIEW_NORMAL_FILL),
                )
            };
            append_box_lines(&mut line_verts, min, max, camera, p_stroke);
            append_box_triangles(&mut tri_verts, min, max, camera, p_fill);
        } else if ghost.is_box_creating {
            if let (Some(s), Some(c)) = (ghost.box_create_start, ghost.box_create_current) {
                let min = [
                    s[0].min(c[0]) as f64,
                    s[1].min(c[1]) as f64,
                    s[2].min(c[2]) as f64,
                ];
                let max = [
                    (s[0].max(c[0]) + 1) as f64,
                    (s[1].max(c[1]) + 1) as f64,
                    (s[2].max(c[2]) + 1) as f64,
                ];
                append_box_lines(&mut line_verts, min, max, camera, argb_to_f32_rgba(COLOR_BOX_CREATE_NORMAL_STROKE));
                append_box_triangles(&mut tri_verts, min, max, camera, argb_to_f32_rgba(COLOR_BOX_CREATE_NORMAL_FILL));
            }
        }

        // 3. Draw Hovered Block in Ghost Mode (when not dragging and no selection)
        if ghost.active && !ghost.is_box_creating && self.selection_manager.current_selection().is_none() {
            if let Some(h) = ghost.hovered_block {
                let min = [h[0] as f64, h[1] as f64, h[2] as f64];
                let max = [(h[0] + 1) as f64, (h[1] + 1) as f64, (h[2] + 1) as f64];
                append_box_lines(&mut line_verts, min, max, camera, argb_to_f32_rgba(COLOR_HOVER_BLOCK_STROKE));
                append_box_triangles(&mut tri_verts, min, max, camera, argb_to_f32_rgba(COLOR_HOVER_BLOCK_FILL));
            }
        }

        // 4. Draw Gizmo handles in Ghost Mode
        if ghost.active {
            let p1 = ghost.drag_preview_pos1.or_else(|| self.selection_manager.pos1());
            let p2 = ghost.drag_preview_pos2.or_else(|| self.selection_manager.pos2());

            if let Some(pos1) = p1 {
                let origin = [pos1[0] as f64 + 0.5, pos1[1] as f64 + 0.5, pos1[2] as f64 + 0.5];
                append_gizmo_axes(&mut line_verts, origin, camera, 1.8);
                let cube_min = [origin[0] - 0.15, origin[1] - 0.15, origin[2] - 0.15];
                let cube_max = [origin[0] + 0.15, origin[1] + 0.15, origin[2] + 0.15];
                append_box_lines(&mut line_verts, cube_min, cube_max, camera, argb_to_f32_rgba(COLOR_HANDLE_POS1));
            }
            if let Some(pos2) = p2 {
                let origin = [pos2[0] as f64 + 0.5, pos2[1] as f64 + 0.5, pos2[2] as f64 + 0.5];
                append_gizmo_axes(&mut line_verts, origin, camera, 1.8);
                let cube_min = [origin[0] - 0.15, origin[1] - 0.15, origin[2] - 0.15];
                let cube_max = [origin[0] + 0.15, origin[1] + 0.15, origin[2] + 0.15];
                append_box_lines(&mut line_verts, cube_min, cube_max, camera, argb_to_f32_rgba(COLOR_HANDLE_POS2));
            }
            if let (Some(pos1), Some(pos2)) = (p1, p2) {
                let center = [
                    (pos1[0].min(pos2[0]) as f64 + pos1[0].max(pos2[0]) as f64 + 1.0) * 0.5,
                    (pos1[1].min(pos2[1]) as f64 + pos1[1].max(pos2[1]) as f64 + 1.0) * 0.5,
                    (pos1[2].min(pos2[2]) as f64 + pos1[2].max(pos2[2]) as f64 + 1.0) * 0.5,
                ];
                append_gizmo_axes(&mut line_verts, center, camera, 2.2);
                let cube_min = [center[0] - 0.2, center[1] - 0.2, center[2] - 0.2];
                let cube_max = [center[0] + 0.2, center[1] + 0.2, center[2] + 0.2];
                append_box_lines(&mut line_verts, cube_min, cube_max, camera, argb_to_f32_rgba(COLOR_HANDLE_CENTER));
            }
        }

        if line_verts.is_empty() && tri_verts.is_empty() {
            return;
        }

        // Initialize or retrieve cached GPU pipelines
        let mut pipelines_guard = self.pipelines.lock();
        let pipelines = pipelines_guard.get_or_insert_with(|| {
            self.init_pipelines(world.device, world.color_format)
        });

        // Upload Uniform Buffer (View Projection matrix)
        let mut uniform_bytes = Vec::with_capacity(64);
        for col in &world.view_proj.to_cols_array() {
            uniform_bytes.extend_from_slice(&col.to_ne_bytes());
        }

        use wgpu::util::DeviceExt;
        let uniform_buffer = world.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("yefira_uniform_buffer"),
            contents: &uniform_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = world.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yefira_bind_group"),
            layout: &pipelines.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Execute Render Pass directly onto the world's color target view
        let mut rpass = world.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("yefira_selection_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: world.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        rpass.set_bind_group(0, &bind_group, &[]);

        // Draw translucent faces first
        if !tri_verts.is_empty() {
            let mut tri_bytes = Vec::with_capacity(tri_verts.len() * 28);
            for v in &tri_verts {
                for c in v.pos {
                    tri_bytes.extend_from_slice(&c.to_ne_bytes());
                }
                for c in v.color {
                    tri_bytes.extend_from_slice(&c.to_ne_bytes());
                }
            }
            let tri_buf = world.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("yefira_tri_vertex_buffer"),
                contents: &tri_bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
            rpass.set_pipeline(&pipelines.tri_pipeline);
            rpass.set_vertex_buffer(0, tri_buf.slice(..));
            rpass.draw(0..tri_verts.len() as u32, 0..1);
        }

        // Draw wireframe lines on top
        if !line_verts.is_empty() {
            let mut line_bytes = Vec::with_capacity(line_verts.len() * 28);
            for v in &line_verts {
                for c in v.pos {
                    line_bytes.extend_from_slice(&c.to_ne_bytes());
                }
                for c in v.color {
                    line_bytes.extend_from_slice(&c.to_ne_bytes());
                }
            }
            let line_buf = world.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("yefira_line_vertex_buffer"),
                contents: &line_bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
            rpass.set_pipeline(&pipelines.line_pipeline);
            rpass.set_vertex_buffer(0, line_buf.slice(..));
            rpass.draw(0..line_verts.len() as u32, 0..1);
        }
    }
}

fn append_box_lines(
    out: &mut Vec<ShaderVertex>,
    min: [f64; 3],
    max: [f64; 3],
    cam: glam::DVec3,
    color: [f32; 4],
) {
    let p = |x: f64, y: f64, z: f64| -> [f32; 3] {
        [(x - cam.x) as f32, (y - cam.y) as f32, (z - cam.z) as f32]
    };

    let (x0, y0, z0) = (min[0], min[1], min[2]);
    let (x1, y1, z1) = (max[0], max[1], max[2]);

    let edges = [
        // Bottom 4 edges
        (p(x0, y0, z0), p(x1, y0, z0)),
        (p(x1, y0, z0), p(x1, y0, z1)),
        (p(x1, y0, z1), p(x0, y0, z1)),
        (p(x0, y0, z1), p(x0, y0, z0)),
        // Top 4 edges
        (p(x0, y1, z0), p(x1, y1, z0)),
        (p(x1, y1, z0), p(x1, y1, z1)),
        (p(x1, y1, z1), p(x0, y1, z1)),
        (p(x0, y1, z1), p(x0, y1, z0)),
        // Vertical 4 pillars
        (p(x0, y0, z0), p(x0, y1, z0)),
        (p(x1, y0, z0), p(x1, y1, z0)),
        (p(x1, y0, z1), p(x1, y1, z1)),
        (p(x0, y0, z1), p(x0, y1, z1)),
    ];

    for (start, end) in edges {
        out.push(ShaderVertex { pos: start, color });
        out.push(ShaderVertex { pos: end, color });
    }
}

fn append_box_triangles(
    out: &mut Vec<ShaderVertex>,
    min: [f64; 3],
    max: [f64; 3],
    cam: glam::DVec3,
    color: [f32; 4],
) {
    let p = |x: f64, y: f64, z: f64| -> [f32; 3] {
        [(x - cam.x) as f32, (y - cam.y) as f32, (z - cam.z) as f32]
    };

    let (x0, y0, z0) = (min[0], min[1], min[2]);
    let (x1, y1, z1) = (max[0], max[1], max[2]);

    let quad = |out: &mut Vec<ShaderVertex>, a, b, c, d| {
        out.push(ShaderVertex { pos: a, color });
        out.push(ShaderVertex { pos: b, color });
        out.push(ShaderVertex { pos: c, color });
        out.push(ShaderVertex { pos: a, color });
        out.push(ShaderVertex { pos: c, color });
        out.push(ShaderVertex { pos: d, color });
    };

    // Bottom
    quad(out, p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1));
    // Top
    quad(out, p(x0, y1, z0), p(x0, y1, z1), p(x1, y1, z1), p(x1, y1, z0));
    // North (-Z)
    quad(out, p(x0, y0, z0), p(x0, y1, z0), p(x1, y1, z0), p(x1, y0, z0));
    // South (+Z)
    quad(out, p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1), p(x0, y0, z1));
    // West (-X)
    quad(out, p(x0, y0, z1), p(x0, y1, z1), p(x0, y1, z0), p(x0, y0, z0));
    // East (+X)
    quad(out, p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1), p(x1, y0, z1));
}

fn append_gizmo_axes(
    out: &mut Vec<ShaderVertex>,
    origin: [f64; 3],
    cam: glam::DVec3,
    length: f64,
) {
    let p = |x: f64, y: f64, z: f64| -> [f32; 3] {
        [(x - cam.x) as f32, (y - cam.y) as f32, (z - cam.z) as f32]
    };

    let o = p(origin[0], origin[1], origin[2]);
    let col_x = argb_to_f32_rgba(COLOR_HANDLE_X);
    let col_y = argb_to_f32_rgba(COLOR_HANDLE_Y);
    let col_z = argb_to_f32_rgba(COLOR_HANDLE_Z);

    // X axis (Red)
    out.push(ShaderVertex { pos: o, color: col_x });
    out.push(ShaderVertex {
        pos: p(origin[0] + length, origin[1], origin[2]),
        color: col_x,
    });

    // Y axis (Green)
    out.push(ShaderVertex { pos: o, color: col_y });
    out.push(ShaderVertex {
        pos: p(origin[0], origin[1] + length, origin[2]),
        color: col_y,
    });

    // Z axis (Blue)
    out.push(ShaderVertex { pos: o, color: col_z });
    out.push(ShaderVertex {
        pos: p(origin[0], origin[1], origin[2] + length),
        color: col_z,
    });
}
