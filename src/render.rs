//! Software rasteriser for `--png`: a `Scene` to RGBA pixels, no GPU.
//!
//! The Linux thumbnailer runs where there may be no display server at all,
//! which is why this exists instead of offscreen GL (`plan.md` §2, the one
//! idea taken from `cadrum`). It is deliberately small: an orthographic
//! isometric view, a z-buffer, two-sided Lambert shading, 2× supersampling.
//!
//! It also owns the **broken-face overlay** (`plan.md` §5 "S1 follow-up"):
//! a face the kernel only approximated is striped amber; a missing face's
//! outline is drawn red on top of everything, so the hole is visible; and a
//! warning badge marks the image. Construction curves beside solids are not
//! drawn unless asked for.

use crate::{Color, FaceStatus, LineKind, Scene};

/// Rendering options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Output edge length in pixels (square).
    pub size: u32,
    /// Draw `LineKind::Construction` curves.
    pub show_construction: bool,
    /// Supersampling factor per axis (1 = none).
    pub supersample: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            size: 512,
            show_construction: false,
            supersample: 2,
        }
    }
}

/// An RGBA8 image, straight (non-premultiplied) alpha, sRGB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// The pixel at `(x, y)` as `[r, g, b, a]`.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    /// Encodes as PNG.
    ///
    /// # Errors
    /// Only if the PNG encoder fails, which for an in-memory buffer means a bug.
    pub fn to_png(&self) -> Result<Vec<u8>, png::EncodingError> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            let mut w = enc.write_header()?;
            w.write_image_data(&self.rgba)?;
        }
        Ok(out)
    }
}

/// Nothing to draw: no triangles and no visible lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyScene;

impl std::fmt::Display for EmptyScene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "scene has nothing to draw")
    }
}

impl std::error::Error for EmptyScene {}

const DEFAULT_COLOR: Color = Color {
    r: 0.38,
    g: 0.43,
    b: 0.50,
};
const APPROX: [f32; 3] = [0.96, 0.62, 0.12];
const APPROX_DARK: [f32; 3] = [0.45, 0.27, 0.02];
const MISSING: [f32; 3] = [0.85, 0.05, 0.10];
const SKETCH: [f32; 3] = [0.12, 0.13, 0.15];
const CONSTRUCTION: [f32; 3] = [0.35, 0.45, 0.60];

/// View rotation: azimuth −35° about Y, then elevation 30° about X.
fn view(p: [f32; 3]) -> [f32; 3] {
    let (sa, ca) = (-35f32).to_radians().sin_cos();
    let (se, ce) = 30f32.to_radians().sin_cos();
    // The scene is Z-up (CAD convention); swap to Y-up for the view.
    let (x, y, z) = (p[0], p[2], -p[1]);
    let (x, z) = (x * ca + z * sa, -x * sa + z * ca);
    let (y, z) = (y * ce - z * se, y * se + z * ce);
    [x, y, z]
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

struct Target {
    w: usize,
    h: usize,
    color: Vec<[f32; 3]>,
    alpha: Vec<f32>,
    depth: Vec<f32>,
}

impl Target {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            color: vec![[0.0; 3]; w * h],
            alpha: vec![0.0; w * h],
            depth: vec![f32::INFINITY; w * h],
        }
    }

    fn put(&mut self, x: usize, y: usize, z: f32, c: [f32; 3], depth_test: bool) {
        let i = y * self.w + x;
        if !depth_test || z < self.depth[i] {
            if depth_test {
                self.depth[i] = z;
            }
            self.color[i] = c;
            self.alpha[i] = 1.0;
        }
    }
}

