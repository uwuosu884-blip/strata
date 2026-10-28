//! Photoshop-style "difference" drawing: shapes invert whatever is under them
//! (Live's playhead). egui has no such blend mode, so this is a tiny glow paint
//! callback: a flat-white shader with blend (1 - dst) * 1.

use eframe::egui::{self, Pos2, Rect};
use eframe::egui_glow::{self, glow};
use glow::HasContext;
use std::sync::{Arc, Mutex};

struct Gl {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
}

/// GL objects, created lazily on the first callback (single window app).
static GL: Mutex<Option<Result<Gl, ()>>> = Mutex::new(None);

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
        let cb = egui_glow::CallbackFn::new(move |info, p| draw(&info, p.gl(), &self));
        painter.add(egui::PaintCallback { rect, callback: Arc::new(cb) });
    }
}

fn compile(gl: &glow::Context) -> Result<Gl, ()> {
    let v = egui_glow::ShaderVersion::get(gl);
    let (attr, outd, outw) =
        if v.is_new_shader_interface() { ("in", "out vec4 o_col;", "o_col") } else { ("attribute", "", "gl_FragColor") };
    let vs = format!("{}{attr} vec2 a_pos;\nvoid main() {{ gl_Position = vec4(a_pos, 0.0, 1.0); }}\n", v.version_declaration());
    let fs = format!(
        "{}#ifdef GL_ES\nprecision mediump float;\n#endif\n{outd}\nvoid main() {{ {outw} = vec4(1.0); }}\n",
        v.version_declaration()
    );
    unsafe {
        let program = gl.create_program().map_err(|_| ())?;
        let mut shaders = Vec::new();
        for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
            let s = gl.create_shader(kind).map_err(|_| ())?;
            gl.shader_source(s, &src);
            gl.compile_shader(s);
            if !gl.get_shader_compile_status(s) {
                eprintln!("invert shader: {}", gl.get_shader_info_log(s));
                return Err(());
            }
            gl.attach_shader(program, s);
            shaders.push(s);
        }
        gl.bind_attrib_location(program, 0, "a_pos");
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            eprintln!("invert program: {}", gl.get_program_info_log(program));
            return Err(());
        }
        for s in shaders {
            gl.detach_shader(program, s);
            gl.delete_shader(s);
        }
        let vao = gl.create_vertex_array().map_err(|_| ())?;
        let vbo = gl.create_buffer().map_err(|_| ())?;
        Ok(Gl { program, vao, vbo })
    }
}

fn draw(info: &egui::PaintCallbackInfo, gl: &glow::Context, s: &Inverted) {
    let Ok(mut guard) = GL.lock() else { return };
    let Ok(g) = guard.get_or_insert_with(|| compile(gl)) else { return };
    // Points → NDC of the GL viewport egui set for this callback (whole pixels).
    let vp = info.viewport_in_pixels();
    let ppp = info.pixels_per_point;
    let (w, h) = (vp.width_px.max(1) as f32, vp.height_px.max(1) as f32);
    let ndc =
        |p: Pos2| [((p.x * ppp).round() - vp.left_px as f32) / w * 2.0 - 1.0, 1.0 - ((p.y * ppp).round() - vp.top_px as f32) / h * 2.0];
    let mut v: Vec<f32> = Vec::with_capacity(s.rects.len() * 12 + s.tris.len() * 6);
    for r in &s.rects {
        let (a, b, c, d) = (ndc(r.left_top()), ndc(r.right_top()), ndc(r.right_bottom()), ndc(r.left_bottom()));
        for q in [a, b, c, a, c, d] {
            v.extend_from_slice(&q);
        }
    }
    for t in &s.tris {
        for &p in t {
            // Triangles stay unrounded so their slopes look smooth.
            v.extend_from_slice(&[(p.x * ppp - vp.left_px as f32) / w * 2.0 - 1.0, 1.0 - (p.y * ppp - vp.top_px as f32) / h * 2.0]);
        }
    }
    unsafe {
        gl.use_program(Some(g.program));
        gl.bind_vertex_array(Some(g.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(g.vbo));
        let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4);
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STREAM_DRAW);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
        gl.enable(glow::BLEND);
        gl.blend_equation(glow::FUNC_ADD);
        // rgb = 1·(1 − dst); alpha untouched.
        gl.blend_func_separate(glow::ONE_MINUS_DST_COLOR, glow::ZERO, glow::ZERO, glow::ONE);
        gl.draw_arrays(glow::TRIANGLES, 0, (v.len() / 2) as i32);
        gl.disable_vertex_attrib_array(0);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        gl.bind_vertex_array(None);
        gl.use_program(None);
    }
}
