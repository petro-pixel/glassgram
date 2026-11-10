//! Real blur under the glass (wgpu).
//!
//! Frame: the background is rendered by a shader into an offscreen texture → downscaled 4× →
//! blurred twice with a Gaussian (H+V) → the background is drawn to the screen, and each glass panel
//! samples the blurred texture, and at its clear rim the sharp one, with an offset at the edges
//! (refraction) and chromatic aberration.
//!
//! Chat content under the bars: egui_wgpu paints the whole UI in one render pass and a pass can't
//! sample its own target, so the history can't be read back before the bars are drawn, and a second
//! egui pass would need the renderer's textures, which callbacks can't reach. So the history shapes
//! that lie under a glass bar are tessellated again on the CPU (`content.rs`: text as soft pills,
//! pictures from small thumbnails), painted over a half-size copy of the background, and blurred by a second
//! small chain; only panels painted over the content sample it. When nothing lies under a bar, none
//! of this runs.

use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use bytemuck::{Pod, Zeroable};
use eframe::egui::{self, Rect};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};

mod content;
pub use content::{ContentMark, Thumb};

const OFF_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DOWNSCALE: u32 = 4;
const PANEL_STRIDE: u64 = 256;
const MAX_PANELS: u32 = 512;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BgU {
    size: [f32; 2],
    time: f32,
    /// 1 means light theme.
    light: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurU {
    dir: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PanelU {
    rect: [f32; 4],
    screen: [f32; 2],
    radius: f32,
    opacity: f32,
    refract: f32,
    ppp: f32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ContentU {
    /// Screen size in points: the content mesh is in points.
    screen: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WallU {
    rect: [f32; 4],
    screen: [f32; 2],
    radius: f32,
    _pad: f32,
    /// Colour the wallpaper is mixed towards (alpha — how far), so text stays readable.
    veil: [f32; 4],
}

/// A chat wallpaper painted into the background before the blur, so glass over it refracts it.
#[derive(Clone)]
pub struct WallLayer {
    pub rect: Rect,
    pub radius: f32,
    pub image: Arc<egui::ColorImage>,
    /// Changes whenever the image does, so the texture is uploaded once.
    pub id: u64,
    pub veil: egui::Color32,
}

/// This frame's wallpaper: set while the UI is laid out, read when the frame is rendered.
static WALL: Mutex<Option<WallLayer>> = Mutex::new(None);

pub fn set_wallpaper(layer: Option<WallLayer>) {
    *WALL.lock().unwrap_or_else(|e| e.into_inner()) = layer;
}

/// The uploaded wallpaper texture and its bind group.
struct WallTex {
    id: u64,
    bind: wgpu::BindGroup,
}

struct Targets {
    size: [u32; 2],
    full: wgpu::TextureView,
    /// The background at half size with the chat content under the bars painted over it. Half size
    /// keeps the stand-in shapes soft where a bar's clear rim shows them.
    content: wgpu::TextureView,
    /// `content` downscaled and blurred.
    c: wgpu::TextureView,
    content_bind: wgpu::BindGroup,
    c_h_bind: wgpu::BindGroup,
    panel_content_bind: wgpu::BindGroup,
    a: wgpu::TextureView,
    b: wgpu::TextureView,
    bg_bind: wgpu::BindGroup,
    full_bind: wgpu::BindGroup,
    a_h_bind: wgpu::BindGroup,
    b_v_bind: wgpu::BindGroup,
    panel_bind: wgpu::BindGroup,
}

pub struct GlassGpu {
    bg_pipe: wgpu::RenderPipeline,
    copy_pipe: wgpu::RenderPipeline,
    blur_pipe: wgpu::RenderPipeline,
    screen_pipe: wgpu::RenderPipeline,
    panel_pipe: wgpu::RenderPipeline,
    bg_layout: wgpu::BindGroupLayout,
    tex_layout: wgpu::BindGroupLayout,
    panel_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bg_ubuf: wgpu::Buffer,
    blur_h: wgpu::Buffer,
    blur_v: wgpu::Buffer,
    panel_ubuf: wgpu::Buffer,
    targets: Option<Targets>,
    panel_count: u32,
    wall_pipe: wgpu::RenderPipeline,
    wall_layout: wgpu::BindGroupLayout,
    wall_ubuf: wgpu::Buffer,
    wall: Option<WallTex>,
    content_pipe: wgpu::RenderPipeline,
    content_ubuf: wgpu::Buffer,
    content_bind: wgpu::BindGroup,
    /// Vertex and index buffers, grown as needed.
    content_bufs: Option<(wgpu::Buffer, wgpu::Buffer)>,
    /// Whether this frame's content was painted and blurred.
    content_ready: bool,
}

/// Registers GPU resources in the egui renderer. Returns `false` if wgpu is unavailable.
pub fn install(cc: &eframe::CreationContext<'_>) -> bool {
    let Some(rs) = &cc.wgpu_render_state else { return false };
    let gpu = GlassGpu::new(&rs.device, rs.target_format);
    rs.renderer.write().callback_resources.insert(gpu);
    true
}

pub fn background_callback(rect: Rect, time: f32, light: bool) -> egui::PaintCallback {
    egui_wgpu::Callback::new_paint_callback(rect, BgCallback { time, light })
}

pub fn panel_callback(rect: Rect, radius: f32, opacity: f32, refract: f32) -> egui::PaintCallback {
    egui_wgpu::Callback::new_paint_callback(
        rect,
        PanelCallback { rect, radius, opacity, refract, content: content::over_content(rect), idx: AtomicU32::new(u32::MAX) },
    )
}

fn uniform_entry(binding: u32, dynamic: bool, size: u64) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: NonZeroU64::new(size),
        },
        count: None,
    }
}

fn tex_entries(tex: u32) -> [wgpu::BindGroupLayoutEntry; 2] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: tex,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: tex + 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
    ]
}