/// Renders `scene`.
///
/// # Errors
/// [`EmptyScene`] when there are no triangles and no lines to draw.
pub fn render(scene: &Scene, opts: &Options) -> Result<Image, EmptyScene> {
    let ss = opts.supersample.max(1) as usize;
    let n = opts.size.max(8) as usize * ss;
    let visible = |k: LineKind| k != LineKind::Construction || opts.show_construction;

    // Fit: projected extents of everything that will be drawn.
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    let mut grow = |p: [f32; 3]| {
        let v = view(p);
        for k in 0..2 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    };
    let mut drawn = 0usize;
    for part in &scene.parts {
        for i in &part.mesh.indices {
            let i = *i as usize * 3;
            grow([
                part.mesh.positions[i],
                part.mesh.positions[i + 1],
                part.mesh.positions[i + 2],
            ]);
        }
        drawn += part.mesh.triangle_count();
        for (s, k) in part.lines.kinds.iter().enumerate() {
            if visible(*k) {
                let q = &part.lines.positions[s * 6..s * 6 + 6];
                grow([q[0], q[1], q[2]]);
                grow([q[3], q[4], q[5]]);
                drawn += 1;
            }
        }
    }
    if drawn == 0 || !lo[0].is_finite() {
        return Err(EmptyScene);
    }
    let margin = 0.06 * n as f32;
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(f32::EPSILON);
    let scale = (n as f32 - 2.0 * margin) / span;
    // Centre the drawing in the square.
    let off = [
        (n as f32 - (hi[0] - lo[0]) * scale) / 2.0,
        (n as f32 - (hi[1] - lo[1]) * scale) / 2.0,
    ];
    let screen = |p: [f32; 3]| -> [f32; 3] {
        let v = view(p);
        [
            off[0] + (v[0] - lo[0]) * scale,
            n as f32 - (off[1] + (v[1] - lo[1]) * scale),
            -v[2], // nearer = smaller
        ]
    };

    let mut t = Target::new(n, n);
    let key = normalize([0.35, 0.75, 0.55]);
    let fill = normalize([-0.6, 0.2, 0.4]);
    let stripe = (6 * ss) as i64;
    let mut badge = false;

    for part in &scene.parts {
        let m = &part.mesh;
        let pts: Vec<[f32; 3]> = m
            .positions
            .chunks_exact(3)
            .map(|p| screen([p[0], p[1], p[2]]))
            .collect();
        for (tri, &fid) in m.indices.chunks_exact(3).zip(&m.face_ids) {
            let status = part
                .faces
                .get(fid as usize)
                .map_or(FaceStatus::Ok, |f| f.status);
            let approx = status == FaceStatus::Approx;
            badge |= approx;
            let base = part.face_color(fid).unwrap_or(DEFAULT_COLOR);
            let (a, b, c) = (
                pts[tri[0] as usize],
                pts[tri[1] as usize],
                pts[tri[2] as usize],
            );
            // Flat shading from the view-space face normal, two-sided: CAD
            // exports do not reliably orient faces, and a black back face
            // reads as "broken model".
            let wa = view(vtx(m, tri[0]));
            let nrm = normalize(cross(
                sub(view(vtx(m, tri[1])), wa),
                sub(view(vtx(m, tri[2])), wa),
            ));
            let light = 0.22 + 0.62 * dot(nrm, key).abs() + 0.16 * dot(nrm, fill).abs();
            let shaded = [base.r * light, base.g * light, base.b * light];
            raster_triangle(&mut t, a, b, c, |x, y| {
                if approx {
                    let band = (x as i64 + y as i64).div_euclid(stripe) % 2 == 0;
                    let s = if band { APPROX } else { APPROX_DARK };
                    lerp3(shaded, s, 0.75)
                } else {
                    shaded
                }
            });
        }
    }

    // Lines: sketch/construction depth-tested; missing outlines on top.
    let width = (1.6 * ss as f32).max(1.0);
    for pass in [false, true] {
        for part in &scene.parts {
            for (s, k) in part.lines.kinds.iter().enumerate() {
                let on_top = *k == LineKind::MissingOutline;
                if on_top != pass || !visible(*k) {
                    continue;
                }
                badge |= on_top;
                let q = &part.lines.positions[s * 6..s * 6 + 6];
                let c = match k {
                    LineKind::Sketch => SKETCH,
                    LineKind::MissingOutline => MISSING,
                    LineKind::Construction => CONSTRUCTION,
                };
                let a = screen([q[0], q[1], q[2]]);
                let b = screen([q[3], q[4], q[5]]);
                raster_line(
                    &mut t,
                    a,
                    b,
                    width * if on_top { 1.4 } else { 1.0 },
                    c,
                    !on_top,
                );
            }
        }
    }

    if badge {
        draw_badge(&mut t, ss);
    }
    Ok(downsample(&t, ss))
}

