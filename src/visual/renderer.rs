//! 3D Selection Box and Gizmo Renderer implementing Horizon's RenderHooks.
//! Renders selection box wireframe lines, translucent fills, and coordinate Gizmo handles
//! directly into the world pass using wgpu with accurate depth testing and Always-On-Top handles.

use parking_lot::Mutex;
use std::sync::Arc;

use horizon_client::mod_api::wgpu::util::DeviceExt;
use horizon_client::mod_api::{RenderHooks, RenderNeedsEvent, WorldDrawnEvent, wgpu};

use crate::ghost::{
    AXIS_CENTER, AXIS_X, AXIS_Y, AXIS_Z, CORNER_CENTER, CORNER_NONE, CORNER_POS1, CORNER_POS2,
    GhostMode,
};
use crate::selection::{SelectionBox, SelectionManager, style::*};

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct ShaderVertex {
    pos: [f32; 3],
    color: [f32; 4],
}

struct GpuPipelines {
    tri_pipeline_occluded: wgpu::RenderPipeline,
    line_pipeline_occluded: wgpu::RenderPipeline,
    tri_pipeline_top: wgpu::RenderPipeline,
    line_pipeline_top: wgpu::RenderPipeline,
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

        // 1. Occluded Triangles Pipeline (Reverse-Z: GreaterEqual against terrain depth)
        let tri_pipeline_occluded =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("yefira_tri_pipeline_occluded"),
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
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });

        // 2. Occluded Lines Pipeline (Reverse-Z: GreaterEqual against terrain depth)
        let line_pipeline_occluded =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("yefira_line_pipeline_occluded"),
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
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });

        // 3. Always-On-Top Triangles Pipeline (CompareFunction::Always)
        let tri_pipeline_top = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yefira_tri_pipeline_top"),
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
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // 4. Always-On-Top Lines Pipeline (CompareFunction::Always)
        let line_pipeline_top = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yefira_line_pipeline_top"),
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
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        GpuPipelines {
            tri_pipeline_occluded,
            line_pipeline_occluded,
            tri_pipeline_top,
            line_pipeline_top,
            bind_group_layout,
        }
    }
}

impl RenderHooks for YefiraRenderer {
    fn render_needs(&self, event: &mut RenderNeedsEvent<'_>) {
        event.needs.world_depth = true;
    }