impl GlassGpu {
    fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("glass.wgsl").into()),
        });

        let bg_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass-bg"),
            entries: &[uniform_entry(0, false, size_of::<BgU>() as u64)],
        });
        let [t0, t1] = tex_entries(1);
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass-tex"),
            entries: &[t0, t1, uniform_entry(3, false, size_of::<BlurU>() as u64)],
        });
        let [p0, p1] = tex_entries(4);
        // The sharp background too: the clear rim refracts it, only the middle is frosted.
        let [p2, p3] = tex_entries(10);
        let panel_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass-panel"),
            entries: &[p0, p1, uniform_entry(6, true, size_of::<PanelU>() as u64), p2, p3],
        });

        let [w0, w1] = tex_entries(7);
        let wall_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass-wall"),
            entries: &[w0, w1, uniform_entry(9, false, size_of::<WallU>() as u64)],
        });

        let pipe = |label: &str, layout: &wgpu::BindGroupLayout, vs: &str, fs: &str, format, blend| {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let premul = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let bg_pipe = pipe("glass-bg", &bg_layout, "vs_full", "fs_bg", OFF_FORMAT, None);
        let copy_pipe = pipe("glass-copy", &tex_layout, "vs_full", "fs_copy", OFF_FORMAT, None);
        let blur_pipe = pipe("glass-blur", &tex_layout, "vs_full", "fs_blur", OFF_FORMAT, None);
        let screen_pipe = pipe("glass-screen", &tex_layout, "vs_full", "fs_copy", target, None);
        let panel_pipe = pipe("glass-panel", &panel_layout, "vs_panel", "fs_panel", target, premul);
        let wall_pipe = pipe("glass-wall", &wall_layout, "vs_wall", "fs_wall", OFF_FORMAT, premul);

        let content_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass-content"),
            entries: &[uniform_entry(12, false, size_of::<ContentU>() as u64)],
        });
        let content_pipe = {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("glass-content"),
                bind_group_layouts: &[Some(&content_layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("glass-content"),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_content"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<content::Vert>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Unorm8x4],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_content"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: OFF_FORMAT, blend: premul, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass-linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let ubuf = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let content_ubuf = ubuf("glass-content-u", size_of::<ContentU>() as u64);
        Self {
            bg_pipe,
            copy_pipe,
            blur_pipe,
            screen_pipe,
            panel_pipe,
            bg_layout,
            tex_layout,
            panel_layout,
            sampler,
            bg_ubuf: ubuf("glass-bg-u", size_of::<BgU>() as u64),
            blur_h: ubuf("glass-blur-h", size_of::<BlurU>() as u64),
            blur_v: ubuf("glass-blur-v", size_of::<BlurU>() as u64),
            panel_ubuf: ubuf("glass-panel-u", PANEL_STRIDE * MAX_PANELS as u64),
            targets: None,
            panel_count: 0,
            wall_pipe,
            wall_layout,
            wall_ubuf: ubuf("glass-wall-u", size_of::<WallU>() as u64),
            wall: None,
            content_bind: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &content_layout,
                entries: &[wgpu::BindGroupEntry { binding: 12, resource: content_ubuf.as_entire_binding() }],
            }),
            content_pipe,
            content_ubuf,
            content_bufs: None,
            content_ready: false,
        }
    }

    /// Uploads the wallpaper image when it changed.
    fn ensure_wall(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layer: &WallLayer) {
        if self.wall.as_ref().is_some_and(|w| w.id == layer.id) {
            return;
        }
        let [w, h] = [layer.image.size[0] as u32, layer.image.size[1] as u32];
        let size = wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 };
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glass-wall"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFF_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            layer.image.as_raw(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            size,
        );
        let view = tex.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.wall_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 9, resource: self.wall_ubuf.as_entire_binding() },
            ],
        });
        self.wall = Some(WallTex { id: layer.id, bind });
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let tex = |label: &str, w: u32, h: u32| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: OFF_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let (sw, sh) = ((size[0] / DOWNSCALE).max(1), (size[1] / DOWNSCALE).max(1));
        let full = tex("glass-full", size[0], size[1]);
        let content = tex("glass-content", (size[0] / 2).max(1), (size[1] / 2).max(1));
        let a = tex("glass-a", sw, sh);
        let b = tex("glass-b", sw, sh);
        let c = tex("glass-c", sw, sh);
        queue.write_buffer(&self.blur_h, 0, bytemuck::bytes_of(&BlurU { dir: [1.0 / sw as f32, 0.0], _pad: [0.0; 2] }));
        queue.write_buffer(&self.blur_v, 0, bytemuck::bytes_of(&BlurU { dir: [0.0, 1.0 / sh as f32], _pad: [0.0; 2] }));

        let tex_bind = |view: &wgpu::TextureView, u: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.tex_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 3, resource: u.as_entire_binding() },
                ],
            })
        };
        // A panel samples a blurred texture, and at its clear rim the sharp one.
        let panel_bind = |blurred: &wgpu::TextureView, sharp: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.panel_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(blurred) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.panel_ubuf,
                            offset: 0,
                            size: NonZeroU64::new(size_of::<PanelU>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(sharp) },
                    wgpu::BindGroupEntry { binding: 11, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            })
        };
        let targets = Targets {
            size,
            bg_bind: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.bg_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.bg_ubuf.as_entire_binding() }],
            }),
            full_bind: tex_bind(&full, &self.blur_h),
            a_h_bind: tex_bind(&a, &self.blur_h),
            b_v_bind: tex_bind(&b, &self.blur_v),
            panel_bind: panel_bind(&a, &full),
            content_bind: tex_bind(&content, &self.blur_h),
            c_h_bind: tex_bind(&c, &self.blur_h),
            panel_content_bind: panel_bind(&c, &content),
            full,
            content,
            a,
            b,
            c,
        };
        self.targets = Some(targets);
    }
}

