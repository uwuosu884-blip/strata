//! Photoshop-style "difference" drawing: shapes invert whatever is under them
//! (Live's playhead). egui has no such blend mode, so this is a tiny wgpu paint
//! callback: a flat-white shader with blend (1 - dst) * 1.

use eframe::egui::{self, Pos2, Rect};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu::{self, util::DeviceExt};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// The pipeline plus this frame's vertex buffers (keyed by callback id).
struct Gpu {
    pipeline: wgpu::RenderPipeline,
    bufs: HashMap<u64, (wgpu::Buffer, u32)>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

const SHADER: &str = "
@vertex fn vs(@location(0) p: vec2<f32>) -> @builtin(position) vec4<f32> { return vec4<f32>(p, 0.0, 1.0); }
@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }
";

/// Creates the pipeline once, at startup (needs the surface format).
pub fn init(rs: &egui_wgpu::RenderState) {
    let d = &rs.device;
    let module =
        d.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("invert"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout =
        d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("invert"), bind_group_layouts: &[], immediate_size: 0 });
    let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("invert"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 8,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2],
            })],
            compilation_options: Default::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState {
                format: rs.target_format,
                // rgb = 1·(1 − dst); alpha untouched.
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::OneMinusDst,
                        dst_factor: wgpu::BlendFactor::Zero,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Zero,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    });
    rs.renderer.write().callback_resources.insert(Gpu { pipeline, bufs: HashMap::new() });
}

/// Shapes to invert: axis-aligned rects and triangles, in egui points.
#[derive(Default, Clone)]
pub struct Inverted {
    pub rects: Vec<Rect>,
    pub tris: Vec<[Pos2; 3]>,
}

impl Inverted {
    /// Vertical 1 px line through pixel column `x` (x on a pixel centre).
    pub fn vline(&mut self, x: f32, y0: f32, y1: f32) {
        if y1 > y0 {
            self.rects.push(Rect::from_min_max(egui::pos2(x - 0.5, y0), egui::pos2(x + 0.5, y1)));
        }
    }
    /// Triangle `w` px wide, `h` tall, its tip at `tip` pointing down (`down`) or up.
    pub fn tri(&mut self, tip: Pos2, w: f32, h: f32, down: bool) {
        let by = if down { tip.y - h } else { tip.y + h };
        self.tris.push([egui::pos2(tip.x - w * 0.5, by), egui::pos2(tip.x + w * 0.5, by), tip]);
    }
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty() && self.tris.is_empty()
    }

    /// Queue the shapes on `painter` (clipped to its clip rect).
    pub fn paint(self, painter: &egui::Painter) {
        if self.is_empty() {
            return;
        }
        let rect = painter.clip_rect();
        if !rect.is_positive() {
            return;
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        painter.add(egui_wgpu::Callback::new_paint_callback(rect, Cb { id, s: self }));
    }
}

struct Cb {
    id: u64,
    s: Inverted,
}

impl CallbackTrait for Cb {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        sd: &ScreenDescriptor,
        _enc: &mut wgpu::CommandEncoder,
        res: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(g) = res.get_mut::<Gpu>() else { return Vec::new() };
        // Points → NDC of the whole surface (paint() sets a full-surface viewport).
        let ppp = sd.pixels_per_point;
        let (w, h) = (sd.size_in_pixels[0].max(1) as f32, sd.size_in_pixels[1].max(1) as f32);
        let ndc = |x: f32, y: f32| [x / w * 2.0 - 1.0, 1.0 - y / h * 2.0];
        let s = &self.s;
        let mut v: Vec<f32> = Vec::with_capacity(s.rects.len() * 12 + s.tris.len() * 6);
        for r in &s.rects {
            // Rects snap to whole pixels.
            let (x0, y0, x1, y1) = ((r.min.x * ppp).round(), (r.min.y * ppp).round(), (r.max.x * ppp).round(), (r.max.y * ppp).round());
            let (a, b, c, d) = (ndc(x0, y0), ndc(x1, y0), ndc(x1, y1), ndc(x0, y1));
            for q in [a, b, c, a, c, d] {
                v.extend_from_slice(&q);
            }
        }
        for t in &s.tris {
            for &p in t {
                // Triangles stay unrounded so their slopes look smooth.
                v.extend_from_slice(&ndc(p.x * ppp, p.y * ppp));
            }
        }
        let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_ne_bytes()).collect();
        let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("invert"),
            contents: &bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        // Drop buffers of earlier frames.
        let id = self.id;
        g.bufs.retain(|&k, _| k + 64 > id);
        g.bufs.insert(id, (buf, (v.len() / 2) as u32));
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, rp: &mut wgpu::RenderPass<'static>, res: &CallbackResources) {
        let Some(g) = res.get::<Gpu>() else { return };
        let Some((buf, n)) = g.bufs.get(&self.id) else { return };
        let [w, h] = info.screen_size_px;
        rp.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
        rp.set_pipeline(&g.pipeline);
        rp.set_vertex_buffer(0, buf.slice(..));
        rp.draw(0..*n, 0..1);
    }
}