    fn world_drawn(&self, event: &mut WorldDrawnEvent<'_, '_>) {
        let world = &mut event.world;

        let mut pipelines_guard = self.pipelines.lock();
        if pipelines_guard.is_none() {
            *pipelines_guard = Some(self.init_pipelines(world.device, world.color_format));
        }
        let pipelines = pipelines_guard.as_ref().unwrap();

        // Separate vertex lists:
        // Occluded: tested against terrain depth (Selection boxes, box create preview, hovered block)
        let mut occluded_line_verts: Vec<ShaderVertex> = Vec::new();
        let mut occluded_tri_verts: Vec<ShaderVertex> = Vec::new();

        // Always-On-Top: drawn in front of blocks (Gizmo handles & axis arrows)
        let mut top_line_verts: Vec<ShaderVertex> = Vec::new();
        let mut top_tri_verts: Vec<ShaderVertex> = Vec::new();

        let cam = world.camera;
        let ghost = self.ghost_mode.state();

        // 1. Draw Active Selection Box (Committed selection) - Occluded by blocks
        if let Some(selection) = self.selection_manager.current_selection() {
            let min = selection.min();
            let max = selection.max();

            let min_f = [min[0] as f64, min[1] as f64, min[2] as f64];
            let max_f = [
                max[0] as f64 + 1.0,
                max[1] as f64 + 1.0,
                max[2] as f64 + 1.0,
            ];

            let is_oversized = selection.is_oversized_default();
            let fill_color = if is_oversized {
                COLOR_OVERSIZED_FILL
            } else {
                COLOR_NORMAL_FILL
            };
            let stroke_color = if is_oversized {
                COLOR_OVERSIZED_STROKE
            } else {
                COLOR_NORMAL_STROKE
            };

            append_box_triangles(
                &mut occluded_tri_verts,
                min_f,
                max_f,
                cam,
                argb_to_f32_rgba(fill_color),
            );
            append_box_lines(
                &mut occluded_line_verts,
                min_f,
                max_f,
                cam,
                argb_to_f32_rgba(stroke_color),
            );
        }

        // 2. Draw Drag Preview Selection (Ghost Mode active gizmo drag) - Occluded by blocks
        if let Some(preview_sel) = ghost.drag_preview_selection {
            let min = preview_sel.min();
            let max = preview_sel.max();

            let min_f = [min[0] as f64, min[1] as f64, min[2] as f64];
            let max_f = [
                max[0] as f64 + 1.0,
                max[1] as f64 + 1.0,
                max[2] as f64 + 1.0,
            ];

            let is_oversized = preview_sel.is_oversized_default();
            let fill_color = if is_oversized {
                COLOR_PREVIEW_OVERSIZED_FILL
            } else {
                COLOR_PREVIEW_NORMAL_FILL
            };
            let stroke_color = if is_oversized {
                COLOR_PREVIEW_OVERSIZED_STROKE
            } else {
                COLOR_PREVIEW_NORMAL_STROKE
            };

            append_box_triangles(
                &mut occluded_tri_verts,
                min_f,
                max_f,
                cam,
                argb_to_f32_rgba(fill_color),
            );
            append_box_lines(
                &mut occluded_line_verts,
                min_f,
                max_f,
                cam,
                argb_to_f32_rgba(stroke_color),
            );
        } else if ghost.is_box_creating {
            // Draw Box Creation Preview when dragging to create a new selection - Occluded by blocks
            if let (Some(s), Some(c)) = (ghost.box_create_start, ghost.box_create_current) {
                let box_preview = SelectionBox::new(s, c);
                let min = box_preview.min();
                let max = box_preview.max();

                let min_f = [min[0] as f64, min[1] as f64, min[2] as f64];
                let max_f = [
                    max[0] as f64 + 1.0,
                    max[1] as f64 + 1.0,
                    max[2] as f64 + 1.0,
                ];

                let is_oversized = box_preview.is_oversized_default();
                let fill_color = if is_oversized {
                    COLOR_BOX_CREATE_OVERSIZED_FILL
                } else {
                    COLOR_BOX_CREATE_NORMAL_FILL
                };
                let stroke_color = if is_oversized {
                    COLOR_BOX_CREATE_OVERSIZED_STROKE
                } else {
                    COLOR_BOX_CREATE_NORMAL_STROKE
                };

                append_box_triangles(
                    &mut occluded_tri_verts,
                    min_f,
                    max_f,
                    cam,
                    argb_to_f32_rgba(fill_color),
                );
                append_box_lines(
                    &mut occluded_line_verts,
                    min_f,
                    max_f,
                    cam,
                    argb_to_f32_rgba(stroke_color),
                );
            }
        }

        // 3. Draw Hovered Block in Ghost Mode (when not dragging, not creating, and no selection) - Occluded by blocks
        if ghost.active
            && !ghost.is_box_creating
            && ghost.dragging_corner == CORNER_NONE
            && self.selection_manager.current_selection().is_none()
            && let Some(h) = ghost.hovered_block
        {
            let min_f = [h[0] as f64, h[1] as f64, h[2] as f64];
            let max_f = [h[0] as f64 + 1.0, h[1] as f64 + 1.0, h[2] as f64 + 1.0];

            append_box_triangles(
                &mut occluded_tri_verts,
                min_f,
                max_f,
                cam,
                argb_to_f32_rgba(COLOR_HOVER_BLOCK_FILL),
            );
            append_box_lines(
                &mut occluded_line_verts,
                min_f,
                max_f,
                cam,
                argb_to_f32_rgba(COLOR_HOVER_BLOCK_STROKE),
            );
        }

        // 4. Draw Gizmo handles in Ghost Mode - Always on Top!
        if ghost.active && !ghost.is_box_creating {
            let p1 = ghost
                .drag_preview_pos1
                .or_else(|| self.selection_manager.pos1());
            let p2 = ghost
                .drag_preview_pos2
                .or_else(|| self.selection_manager.pos2());

            if let Some(pos1) = p1 {
                let origin = [
                    pos1[0] as f64 + 0.5,
                    pos1[1] as f64 + 0.5,
                    pos1[2] as f64 + 0.5,
                ];
                append_corner_gizmo(
                    &mut top_line_verts,
                    &mut top_tri_verts,
                    origin,
                    CORNER_POS1,
                    cam,
                    &ghost,
                    COLOR_HANDLE_POS1,
                );
            }

            if let Some(pos2) = p2 {
                let origin = [
                    pos2[0] as f64 + 0.5,
                    pos2[1] as f64 + 0.5,
                    pos2[2] as f64 + 0.5,
                ];
                append_corner_gizmo(
                    &mut top_line_verts,
                    &mut top_tri_verts,
                    origin,
                    CORNER_POS2,
                    cam,
                    &ghost,
                    COLOR_HANDLE_POS2,
                );
            }

            if let (Some(pos1), Some(pos2)) = (p1, p2) {
                let min_x = pos1[0].min(pos2[0]) as f64;
                let max_x = pos1[0].max(pos2[0]) as f64;
                let min_y = pos1[1].min(pos2[1]) as f64;
                let max_y = pos1[1].max(pos2[1]) as f64;
                let min_z = pos1[2].min(pos2[2]) as f64;
                let max_z = pos1[2].max(pos2[2]) as f64;
                let center = [
                    (min_x + max_x + 1.0) * 0.5,
                    (min_y + max_y + 1.0) * 0.5,
                    (min_z + max_z + 1.0) * 0.5,
                ];

                append_center_gizmo(&mut top_line_verts, &mut top_tri_verts, center, cam, &ghost);
            }
        }

        // Nothing to render
        if occluded_line_verts.is_empty()
            && occluded_tri_verts.is_empty()
            && top_line_verts.is_empty()
            && top_tri_verts.is_empty()
        {
            return;
        }

        // Upload view_proj uniform buffer
        let view_proj = world.view_proj;
        let mut uniform_bytes = [0u8; 64];
        for (i, val) in view_proj.to_cols_array().iter().enumerate() {
            uniform_bytes[i * 4..(i + 1) * 4].copy_from_slice(&val.to_ne_bytes());
        }

        let uniform_buf = world
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("yefira_uniform_buffer"),
                contents: &uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let bind_group = world.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yefira_bind_group"),
            layout: &pipelines.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        // Begin Render Pass with color and loaded depth
        let mut rpass = world
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yefira_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: world.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: world.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

        rpass.set_bind_group(0, &bind_group, &[]);

        // 1. Draw Occluded Triangles (depth tested against world terrain)
        if !occluded_tri_verts.is_empty() {
            let mut tri_bytes = Vec::with_capacity(occluded_tri_verts.len() * 28);
            for v in &occluded_tri_verts {
                for c in v.pos {
                    tri_bytes.extend_from_slice(&c.to_ne_bytes());
                }
                for c in v.color {
                    tri_bytes.extend_from_slice(&c.to_ne_bytes());
                }
            }
            let tri_buf = world
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("yefira_occluded_tri_buf"),
                    contents: &tri_bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                });
            rpass.set_pipeline(&pipelines.tri_pipeline_occluded);
            rpass.set_vertex_buffer(0, tri_buf.slice(..));
            rpass.draw(0..occluded_tri_verts.len() as u32, 0..1);
        }

        // 2. Draw Occluded Lines (depth tested against world terrain)
        if !occluded_line_verts.is_empty() {
            let mut line_bytes = Vec::with_capacity(occluded_line_verts.len() * 28);
            for v in &occluded_line_verts {
                for c in v.pos {
                    line_bytes.extend_from_slice(&c.to_ne_bytes());
                }
                for c in v.color {
                    line_bytes.extend_from_slice(&c.to_ne_bytes());
                }
            }
            let line_buf = world
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("yefira_occluded_line_buf"),
                    contents: &line_bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                });
            rpass.set_pipeline(&pipelines.line_pipeline_occluded);
            rpass.set_vertex_buffer(0, line_buf.slice(..));
            rpass.draw(0..occluded_line_verts.len() as u32, 0..1);
        }

        // 3. Draw Always-On-Top Triangles (Gizmo handle cubes)
        if !top_tri_verts.is_empty() {
            let mut top_tri_bytes = Vec::with_capacity(top_tri_verts.len() * 28);
            for v in &top_tri_verts {
                for c in v.pos {
                    top_tri_bytes.extend_from_slice(&c.to_ne_bytes());
                }
                for c in v.color {
                    top_tri_bytes.extend_from_slice(&c.to_ne_bytes());
                }
            }
            let top_tri_buf = world
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("yefira_top_tri_buf"),
                    contents: &top_tri_bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                });
            rpass.set_pipeline(&pipelines.tri_pipeline_top);
            rpass.set_vertex_buffer(0, top_tri_buf.slice(..));
            rpass.draw(0..top_tri_verts.len() as u32, 0..1);
        }

        // 4. Draw Always-On-Top Lines (Gizmo handle edges and 3D arrow shafts/pyramids)
        if !top_line_verts.is_empty() {
            let mut top_line_bytes = Vec::with_capacity(top_line_verts.len() * 28);
            for v in &top_line_verts {
                for c in v.pos {
                    top_line_bytes.extend_from_slice(&c.to_ne_bytes());
                }
                for c in v.color {
                    top_line_bytes.extend_from_slice(&c.to_ne_bytes());
                }
            }
            let top_line_buf = world
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("yefira_top_line_buf"),
                    contents: &top_line_bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                });
            rpass.set_pipeline(&pipelines.line_pipeline_top);
            rpass.set_vertex_buffer(0, top_line_buf.slice(..));
            rpass.draw(0..top_line_verts.len() as u32, 0..1);
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
    quad(
        out,
        p(x0, y0, z0),
        p(x1, y0, z0),
        p(x1, y0, z1),
        p(x0, y0, z1),
    );
    // Top
    quad(
        out,
        p(x0, y1, z0),
        p(x0, y1, z1),
        p(x1, y1, z1),
        p(x1, y1, z0),
    );
    // North (-Z)
    quad(
        out,
        p(x0, y0, z0),
        p(x0, y1, z0),
        p(x1, y1, z0),
        p(x1, y0, z0),
    );
    // South (+Z)
    quad(
        out,
        p(x1, y0, z1),
        p(x1, y1, z1),
        p(x0, y1, z1),
        p(x0, y0, z1),
    );
    // West (-X)
    quad(
        out,
        p(x0, y0, z1),
        p(x0, y1, z1),
        p(x0, y1, z0),
        p(x0, y0, z0),
    );
    // East (+X)
    quad(
        out,
        p(x1, y0, z0),
        p(x1, y1, z0),
        p(x1, y1, z1),
        p(x1, y0, z1),
    );
}

