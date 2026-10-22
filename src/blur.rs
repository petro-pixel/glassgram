//! Справжнє розмиття під склом (wgpu).
//!
//! Кадр: фон рендериться шейдером в офскрін-текстуру → зменшується в 4 рази →
//! двічі розмивається гаусом (H+V) → фон малюється на екран, а кожна скляна панель
//! бере розмиту текстуру зі зсувом на краях (заломлення) та легкою хроматичною аберацією.

use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU32, Ordering};

use bytemuck::{Pod, Zeroable};
use eframe::egui::{self, Rect};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};

const OFF_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DOWNSCALE: u32 = 4;
const PANEL_STRIDE: u64 = 256;
const MAX_PANELS: u32 = 512;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BgU {
    size: [f32; 2],
    time: f32,
    /// 1 — світла тема.
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

struct Targets {
    size: [u32; 2],
    full: wgpu::TextureView,
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
}

/// Реєструє GPU-ресурси в рендерері egui. Повертає `false`, якщо wgpu недоступний.
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
        PanelCallback { rect, radius, opacity, refract, idx: AtomicU32::new(u32::MAX) },
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
        let panel_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass-panel"),
            entries: &[p0, p1, uniform_entry(6, true, size_of::<PanelU>() as u64)],
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
        }
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
        let a = tex("glass-a", sw, sh);
        let b = tex("glass-b", sw, sh);
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
            panel_bind: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.panel_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&a) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.panel_ubuf,
                            offset: 0,
                            size: NonZeroU64::new(size_of::<PanelU>() as u64),
                        }),
                    },
                ],
            }),
            full,
            a,
            b,
        };
        self.targets = Some(targets);
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
        let t = gpu.targets.as_ref().unwrap();
        fullscreen_pass(enc, &t.full, &gpu.bg_pipe, &t.bg_bind);
        fullscreen_pass(enc, &t.a, &gpu.copy_pipe, &t.full_bind);
        for _ in 0..2 {
            fullscreen_pass(enc, &t.b, &gpu.blur_pipe, &t.a_h_bind);
            fullscreen_pass(enc, &t.a, &gpu.blur_pipe, &t.b_v_bind);
        }
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
        rp.set_pipeline(&gpu.panel_pipe);
        rp.set_bind_group(0, &t.panel_bind, &[(idx as u64 * PANEL_STRIDE) as u32]);
        rp.draw(0..6, 0..1);
    }
}
