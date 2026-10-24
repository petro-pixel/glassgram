// “Liquid glass” shaders: procedural background, Gaussian blur, glass panels with refraction.
// All colours are in gamma space (egui renders to a non-sRGB surface).

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Fullscreen triangle; uv is in texture coordinates (v=0 at the top).
@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

// ───────────── Background ─────────────

struct BgU {
    size: vec2<f32>,
    time: f32,
    light: f32,
};
@group(0) @binding(0) var<uniform> bg: BgU;

fn blob(uv: vec2<f32>, aspect: f32, c: vec2<f32>, r: f32) -> f32 {
    let d = (uv - c) * vec2<f32>(aspect / 1.15, 1.0);
    return exp(-dot(d, d) / (r * r) * 2.2);
}

@fragment
fn fs_bg(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let aspect = bg.size.x / bg.size.y;
    let t = bg.time;
    let dark = mix(vec3<f32>(0.063, 0.071, 0.173), vec3<f32>(0.024, 0.031, 0.086), uv.y);
    let light = mix(vec3<f32>(0.933, 0.945, 1.0), vec3<f32>(0.84, 0.866, 0.973), uv.y);
    var col = mix(dark, light, bg.light);
    // On a light background the blobs are more pastel.
    let strength = mix(0.6, 0.38, bg.light);

    let cols = array<vec3<f32>, 5>(
        vec3<f32>(0.47, 0.31, 1.0), vec3<f32>(0.0, 0.75, 1.0), vec3<f32>(1.0, 0.27, 0.63),
        vec3<f32>(1.0, 0.59, 0.24), vec3<f32>(0.16, 0.9, 0.7));
    let sizes = array<f32, 5>(0.62, 0.55, 0.48, 0.36, 0.40);
    let speeds = array<f32, 5>(0.07, 0.05, 0.06, 0.08, 0.045);
    let phases = array<f32, 5>(0.0, 2.1, 4.0, 5.3, 1.2);
    let s = min(1.0, 1.0 / aspect);

    var field = 0.0;
    for (var i = 0; i < 5; i++) {
        let c = vec2<f32>(0.5 + sin(t * speeds[i] + phases[i]) * 0.38,
                          0.5 + cos(t * speeds[i] * 0.8 + phases[i] * 1.7) * 0.34);
        let r = sizes[i] * s * (1.0 + 0.08 * sin(t * speeds[i] * 3.0 + phases[i]));
        let k = blob(uv, aspect, c, r);
        field += k;
        col = mix(col, cols[i], k * strength);
    }

    // Thin “topographic” lines: fine detail that makes blur and refraction visible.
    let f = field * 7.0 + t * 0.05;
    let line = 1.0 - smoothstep(0.0, 0.06, abs(fract(f) - 0.5));
    col += line * mix(0.10, -0.05, bg.light) * (0.4 + field);
    return vec4<f32>(col, 1.0);
}

// ───────────── Copy and blur ─────────────

@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var src_smp: sampler;

struct BlurU {
    dir: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(3) var<uniform> blur: BlurU;

@fragment
fn fs_copy(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(src_tex, src_smp, in.uv);
}

@fragment
fn fs_blur(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let w = array<f32, 5>(0.2270270, 0.1945946, 0.1216216, 0.0540540, 0.0162162);
    var acc = textureSample(src_tex, src_smp, uv).rgb * w[0];
    for (var i = 1; i < 5; i++) {
        let o = blur.dir * f32(i) * 1.6;
        acc += textureSample(src_tex, src_smp, uv + o).rgb * w[i];
        acc += textureSample(src_tex, src_smp, uv - o).rgb * w[i];
    }
    return vec4<f32>(acc, 1.0);
}

// ───────────── Glass panel ─────────────

struct PanelU {
    rect: vec4<f32>,      // min.x, min.y, max.x, max.y in pixels
    screen: vec2<f32>,
    radius: f32,
    opacity: f32,
    refract: f32,
    ppp: f32,
    _pad: vec2<f32>,
};
@group(0) @binding(4) var blur_tex: texture_2d<f32>;
@group(0) @binding(5) var blur_smp: sampler;
@group(0) @binding(6) var<uniform> panel: PanelU;

@vertex
fn vs_panel(@builtin(vertex_index) i: u32) -> VsOut {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
    let c = corners[i];
    let px = mix(panel.rect.xy - 1.0, panel.rect.zw + 1.0, c);
    var o: VsOut;
    o.pos = vec4<f32>(px.x / panel.screen.x * 2.0 - 1.0, 1.0 - px.y / panel.screen.y * 2.0, 0.0, 1.0);
    o.uv = c;
    return o;
}

@fragment
fn fs_panel(in: VsOut) -> @location(0) vec4<f32> {
    let p = in.pos.xy;
    let c = (panel.rect.xy + panel.rect.zw) * 0.5;
    let b = (panel.rect.zw - panel.rect.xy) * 0.5;
    let r = min(panel.radius, min(b.x, b.y));
    let rel = p - c;
    let q = abs(rel) - b + r;
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let mask = clamp(0.5 - d, 0.0, 1.0);
    if (mask <= 0.0) {
        discard;
    }

    // Normal to the rounded rectangle's edge.
    var n: vec2<f32>;
    if (q.x > 0.0 && q.y > 0.0) {
        n = normalize(q) * sign(rel);
    } else if (q.x > q.y) {
        n = vec2<f32>(sign(rel.x), 0.0);
    } else {
        n = vec2<f32>(0.0, sign(rel.y));
    }

    // Lens at the edges: the closer to the border, the more the outside background is pulled in.
    let band = max(min(r * 1.2, 26.0 * panel.ppp), 1.0);
    let t = clamp(1.0 + d / band, 0.0, 1.0);
    let bend = t * t * panel.refract * panel.ppp;
    let base = p / panel.screen;
    let off = n * bend / panel.screen;
    let col = vec3<f32>(
        textureSample(blur_tex, blur_smp, base + off * 1.10).r,
        textureSample(blur_tex, blur_smp, base + off).g,
        textureSample(blur_tex, blur_smp, base + off * 0.90).b);

    // “Vibrancy”: a bit of saturation and light, like real glass.
    let lum = dot(col, vec3<f32>(0.299, 0.587, 0.114));
    var outc = mix(vec3<f32>(lum), col, 1.35) * 1.06 + t * t * t * 0.07;
    outc = clamp(outc, vec3<f32>(0.0), vec3<f32>(1.0));
    let a = mask * panel.opacity;
    return vec4<f32>(outc * a, a);
}