/// Appends a 3D coordinate axis arrow with shaft, base ring, diagonals, and pyramid arrowhead edges.
#[allow(clippy::too_many_arguments)]
fn append_axis_arrow(
    out: &mut Vec<ShaderVertex>,
    origin: [f64; 3],
    dir: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
    cam: glam::DVec3,
    axis_length: f64,
    color: [f32; 4],
) {
    let p = |x: f64, y: f64, z: f64| -> [f32; 3] {
        [(x - cam.x) as f32, (y - cam.y) as f32, (z - cam.z) as f32]
    };

    let tip = [
        origin[0] + dir[0] * axis_length,
        origin[1] + dir[1] * axis_length,
        origin[2] + dir[2] * axis_length,
    ];
    let head_len = axis_length * 0.28;
    let head_radius = head_len * 0.42;
    let base = [
        tip[0] - dir[0] * head_len,
        tip[1] - dir[1] * head_len,
        tip[2] - dir[2] * head_len,
    ];

    let p_orig = p(origin[0], origin[1], origin[2]);
    let p_tip = p(tip[0], tip[1], tip[2]);

    // 1. Shaft line
    out.push(ShaderVertex { pos: p_orig, color });
    out.push(ShaderVertex { pos: p_tip, color });

    // 2. Base perimeter vertices on the plane orthogonal to dir
    let b0 = p(
        base[0] + u[0] * head_radius,
        base[1] + u[1] * head_radius,
        base[2] + u[2] * head_radius,
    );
    let b1 = p(
        base[0] + v[0] * head_radius,
        base[1] + v[1] * head_radius,
        base[2] + v[2] * head_radius,
    );
    let b2 = p(
        base[0] - u[0] * head_radius,
        base[1] - u[1] * head_radius,
        base[2] - u[2] * head_radius,
    );
    let b3 = p(
        base[0] - v[0] * head_radius,
        base[1] - v[1] * head_radius,
        base[2] - v[2] * head_radius,
    );

    // 3. 4 side edges pointing to tip (forming 3D pyramid arrowhead)
    out.push(ShaderVertex { pos: b0, color });
    out.push(ShaderVertex { pos: p_tip, color });
    out.push(ShaderVertex { pos: b1, color });
    out.push(ShaderVertex { pos: p_tip, color });
    out.push(ShaderVertex { pos: b2, color });
    out.push(ShaderVertex { pos: p_tip, color });
    out.push(ShaderVertex { pos: b3, color });
    out.push(ShaderVertex { pos: p_tip, color });

    // 4. Closed base ring
    out.push(ShaderVertex { pos: b0, color });
    out.push(ShaderVertex { pos: b1, color });
    out.push(ShaderVertex { pos: b1, color });
    out.push(ShaderVertex { pos: b2, color });
    out.push(ShaderVertex { pos: b2, color });
    out.push(ShaderVertex { pos: b3, color });
    out.push(ShaderVertex { pos: b3, color });
    out.push(ShaderVertex { pos: b0, color });

    // 5. Cross diagonals on base
    out.push(ShaderVertex { pos: b0, color });
    out.push(ShaderVertex { pos: b2, color });
    out.push(ShaderVertex { pos: b1, color });
    out.push(ShaderVertex { pos: b3, color });
}