fn vtx(m: &crate::Mesh, i: u32) -> [f32; 3] {
    let i = i as usize * 3;
    [m.positions[i], m.positions[i + 1], m.positions[i + 2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l > 0.0 {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        [0.0, 0.0, 1.0]
    }
}

fn raster_triangle(
    t: &mut Target,
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    shade: impl Fn(usize, usize) -> [f32; 3],
) {
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area.abs() < 1e-12 || !area.is_finite() {
        return;
    }
    let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
    let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
    let x1 = (a[0].max(b[0]).max(c[0]).ceil() as usize).min(t.w.saturating_sub(1));
    let y1 = (a[1].max(b[1]).max(c[1]).ceil() as usize).min(t.h.saturating_sub(1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
            let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
            let i = y * t.w + x;
            if z < t.depth[i] {
                t.put(x, y, z, shade(x, y), true);
            }
        }
    }
}

fn raster_line(
    t: &mut Target,
    a: [f32; 3],
    b: [f32; 3],
    width: f32,
    c: [f32; 3],
    depth_test: bool,
) {
    let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    let steps = (len.ceil() as usize).max(1);
    let r = width / 2.0;
    let ri = r.ceil() as i64;
    for s in 0..=steps {
        let f = s as f32 / steps as f32;
        let p = lerp3(a, b, f);
        // A small depth bias so lines lying on a face win against it.
        let z = p[2] - 1e-3 * t.w as f32;
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                if (dx * dx + dy * dy) as f32 > r * r + 0.5 {
                    continue;
                }
                let (x, y) = (p[0] as i64 + dx, p[1] as i64 + dy);
                if x >= 0 && y >= 0 && (x as usize) < t.w && (y as usize) < t.h {
                    t.put(x as usize, y as usize, z, c, depth_test);
                }
            }
        }
    }
}

/// A warning triangle in the top-left corner: amber, dark "!".
fn draw_badge(t: &mut Target, ss: usize) {
    let s = (t.w as f32 * 0.09).max(12.0 * ss as f32);
    let (ox, oy) = (s * 0.35, s * 0.35);
    let apex = [ox + s / 2.0, oy, 0.0];
    let left = [ox, oy + s * 0.88, 0.0];
    let right = [ox + s, oy + s * 0.88, 0.0];
    let border = 0.10 * s;
    // Dark rim, then the amber face, then the mark: painted, not depth-tested.
    paint_triangle(
        t,
        [apex[0], apex[1] - border, 0.0],
        [left[0] - border, left[1] + border * 0.6, 0.0],
        [right[0] + border, right[1] + border * 0.6, 0.0],
        APPROX_DARK,
    );
    paint_triangle(t, apex, left, right, APPROX);
    let cx = ox + s / 2.0;
    let w = s * 0.07;
    paint_rect(t, cx - w, oy + s * 0.30, cx + w, oy + s * 0.62, APPROX_DARK);
    paint_rect(t, cx - w, oy + s * 0.68, cx + w, oy + s * 0.78, APPROX_DARK);
}

fn paint_triangle(t: &mut Target, a: [f32; 3], b: [f32; 3], c: [f32; 3], col: [f32; 3]) {
    // Depth -inf so the badge always lands over the model.
    let lift = |p: [f32; 3]| [p[0], p[1], f32::NEG_INFINITY];
    let (a, b, c) = (lift(a), lift(b), lift(c));
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area.abs() < 1e-12 {
        return;
    }
    let x0 = a[0].min(b[0]).min(c[0]).max(0.0) as usize;
    let y0 = a[1].min(b[1]).min(c[1]).max(0.0) as usize;
    let x1 = (a[0].max(b[0]).max(c[0]).ceil() as usize).min(t.w - 1);
    let y1 = (a[1].max(b[1]).max(c[1]).ceil() as usize).min(t.h - 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
            let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
            if w0 >= 0.0 && w1 >= 0.0 && w0 + w1 <= 1.0 {
                t.put(x, y, 0.0, col, false);
            }
        }
    }
}

fn paint_rect(t: &mut Target, x0: f32, y0: f32, x1: f32, y1: f32, col: [f32; 3]) {
    for y in (y0.max(0.0) as usize)..(y1.min(t.h as f32) as usize) {
        for x in (x0.max(0.0) as usize)..(x1.min(t.w as f32) as usize) {
            t.put(x, y, 0.0, col, false);
        }
    }
}