impl GlassGpu {
    /// Paints the content mesh over a half-size copy of the background and blurs it into `c`.
    fn paint_content(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, screen: &ScreenDescriptor, enc: &mut wgpu::CommandEncoder, layer: &content::Layer) -> bool {
        let vb_size = std::mem::size_of_val(layer.verts.as_slice()) as u64;
        let ib_size = std::mem::size_of_val(layer.indices.as_slice()) as u64;
        if self.content_bufs.as_ref().is_none_or(|(v, i)| v.size() < vb_size || i.size() < ib_size) {
            let buf = |label: &str, size: u64, usage| {
                device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.next_power_of_two().max(4096), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
            };
            self.content_bufs = Some((buf("glass-content-v", vb_size, wgpu::BufferUsages::VERTEX), buf("glass-content-i", ib_size, wgpu::BufferUsages::INDEX)));
        }
        let (Some(t), Some((vb, ib))) = (&self.targets, &self.content_bufs) else { return false };
        queue.write_buffer(vb, 0, bytemuck::cast_slice(&layer.verts));
        queue.write_buffer(ib, 0, bytemuck::cast_slice(&layer.indices));
        let [w, h] = screen.size_in_pixels;
        let ppp = screen.pixels_per_point;
        queue.write_buffer(&self.content_ubuf, 0, bytemuck::bytes_of(&ContentU { screen: [w as f32 / ppp, h as f32 / ppp], _pad: [0.0; 2] }));
        fullscreen_pass(enc, &t.content, &self.copy_pipe, &t.full_bind);
        // Scissors are in the half-size target's pixels.
        let (cw, ch, k) = ((w / 2).max(1), (h / 2).max(1), ppp / 2.0);
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass-content"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.content,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            rp.set_pipeline(&self.content_pipe);
            rp.set_bind_group(0, &self.content_bind, &[]);
            rp.set_vertex_buffer(0, vb.slice(..vb_size));
            rp.set_index_buffer(ib.slice(..ib_size), wgpu::IndexFormat::Uint32);
            for d in &layer.draws {
                let x0 = (d.clip.min.x * k).floor().clamp(0.0, cw as f32) as u32;
                let y0 = (d.clip.min.y * k).floor().clamp(0.0, ch as f32) as u32;
                let x1 = (d.clip.max.x * k).ceil().clamp(0.0, cw as f32) as u32;
                let y1 = (d.clip.max.y * k).ceil().clamp(0.0, ch as f32) as u32;
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                rp.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
                rp.draw_indexed(d.indices.clone(), 0, 0..1);
            }
        }
        fullscreen_pass(enc, &t.c, &self.copy_pipe, &t.content_bind);
        for _ in 0..2 {
            fullscreen_pass(enc, &t.b, &self.blur_pipe, &t.c_h_bind);
            fullscreen_pass(enc, &t.c, &self.blur_pipe, &t.b_v_bind);
        }
        true
    }
}