fn append_corner_gizmo(
    top_line_verts: &mut Vec<ShaderVertex>,
    top_tri_verts: &mut Vec<ShaderVertex>,
    origin: [f64; 3],
    corner_id: i32,
    cam: glam::DVec3,
    ghost: &crate::ghost::GhostState,
    default_handle_color: u32,
) {
    let dist =
        ((origin[0] - cam.x).powi(2) + (origin[1] - cam.y).powi(2) + (origin[2] - cam.z).powi(2))
            .sqrt()
            .max(0.5);
    let axis_len = (dist * 0.15).max(0.3);
    let cube_half = (dist * 0.025).max(0.06);

    let is_current_corner = (ghost.dragging_corner == corner_id)
        || (ghost.dragging_corner == CORNER_NONE && ghost.hovered_corner == corner_id);
    let active_axis = if ghost.dragging_corner != CORNER_NONE {
        ghost.dragging_axis
    } else {
        ghost.hovered_axis
    };

    // Center handle cube
    let center_highlighted = is_current_corner && active_axis == AXIS_CENTER;
    let center_color = if center_highlighted {
        COLOR_HANDLE_HIGHLIGHT
    } else {
        default_handle_color
    };
    let center_rgba = argb_to_f32_rgba(center_color);
    let center_fill_rgba = [center_rgba[0], center_rgba[1], center_rgba[2], 0.6];

    let min_cube = [
        origin[0] - cube_half,
        origin[1] - cube_half,
        origin[2] - cube_half,
    ];
    let max_cube = [
        origin[0] + cube_half,
        origin[1] + cube_half,
        origin[2] + cube_half,
    ];
    append_box_triangles(top_tri_verts, min_cube, max_cube, cam, center_fill_rgba);
    append_box_lines(top_line_verts, min_cube, max_cube, cam, center_rgba);

    // X Axis (Red)
    let x_highlighted = is_current_corner && active_axis == AXIS_X;
    let color_x = if x_highlighted {
        COLOR_HANDLE_HIGHLIGHT
    } else {
        COLOR_HANDLE_X
    };
    append_axis_arrow(
        top_line_verts,
        origin,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        cam,
        axis_len,
        argb_to_f32_rgba(color_x),
    );

    // Y Axis (Green)
    let y_highlighted = is_current_corner && active_axis == AXIS_Y;
    let color_y = if y_highlighted {
        COLOR_HANDLE_HIGHLIGHT
    } else {
        COLOR_HANDLE_Y
    };
    append_axis_arrow(
        top_line_verts,
        origin,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        cam,
        axis_len,
        argb_to_f32_rgba(color_y),
    );

    // Z Axis (Blue)
    let z_highlighted = is_current_corner && active_axis == AXIS_Z;
    let color_z = if z_highlighted {
        COLOR_HANDLE_HIGHLIGHT
    } else {
        COLOR_HANDLE_Z
    };
    append_axis_arrow(
        top_line_verts,
        origin,
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        cam,
        axis_len,
        argb_to_f32_rgba(color_z),
    );
}