fn downsample(t: &Target, ss: usize) -> Image {
    let (w, h) = (t.w / ss, t.h / ss);
    let mut rgba = Vec::with_capacity(w * h * 4);
    let k = (ss * ss) as f32;
    for y in 0..h {
        for x in 0..w {
            let (mut c, mut a) = ([0.0f32; 3], 0.0f32);
            for sy in 0..ss {
                for sx in 0..ss {
                    let i = (y * ss + sy) * t.w + x * ss + sx;
                    let al = t.alpha[i];
                    a += al;
                    for (acc, v) in c.iter_mut().zip(t.color[i]) {
                        *acc += v * al;
                    }
                }
            }
            if a > 0.0 {
                // Average in linear light, un-premultiply, then encode sRGB.
                rgba.extend([srgb(c[0] / a), srgb(c[1] / a), srgb(c[2] / a)]);
            } else {
                rgba.extend([0, 0, 0]);
            }
            rgba.push((a / k * 255.0 + 0.5) as u8);
        }
    }
    Image {
        width: w as u32,
        height: h as u32,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BBox, Face, Lines, Mesh, Part};

    /// A unit cube: 12 triangles, 6 faces. `face_status` per face.
    fn cube(status: [FaceStatus; 6]) -> Scene {
        let corners = [
            [0., 0., 0.],
            [1., 0., 0.],
            [1., 1., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            [1., 0., 1.],
            [1., 1., 1.],
            [0., 1., 1.],
        ];
        let quads = [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [2, 3, 7, 6],
            [1, 2, 6, 5],
            [3, 0, 4, 7],
        ];
        let mut m = Mesh::default();
        for (f, q) in quads.iter().enumerate() {
            let base = (m.positions.len() / 3) as u32;
            for &c in q {
                m.positions.extend(corners[c].map(|v: f64| v as f32));
                m.normals.extend([0.0, 0.0, 1.0]);
            }
            m.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            m.face_ids.extend([f as u32, f as u32]);
        }
        Scene {
            bbox: BBox {
                min: [0.0; 3],
                max: [1.0; 3],
            },
            parts: vec![Part {
                name: None,
                color: Some(Color {
                    r: 0.2,
                    g: 0.4,
                    b: 0.8,
                }),
                mesh: m,
                faces: status.iter().map(|&s| Face::plain(s)).collect(),
                lines: Lines::default(),
            }],
        }
    }

    fn opts() -> Options {
        Options {
            size: 64,
            ..Options::default()
        }
    }

    #[test]
    fn cube_fills_the_centre_and_leaves_corners_transparent() {
        let img = render(&cube([FaceStatus::Ok; 6]), &opts()).unwrap();
        assert_eq!((img.width, img.height), (64, 64));
        assert_eq!(img.pixel(32, 32)[3], 255, "centre is covered");
        assert_eq!(img.pixel(0, 0)[3], 0, "corner is transparent");
        assert_eq!(img.pixel(63, 63)[3], 0);
        // Blue part colour dominates the shaded pixel.
        let [r, g, b, _] = img.pixel(32, 32);
        assert!(b > r && b > g);
    }

    #[test]
    fn rendering_is_deterministic() {
        let s = cube([FaceStatus::Ok; 6]);
        assert_eq!(render(&s, &opts()).unwrap(), render(&s, &opts()).unwrap());
    }

    #[test]
    fn approximated_faces_get_the_overlay_and_a_badge() {
        let ok = render(&cube([FaceStatus::Ok; 6]), &opts()).unwrap();
        let approx = render(&cube([FaceStatus::Approx; 6]), &opts()).unwrap();
        let amberish = |p: [u8; 4]| {
            let [r, g, b, a] = p.map(i32::from);
            a > 0 && r > b + 40 && g > b
        };
        let count = |img: &Image| {
            (0..64)
                .flat_map(|y| (0..64).map(move |x| (x, y)))
                .filter(|&(x, y)| amberish(img.pixel(x, y)))
                .count()
        };
        assert_eq!(count(&ok), 0, "no amber without approximation");
        assert!(count(&approx) > 200, "approximated faces are amber");
        // Badge in the top-left corner, which is otherwise empty.
        let corner = |img: &Image| {
            (0..14)
                .flat_map(|y| (0..14).map(move |x| (x, y)))
                .filter(|&(x, y)| amberish(img.pixel(x, y)))
                .count()
        };
        assert_eq!(corner(&ok), 0);
        assert!(corner(&approx) > 10, "badge drawn");
    }

    #[test]
    fn sketch_only_scene_draws_lines() {
        let mut s = cube([FaceStatus::Ok; 6]);
        s.parts[0].mesh = Mesh::default();
        s.parts[0].faces.clear();
        s.parts[0].lines = Lines {
            positions: vec![0.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            kinds: vec![LineKind::Sketch],
        };
        let img = render(&s, &opts()).unwrap();
        let covered = img.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(covered > 20);
    }

    #[test]
    fn construction_only_is_hidden_and_reports_empty() {
        let mut s = cube([FaceStatus::Ok; 6]);
        s.parts[0].mesh = Mesh::default();
        s.parts[0].lines = Lines {
            positions: vec![0.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            kinds: vec![LineKind::Construction],
        };
        assert_eq!(render(&s, &opts()), Err(EmptyScene));
        let shown = Options {
            show_construction: true,
            ..opts()
        };
        assert!(render(&s, &shown).is_ok());
    }

    #[test]
    fn png_round_trips() {
        let img = render(&cube([FaceStatus::Ok; 6]), &opts()).unwrap();
        let png = img.to_png().unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let dec = png::Decoder::new(std::io::Cursor::new(png));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (64, 64));
        assert_eq!(&buf[..info.buffer_size()], &img.rgba[..]);
    }
}