fn fullscreen_pass(enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, pipe: &wgpu::RenderPipeline, bind: &wgpu::BindGroup) {
    let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("glass-pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
        })],
        ..Default::default()
    });
    rp.set_pipeline(pipe);
    rp.set_bind_group(0, bind, &[]);
    rp.draw(0..3, 0..1);
}

struct BgCallback {
    time: f32,
    light: bool,
}

impl CallbackTrait for BgCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        enc: &mut wgpu::CommandEncoder,
        res: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(gpu) = res.get_mut::<GlassGpu>() else { return Vec::new() };
        gpu.panel_count = 0;
        gpu.ensure_targets(device, queue, screen.size_in_pixels);
        let size = screen.size_in_pixels;
        queue.write_buffer(&gpu.bg_ubuf, 0, bytemuck::bytes_of(&BgU { size: [size[0] as f32, size[1] as f32], time: self.time, light: if self.light { 1.0 } else { 0.0 } }));
        let layer = WALL.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(layer) = &layer {
            gpu.ensure_wall(device, queue, layer);
            let (r, ppp) = (layer.rect, screen.pixels_per_point);
            let u = WallU { rect: [r.min.x * ppp, r.min.y * ppp, r.max.x * ppp, r.max.y * ppp], screen: [size[0] as f32, size[1] as f32], radius: layer.radius * ppp, _pad: 0.0, veil: layer.veil.to_srgba_unmultiplied().map(|c| c as f32 / 255.0) };
            queue.write_buffer(&gpu.wall_ubuf, 0, bytemuck::bytes_of(&u));
        }
        let Some(t) = gpu.targets.as_ref() else { return Vec::new() };
        fullscreen_pass(enc, &t.full, &gpu.bg_pipe, &t.bg_bind);
        if let (Some(_), Some(w)) = (&layer, &gpu.wall) {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass-wall"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.full,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            rp.set_pipeline(&gpu.wall_pipe);
            rp.set_bind_group(0, &w.bind, &[]);
            rp.draw(0..6, 0..1);
        }
        fullscreen_pass(enc, &t.a, &gpu.copy_pipe, &t.full_bind);
        for _ in 0..2 {
            fullscreen_pass(enc, &t.b, &gpu.blur_pipe, &t.a_h_bind);
            fullscreen_pass(enc, &t.a, &gpu.blur_pipe, &t.b_v_bind);
        }
        let layer = content::LAYER.lock().unwrap_or_else(|e| e.into_inner()).take();
        let ready = layer.is_some_and(|l| gpu.paint_content(device, queue, screen, enc, &l));
        gpu.content_ready = ready;
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, rp: &mut wgpu::RenderPass<'static>, res: &CallbackResources) {
        let Some(gpu) = res.get::<GlassGpu>() else { return };
        let Some(t) = &gpu.targets else { return };
        let [w, h] = info.screen_size_px;
        rp.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
        rp.set_pipeline(&gpu.screen_pipe);
        rp.set_bind_group(0, &t.full_bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

struct PanelCallback {
    rect: Rect,
    radius: f32,
    opacity: f32,
    refract: f32,
    /// Painted over the chat content, so it samples the content's blur.
    content: bool,
    idx: AtomicU32,
}

impl CallbackTrait for PanelCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        _enc: &mut wgpu::CommandEncoder,
        res: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(gpu) = res.get_mut::<GlassGpu>() else { return Vec::new() };
        if gpu.panel_count >= MAX_PANELS {
            return Vec::new();
        }
        let idx = gpu.panel_count;
        gpu.panel_count += 1;
        let ppp = screen.pixels_per_point;
        let u = PanelU {
            rect: [self.rect.min.x * ppp, self.rect.min.y * ppp, self.rect.max.x * ppp, self.rect.max.y * ppp],
            screen: [screen.size_in_pixels[0] as f32, screen.size_in_pixels[1] as f32],
            radius: self.radius * ppp,
            opacity: self.opacity,
            refract: self.refract,
            ppp,
            _pad: [0.0; 2],
        };
        queue.write_buffer(&gpu.panel_ubuf, idx as u64 * PANEL_STRIDE, bytemuck::bytes_of(&u));
        self.idx.store(idx, Ordering::Relaxed);
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, rp: &mut wgpu::RenderPass<'static>, res: &CallbackResources) {
        let idx = self.idx.load(Ordering::Relaxed);
        let Some(gpu) = res.get::<GlassGpu>() else { return };
        let Some(t) = &gpu.targets else { return };
        if idx == u32::MAX {
            return;
        }
        let [w, h] = info.screen_size_px;
        rp.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
        let bind = if self.content && gpu.content_ready { &t.panel_content_bind } else { &t.panel_bind };
        rp.set_pipeline(&gpu.panel_pipe);
        rp.set_bind_group(0, bind, &[(idx as u64 * PANEL_STRIDE) as u32]);
        rp.draw(0..6, 0..1);
    }
}
