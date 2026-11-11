//! Dibujo del visor 3D con wgpu, la única parte que toca la tarjeta gráfica.
//!
//! Dibuja a una textura propia que egui muestra como imagen, en tres
//! pasadas: opacas con profundidad; transparentes con mezcla independiente
//! del orden (weighted blended), para que no parpadeen al girar; y la
//! composición de las transparentes sobre el color opaco ya resuelto. Las
//! curvas de borde van en la pasada opaca, como cuadriláteros con ancho en
//! píxeles. Todo con suavizado 4×.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use egui::mutex::RwLock;
use hm_design::scene::{Piece, PieceId, Scene};
use wgpu::util::DeviceExt;

use super::camera::Camera;

/// Muestras del suavizado.
const SAMPLES: u32 = 4;
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// La imagen que se entrega a egui: egui-wgpu espera bytes ya codificados en
/// gamma (`Rgba8Unorm`) y no texturas sRGB, que leería linealizadas dos veces.
/// Se dibuja en ella por una vista sRGB de la misma textura.
const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const ACCUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const REVEAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// Cómo se dibuja una pieza. Color lineal; con alfa < 1 pasa por la mezcla
/// transparente.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PieceStyle {
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    light: [f32; 4],
    /// Ancho y alto del cuadro en píxeles, ancho de las curvas en píxeles y 0.
    viewport: [f32; 4],
}

/// Un tramo de curva: se dibuja como un cuadrilátero (seis vértices).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Segment {
    a: [f32; 3],
    b: [f32; 3],
}

/// Una pieza ya en la tarjeta.
struct GpuPiece {
    id: PieceId,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// 0 si la pieza no tiene triángulos.
    index_count: u32,
    bind_group: wgpu::BindGroup,
    transparent: bool,
    /// Tramos de sus curvas de borde.
    segments: wgpu::Buffer,
    segment_count: u32,
}

/// Las texturas de un tamaño.
struct Targets {
    size: [u32; 2],
    color_msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    /// Vista sRGB de la imagen: resolución y composición escriben aquí.
    resolved: wgpu::TextureView,
    /// Vista `Rgba8Unorm` de la misma imagen, la que lee egui.
    image: wgpu::TextureView,
    accum_msaa: wgpu::TextureView,
    accum: wgpu::TextureView,
    reveal_msaa: wgpu::TextureView,
    reveal: wgpu::TextureView,
    composite: wgpu::BindGroup,
}

/// Lo que se pintó la última vez: si no cambia, no se vuelve a pintar.
#[derive(Debug, Clone, PartialEq)]
struct Painted {
    size: [u32; 2],
    view_proj: [f32; 16],
    visible: Vec<bool>,
    background: [f32; 4],
    curve_width_px: f32,
}

pub(crate) struct Renderer3d {
    device: wgpu::Device,
    queue: wgpu::Queue,
    egui_renderer: Arc<RwLock<egui_wgpu::Renderer>>,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    piece_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    opaque: wgpu::RenderPipeline,
    transparent: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    curves: wgpu::RenderPipeline,
    /// Color de las curvas, común a todas las piezas.
    curve_bind_group: Option<wgpu::BindGroup>,
    pieces: Vec<GpuPiece>,
    targets: Option<Targets>,
    texture: Option<egui::TextureId>,
    painted: Option<Painted>,
}