fn append_center_gizmo(
    top_line_verts: &mut Vec<ShaderVertex>,
    top_tri_verts: &mut Vec<ShaderVertex>,
    center: [f64; 3],
    cam: glam::DVec3,
    ghost: &crate::ghost::GhostState,
) {
    let dist =
        ((center[0] - cam.x).powi(2) + (center[1] - cam.y).powi(2) + (center[2] - cam.z).powi(2))
            .sqrt()
            .max(0.5);
    let axis_len = (dist * 0.15).max(0.3);
    let cube_half = (dist * 0.035).max(0.08);

    let is_center_gizmo = (ghost.dragging_corner == CORNER_CENTER)
        || (ghost.dragging_corner == CORNER_NONE && ghost.hovered_corner == CORNER_CENTER);
    let active_axis = if ghost.dragging_corner != CORNER_NONE {
        ghost.dragging_axis
    } else {
        ghost.hovered_axis
    };

    let center_highlighted = is_center_gizmo && active_axis == AXIS_CENTER;
    let center_color = if center_highlighted {
        COLOR_HANDLE_HIGHLIGHT
    } else {
        COLOR_HANDLE_CENTER
    };
    let center_rgba = argb_to_f32_rgba(center_color);
    let center_fill_rgba = [center_rgba[0], center_rgba[1], center_rgba[2], 0.6];

    let min_cube = [
        center[0] - cube_half,
        center[1] - cube_half,
        center[2] - cube_half,
    ];
    let max_cube = [
        center[0] + cube_half,
        center[1] + cube_half,
        center[2] + cube_half,
    ];
    append_box_triangles(top_tri_verts, min_cube, max_cube, cam, center_fill_rgba);
    append_box_lines(top_line_verts, min_cube, max_cube, cam, center_rgba);

    // X Axis
    let x_highlighted = is_center_gizmo && active_axis == AXIS_X;
    let color_x = if x_highlighted {
        COLOR_HANDLE_HIGHLIGHT
    } else {
        COLOR_HANDLE_X
    };
    append_axis_arrow(
        top_line_verts,
        center,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        cam,
        axis_len,
        argb_to_f32_rgba(color_x),
    );

    // Y Axis
    let y_highlighted = is_center_gizmo && active_axis == AXIS_Y;
    let color_y = if y_highlighted {
        COLOR_HANDLE_HIGHLIGHT
    } else {
        COLOR_HANDLE_Y
    };
    append_axis_arrow(
        top_line_verts,
        center,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        cam,
        axis_len,
        argb_to_f32_rgba(color_y),
    );

    // Z Axis
    let z_highlighted = is_center_gizmo && active_axis == AXIS_Z;
    let color_z = if z_highlighted {
        COLOR_HANDLE_HIGHLIGHT
    } else {
        COLOR_HANDLE_Z
    };
    append_axis_arrow(
        top_line_verts,
        center,
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        cam,
        axis_len,
        argb_to_f32_rgba(color_z),
    );
}