impl Renderer3d {
    pub(crate) fn new(state: &egui_wgpu::RenderState) -> Self {
        let device = state.device.clone();
        let uniform_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("visor 3D: cámara"),
            entries: &[uniform_entry],
        });
        let piece_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("visor 3D: pieza"),
            entries: &[uniform_entry],
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("visor 3D: composición"),
            entries: &[texture_entry(0), texture_entry(1)],
        });
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visor 3D: cámara"),
            size: std::mem::size_of::<CameraUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("visor 3D: cámara"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let mesh_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/mesh.wgsl"));
        let composite_shader =
            device.create_shader_module(wgpu::include_wgsl!("shaders/composite.wgsl"));
        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("visor 3D: mallas"),
            bind_group_layouts: &[Some(&camera_layout), Some(&piece_layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let vertex_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let multisample = wgpu::MultisampleState {
            count: SAMPLES,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };
        let mesh_pipeline = |label: &str,
                             entry: &str,
                             targets: &[Option<wgpu::ColorTargetState>],
                             depth_write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&mesh_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &vertex_buffers,
                },
                // Sin descarte de caras: la tapa se ve por dentro y por fuera.
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample,
                fragment: Some(wgpu::FragmentState {
                    module: &mesh_shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque = mesh_pipeline(
            "visor 3D: opacas",
            "fs_opaque",
            &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            true,
        );
        let additive = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let reveal = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrc,
            operation: wgpu::BlendOperation::Add,
        };
        let transparent = mesh_pipeline(
            "visor 3D: transparentes",
            "fs_transparent",
            &[
                Some(wgpu::ColorTargetState {
                    format: ACCUM_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: additive,
                        alpha: additive,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: REVEAL_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: reveal,
                        alpha: reveal,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                }),
            ],
            false,
        );
        let line_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/line.wgsl"));
        let segment_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let curves = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("visor 3D: curvas"),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Segment>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &segment_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("visor 3D: composición"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });
        let composite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("visor 3D: composición"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            device,
            queue: state.queue.clone(),
            egui_renderer: state.renderer.clone(),
            camera_buffer,
            camera_bind_group,
            piece_layout,
            composite_layout,
            opaque,
            transparent,
            composite,
            curves,
            curve_bind_group: None,
            pieces: Vec::new(),
            targets: None,
            texture: None,
            painted: None,
        }
    }

    /// Sube las piezas de la escena con su estilo y el color lineal de las
    /// curvas. Una pieza sin triángulos ni curvas no se sube.
    pub(crate) fn set_scene(
        &mut self,
        scene: &Scene,
        style: &dyn Fn(PieceId) -> PieceStyle,
        curve_color: [f32; 4],
    ) {
        let curve_uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("visor 3D: color de curvas"),
                contents: bytemuck::cast_slice(&curve_color),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        self.curve_bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("visor 3D: curvas"),
            layout: &self.piece_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: curve_uniform.as_entire_binding(),
            }],
        }));
        self.pieces = scene
            .pieces
            .iter()
            .filter(|piece| {
                !piece.surface.triangles.is_empty()
                    || piece.curves.iter().any(|curve| curve.points.len() > 1)
            })
            .map(|piece| {
                let mesh = &piece.surface;
                let vertices: Vec<Vertex> = mesh
                    .vertices
                    .iter()
                    .zip(&mesh.normals)
                    .map(|(position, normal)| Vertex {
                        position: position.map(|value| value as f32),
                        normal: normal.map(|value| value as f32),
                    })
                    .collect();
                let indices: Vec<u32> = mesh.triangles.iter().flatten().copied().collect();
                let segments = segments_of(piece);
                let color = style(piece.id).color;
                let uniform = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("visor 3D: color de pieza"),
                        contents: bytemuck::cast_slice(&color),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                GpuPiece {
                    id: piece.id,
                    vertices: self
                        .device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("visor 3D: vértices"),
                            contents: bytemuck::cast_slice(&vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        }),
                    indices: self
                        .device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("visor 3D: índices"),
                            contents: bytemuck::cast_slice(&indices),
                            usage: wgpu::BufferUsages::INDEX,
                        }),
                    index_count: indices.len() as u32,
                    bind_group: self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("visor 3D: pieza"),
                        layout: &self.piece_layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        }],
                    }),
                    transparent: color[3] < 1.0,
                    segments: self
                        .device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("visor 3D: tramos de curva"),
                            contents: bytemuck::cast_slice(&segments),
                            usage: wgpu::BufferUsages::VERTEX,
                        }),
                    segment_count: segments.len() as u32,
                }
            })
            .collect();
        self.painted = None;
    }

    /// Pinta si algo cambió y devuelve la textura que egui muestra. `None` si
    /// `size_px` tiene un lado 0. `background` es color lineal; las curvas
    /// tienen `curve_width_px` píxeles de ancho.
    pub(crate) fn paint(
        &mut self,
        size_px: [u32; 2],
        camera: &Camera,
        visible: &dyn Fn(PieceId) -> bool,
        background: [f32; 4],
        curve_width_px: f32,
    ) -> Option<egui::TextureId> {
        if size_px.contains(&0) {
            return None;
        }
        let limit = self.device.limits().max_texture_dimension_2d;
        let size = size_px.map(|side| side.min(limit));
        let viewport = size.map(|side| side as f32);
        let view_proj = camera.view_proj(viewport);
        let painted = Painted {
            size,
            view_proj: view_proj.to_cols_array(),
            visible: self.pieces.iter().map(|piece| visible(piece.id)).collect(),
            background,
            curve_width_px,
        };
        if self.painted.as_ref() == Some(&painted) {
            return self.texture;
        }
        self.ensure_targets(size);
        let targets = self.targets.as_ref()?;

        let light = -camera.forward();
        let uniform = CameraUniform {
            view_proj: view_proj.to_cols_array_2d(),
            light: [light.x, light.y, light.z, 0.0],
            viewport: [viewport[0], viewport[1], curve_width_px, 0.0],
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&uniform));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("visor 3D"),
            });
        let shown = |transparent: bool| {
            self.pieces
                .iter()
                .zip(&painted.visible)
                .filter(move |(piece, shown)| **shown && piece.transparent == transparent)
                .map(|(piece, _)| piece)
        };
        {
            let [r, g, b, a] = background.map(f64::from);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("visor 3D: opacas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color_msaa,
                    depth_slice: None,
                    resolve_target: Some(&targets.resolved),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
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
            pass.set_pipeline(&self.opaque);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            for piece in shown(false) {
                draw(&mut pass, piece);
            }
            if let Some(curve_bind_group) = &self.curve_bind_group {
                pass.set_pipeline(&self.curves);
                pass.set_bind_group(1, curve_bind_group, &[]);
                let with_curves = self
                    .pieces
                    .iter()
                    .zip(&painted.visible)
                    .filter(|(piece, shown)| **shown && piece.segment_count > 0);
                for (piece, _) in with_curves {
                    pass.set_vertex_buffer(0, piece.segments.slice(..));
                    pass.draw(0..6, 0..piece.segment_count);
                }
            }
        }
        let any_transparent = shown(true).next().is_some();
        if any_transparent {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("visor 3D: transparentes"),
                    color_attachments: &[
                        Some(wgpu::RenderPassColorAttachment {
                            view: &targets.accum_msaa,
                            depth_slice: None,
                            resolve_target: Some(&targets.accum),
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        }),
                        Some(wgpu::RenderPassColorAttachment {
                            view: &targets.reveal_msaa,
                            depth_slice: None,
                            resolve_target: Some(&targets.reveal),
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                                store: wgpu::StoreOp::Store,
                            },
                        }),
                    ],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &targets.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.transparent);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                for piece in shown(true) {
                    draw(&mut pass, piece);
                }
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("visor 3D: composición"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.resolved,
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
            pass.set_pipeline(&self.composite);
            pass.set_bind_group(0, &targets.composite, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.painted = Some(painted);
        self.texture
    }

    /// Rehace las texturas si cambió el tamaño, y avisa a egui.
    fn ensure_targets(&mut self, size: [u32; 2]) {
        if self
            .targets
            .as_ref()
            .is_some_and(|targets| targets.size == size)
        {
            return;
        }
        let device = &self.device;
        let texture = |label: &str, format, samples, binding: bool| {
            let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
            if binding {
                usage |= wgpu::TextureUsages::TEXTURE_BINDING;
            }
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let image_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("visor 3D: imagen"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[COLOR_FORMAT],
        });
        let resolved = image_texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(COLOR_FORMAT),
            ..wgpu::TextureViewDescriptor::default()
        });
        let image = image_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let accum = texture("visor 3D: acumulación", ACCUM_FORMAT, 1, true);
        let reveal = texture("visor 3D: revelado", REVEAL_FORMAT, 1, true);
        let composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("visor 3D: composición"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&accum),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&reveal),
                },
            ],
        });
        let targets = Targets {
            size,
            color_msaa: texture("visor 3D: color", COLOR_FORMAT, SAMPLES, false),
            depth: texture("visor 3D: profundidad", DEPTH_FORMAT, SAMPLES, false),
            resolved,
            image,
            accum_msaa: texture("visor 3D: acumulación 4×", ACCUM_FORMAT, SAMPLES, false),
            accum,
            reveal_msaa: texture("visor 3D: revelado 4×", REVEAL_FORMAT, SAMPLES, false),
            reveal,
            composite,
        };
        let mut egui_renderer = self.egui_renderer.write();
        match self.texture {
            Some(id) => egui_renderer.update_egui_texture_from_wgpu_texture(
                device,
                &targets.image,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.texture = Some(egui_renderer.register_native_texture(
                    device,
                    &targets.image,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
        drop(egui_renderer);
        self.targets = Some(targets);
        self.painted = None;
    }
}

impl Drop for Renderer3d {
    fn drop(&mut self) {
        if let Some(id) = self.texture.take() {
            self.egui_renderer.write().free_texture(&id);
        }
    }
}

fn draw(pass: &mut wgpu::RenderPass<'_>, piece: &GpuPiece) {
    if piece.index_count == 0 {
        return;
    }
    pass.set_bind_group(1, &piece.bind_group, &[]);
    pass.set_vertex_buffer(0, piece.vertices.slice(..));
    pass.set_index_buffer(piece.indices.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..piece.index_count, 0, 0..1);
}

/// Los tramos de las curvas de una pieza, en f32; una curva cerrada une el
/// último punto con el primero.
fn segments_of(piece: &Piece) -> Vec<Segment> {
    let mut segments = Vec::new();
    for curve in &piece.curves {
        let points: Vec<[f32; 3]> = curve
            .points
            .iter()
            .map(|point| point.map(|value| value as f32))
            .collect();
        for pair in points.windows(2) {
            segments.push(Segment {
                a: pair[0],
                b: pair[1],
            });
        }
        if curve.closed && points.len() > 2 {
            segments.push(Segment {
                a: points[points.len() - 1],
                b: points[0],
            });
        }
    }
    segments
}
