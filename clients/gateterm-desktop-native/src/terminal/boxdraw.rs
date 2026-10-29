//! Pixel-aligned procedural geometry for Unicode Box-Drawing (`U+2500..=U+257F`)
//! and Block Elements (`U+2580..=U+259F`).
//!
//! Drawing these glyphs directly as device-pixel-snapped GPU primitives instead of
//! relying on font glyphs guarantees zero 1px seams across cells in TUI frames
//! (Claude Code, Codex, lazygit, tmux, btop) regardless of font or line-height.

use gpui::{Bounds, Hsla, Pixels, Window, fill, point, px, size};

pub enum BoxInk {
    Rect(Bounds<Pixels>),
    Shade(Bounds<Pixels>, f32),
    Path(gpui::Path<Pixels>),
}

#[inline]
pub fn is_box_or_block(c: char) -> bool {
    ('\u{2500}'..='\u{259f}').contains(&c)
}

pub fn paint_box_or_block(
    c: char,
    bounds: Bounds<Pixels>,
    color: Hsla,
    scale: f32,
    window: &mut Window,
) -> bool {
    let Some(inks) = glyph(c, bounds, scale) else {
        return false;
    };
    for ink in inks {
        match ink {
            BoxInk::Rect(rect) => {
                window.paint_quad(fill(rect, color));
            }
            BoxInk::Shade(rect, alpha_mul) => {
                let mut shaded = color;
                shaded.a *= alpha_mul;
                window.paint_quad(fill(rect, shaded));
            }
            BoxInk::Path(path) => {
                window.paint_path(path, color);
            }
        }
    }
    true
}

pub fn glyph(c: char, bounds: Bounds<Pixels>, scale: f32) -> Option<Vec<BoxInk>> {
    if !is_box_or_block(c) {
        return None;
    }
    let g = Cell::new(&bounds, scale);
    if let Some((u, d, l, r)) = arms_of(c) {
        return Some(g.arms(u, d, l, r));
    }
    g.doubles(c)
        .or_else(|| g.rounded(c))
        .or_else(|| g.dashed(c))
        .or_else(|| g.diagonal(c))
        .or_else(|| g.blocks(c))
}

#[derive(Clone, Copy, PartialEq)]
enum Arm {
    None,
    Light,
    Heavy,
}

struct Cell {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    cx: f32,
    cy: f32,
    t: f32,
    scale: f32,
}

fn light_thickness(cell_width: f32, scale: f32) -> f32 {
    let logical = (cell_width * 0.15).round().max(1.0);
    (logical * scale).round().max(1.0) / scale
}

impl Cell {
    fn new(b: &Bounds<Pixels>, scale: f32) -> Self {
        let scale = scale.max(0.1);
        let x0 = (b.origin.x.as_f32() * scale).round() / scale;
        let y0 = (b.origin.y.as_f32() * scale).round() / scale;
        let x1 = ((b.origin.x.as_f32() + b.size.width.as_f32()) * scale).round() / scale;
        let y1 = ((b.origin.y.as_f32() + b.size.height.as_f32()) * scale).round() / scale;
        Self {
            x0,
            y0,
            x1,
            y1,
            cx: (x0 + x1) * 0.5,
            cy: (y0 + y1) * 0.5,
            t: light_thickness(x1 - x0, scale),
            scale,
        }
    }

    fn snap(&self, v: f32) -> f32 {
        (v * self.scale).round() / self.scale
    }

    fn rectb(&self, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        let (sx0, sy0) = (self.snap(x), self.snap(y));
        let (sx1, sy1) = (self.snap(x + w), self.snap(y + h));
        Bounds::new(point(px(sx0), px(sy0)), size(px(sx1 - sx0), px(sy1 - sy0)))
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> BoxInk {
        BoxInk::Rect(self.rectb(x, y, w, h))
    }

    fn stroke_px(&self, w: f32) -> f32 {
        (w * self.scale).round().max(1.0) / self.scale
    }

    fn vstroke(&self, x: f32, w: f32, ya: f32, yb: f32) -> BoxInk {
        let (x0, y0, y1) = (self.snap(x - w * 0.5), self.snap(ya), self.snap(yb));
        BoxInk::Rect(Bounds::new(
            point(px(x0), px(y0)),
            size(px(self.stroke_px(w)), px(y1 - y0)),
        ))
    }

    fn hstroke(&self, y: f32, w: f32, xa: f32, xb: f32) -> BoxInk {
        let (y0, x0, x1) = (self.snap(y - w * 0.5), self.snap(xa), self.snap(xb));
        BoxInk::Rect(Bounds::new(
            point(px(x0), px(y0)),
            size(px(x1 - x0), px(self.stroke_px(w))),
        ))
    }

    fn arms(&self, u: Arm, d: Arm, l: Arm, r: Arm) -> Vec<BoxInk> {
        let w = |a: Arm| match a {
            Arm::None => 0.0,
            Arm::Light => self.t,
            Arm::Heavy => self.t * 2.0,
        };
        let (wu, wd, wl, wr) = (w(u), w(d), w(l), w(r));
        let m = wu.max(wd).max(wl).max(wr) * 0.5;
        let mut ink = Vec::new();
        if wu > 0.0 {
            ink.push(self.vstroke(self.cx, wu, self.y0, self.cy + m));
        }
        if wd > 0.0 {
            ink.push(self.vstroke(self.cx, wd, self.cy - m, self.y1));
        }
        if wl > 0.0 {
            ink.push(self.hstroke(self.cy, wl, self.x0, self.cx + m));
        }
        if wr > 0.0 {
            ink.push(self.hstroke(self.cy, wr, self.cx - m, self.x1));
        }
        ink
    }

    fn doubles(&self, c: char) -> Option<Vec<BoxInk>> {
        let t = self.t;
        let h = t * 0.5;
        let d = (t * 1.5).max(2.0);
        let (x0, x1, y0, y1, cx, cy) = (self.x0, self.x1, self.y0, self.y1, self.cx, self.cy);
        let (va, vb) = (cx - d, cx + d);
        let (ha, hb) = (cy - d, cy + d);
        let v = |x: f32, ya: f32, yb: f32| self.vstroke(x, t, ya, yb);
        let hz = |y: f32, xa: f32, xb: f32| self.hstroke(y, t, xa, xb);
        Some(match c {
            '═' => vec![hz(ha, x0, x1), hz(hb, x0, x1)],
            '║' => vec![v(va, y0, y1), v(vb, y0, y1)],
            '╒' => vec![hz(ha, cx - h, x1), hz(hb, cx - h, x1), v(cx, ha - h, y1)],
            '╓' => vec![hz(cy, va - h, x1), v(va, cy - h, y1), v(vb, cy - h, y1)],
            '╔' => vec![
                v(va, ha - h, y1),
                hz(ha, va - h, x1),
                v(vb, hb - h, y1),
                hz(hb, vb - h, x1),
            ],
            '╕' => vec![hz(ha, x0, cx + h), hz(hb, x0, cx + h), v(cx, ha - h, y1)],
            '╖' => vec![hz(cy, x0, vb + h), v(va, cy - h, y1), v(vb, cy - h, y1)],
            '╗' => vec![
                v(vb, ha - h, y1),
                hz(ha, x0, vb + h),
                v(va, hb - h, y1),
                hz(hb, x0, va + h),
            ],
            '╘' => vec![v(cx, y0, hb + h), hz(ha, cx - h, x1), hz(hb, cx - h, x1)],
            '╙' => vec![v(va, y0, cy + h), v(vb, y0, cy + h), hz(cy, va - h, x1)],
            '╚' => vec![
                v(va, y0, hb + h),
                hz(hb, va - h, x1),
                v(vb, y0, ha + h),
                hz(ha, vb - h, x1),
            ],
            '╛' => vec![v(cx, y0, hb + h), hz(ha, x0, cx + h), hz(hb, x0, cx + h)],
            '╜' => vec![v(va, y0, cy + h), v(vb, y0, cy + h), hz(cy, x0, vb + h)],
            '╝' => vec![
                v(vb, y0, hb + h),
                hz(hb, x0, vb + h),
                v(va, y0, ha + h),
                hz(ha, x0, va + h),
            ],
            '╞' => vec![v(cx, y0, y1), hz(ha, cx - h, x1), hz(hb, cx - h, x1)],
            '╟' => vec![v(va, y0, y1), v(vb, y0, y1), hz(cy, vb - h, x1)],
            '╠' => vec![
                v(va, y0, y1),
                v(vb, y0, ha + h),
                v(vb, hb - h, y1),
                hz(ha, vb - h, x1),
                hz(hb, vb - h, x1),
            ],
            '╡' => vec![v(cx, y0, y1), hz(ha, x0, cx + h), hz(hb, x0, cx + h)],
            '╢' => vec![v(va, y0, y1), v(vb, y0, y1), hz(cy, x0, va + h)],
            '╣' => vec![
                v(vb, y0, y1),
                v(va, y0, ha + h),
                v(va, hb - h, y1),
                hz(ha, x0, va + h),
                hz(hb, x0, va + h),
            ],
            '╤' => vec![hz(ha, x0, x1), hz(hb, x0, x1), v(cx, hb - h, y1)],
            '╥' => vec![hz(cy, x0, x1), v(va, cy - h, y1), v(vb, cy - h, y1)],
            '╦' => vec![
                hz(ha, x0, x1),
                hz(hb, x0, va + h),
                hz(hb, vb - h, x1),
                v(va, hb - h, y1),
                v(vb, hb - h, y1),
            ],
            '╧' => vec![hz(ha, x0, x1), hz(hb, x0, x1), v(cx, y0, ha + h)],
            '╨' => vec![hz(cy, x0, x1), v(va, y0, cy + h), v(vb, y0, cy + h)],
            '╩' => vec![
                hz(hb, x0, x1),
                hz(ha, x0, va + h),
                hz(ha, vb - h, x1),
                v(va, y0, ha + h),
                v(vb, y0, ha + h),
            ],
            '╪' => vec![v(cx, y0, y1), hz(ha, x0, x1), hz(hb, x0, x1)],
            '╫' => vec![v(va, y0, y1), v(vb, y0, y1), hz(cy, x0, x1)],
            '╬' => vec![
                v(va, y0, ha + h),
                v(vb, y0, ha + h),
                v(va, hb - h, y1),
                v(vb, hb - h, y1),
                hz(ha, x0, va + h),
                hz(ha, vb - h, x1),
                hz(hb, x0, va + h),
                hz(hb, vb - h, x1),
            ],
            _ => return None,
        })
    }

    fn rounded(&self, c: char) -> Option<Vec<BoxInk>> {
        let (sx, sy): (f32, f32) = match c {
            '╭' => (1.0, 1.0),
            '╮' => (-1.0, 1.0),
            '╯' => (-1.0, -1.0),
            '╰' => (1.0, -1.0),
            _ => return None,
        };
        let h = self.t * 0.5;
        let r = ((self.x1 - self.x0).min(self.y1 - self.y0) * 0.5).max(h * 2.0);
        let (cx, cy) = (self.cx, self.cy);
        let mut ink = Vec::new();
        let lap = 1.0 / self.scale;
        if sy > 0.0 {
            ink.push(self.vstroke(cx, self.t, cy + r - lap, self.y1));
        } else {
            ink.push(self.vstroke(cx, self.t, self.y0, cy - r + lap));
        }
        if sx > 0.0 {
            ink.push(self.hstroke(cy, self.t, cx + r - lap, self.x1));
        } else {
            ink.push(self.hstroke(cy, self.t, self.x0, cx - r + lap));
        }
        let (ax, ay) = (cx + sx * r, cy + sy * r);
        let at = |radius: f32, theta: f32| {
            let (x, y) = (
                ax - sx * radius * theta.cos(),
                ay - sy * radius * theta.sin(),
            );
            point(px(x), px(y))
        };
        const SEGS: usize = 3;
        const INNER_PTS: usize = 4;
        let step = std::f32::consts::FRAC_PI_2 / SEGS as f32;
        let mut path: Option<gpui::Path<Pixels>> = None;
        for i in 0..SEGS {
            let t0 = step * i as f32;
            let t1 = step * (i + 1) as f32;
            let start = at(r + h, t0);
            let p = match path.as_mut() {
                Some(p) => {
                    p.move_to(start);
                    p
                }
                None => path.insert(gpui::Path::new(start)),
            };
            let ctrl = at((r + h) / (step * 0.5).cos(), (t0 + t1) * 0.5);
            p.curve_to(at(r + h, t1), ctrl);
            p.line_to(at(r - h, t1));
            for k in (0..INNER_PTS).rev() {
                p.line_to(at(r - h, t0 + (t1 - t0) * k as f32 / INNER_PTS as f32));
            }
        }
        if let Some(p) = path {
            ink.push(BoxInk::Path(p));
        }
        Some(ink)
    }

    fn dashed(&self, c: char) -> Option<Vec<BoxInk>> {
        let (n, heavy, vertical) = match c {
            '╌' => (2, false, false),
            '╍' => (2, true, false),
            '╎' => (2, false, true),
            '╏' => (2, true, true),
            '┄' => (3, false, false),
            '┅' => (3, true, false),
            '┆' => (3, false, true),
            '┇' => (3, true, true),
            '┈' => (4, false, false),
            '┉' => (4, true, false),
            '┊' => (4, false, true),
            '┋' => (4, true, true),
            _ => return None,
        };
        let w = if heavy { self.t * 2.0 } else { self.t };
        let (a0, a1) = if vertical {
            (self.y0, self.y1)
        } else {
            (self.x0, self.x1)
        };
        let seg = (a1 - a0) / n as f32;
        let ink = (0..n)
            .map(|i| {
                let s = a0 + seg * (i as f32 + 0.15);
                let len = seg * 0.7;
                if vertical {
                    self.vstroke(self.cx, w, s, s + len)
                } else {
                    self.hstroke(self.cy, w, s, s + len)
                }
            })
            .collect();
        Some(ink)
    }

    fn diagonal(&self, c: char) -> Option<Vec<BoxInk>> {
        let (w, hgt) = (self.x1 - self.x0, self.y1 - self.y0);
        let v = self.t * (w * w + hgt * hgt).sqrt() / w.max(1.0);
        let p = |x: f32, y: f32| point(px(x), px(y));
        let quad = |top_x: f32, bot_x: f32| {
            let mut path = gpui::Path::new(p(top_x, self.y0));
            path.line_to(p(top_x, self.y0 + v));
            path.line_to(p(bot_x, self.y1));
            path.line_to(p(bot_x, self.y1 - v));
            BoxInk::Path(path)
        };
        Some(match c {
            '╱' => vec![quad(self.x1, self.x0)],
            '╲' => vec![quad(self.x0, self.x1)],
            '╳' => vec![quad(self.x1, self.x0), quad(self.x0, self.x1)],
            _ => return None,
        })
    }

    fn blocks(&self, c: char) -> Option<Vec<BoxInk>> {
        let (x0, x1, y0, y1, cx, cy) = (self.x0, self.x1, self.y0, self.y1, self.cx, self.cy);
        let (w, hgt) = (x1 - x0, y1 - y0);
        let r = |x: f32, y: f32, ww: f32, hh: f32| self.rect(x, y, ww, hh);
        let ul = || r(x0, y0, cx - x0, cy - y0);
        let ur = || r(cx, y0, x1 - cx, cy - y0);
        let ll = || r(x0, cy, cx - x0, y1 - cy);
        let lr = || r(cx, cy, x1 - cx, y1 - cy);
        Some(match c {
            '▀' => vec![r(x0, y0, w, hgt * 0.5)],
            '▁'..='█' => {
                let k = (c as u32 - 0x2580) as f32;
                let hh = hgt * k / 8.0;
                vec![r(x0, y1 - hh, w, hh)]
            }
            '▉'..='▏' => {
                let k = (0x2590 - c as u32) as f32;
                vec![r(x0, y0, w * k / 8.0, hgt)]
            }
            '▐' => vec![r(cx, y0, x1 - cx, hgt)],
            '░' => vec![BoxInk::Shade(self.rectb(x0, y0, w, hgt), 0.25)],
            '▒' => vec![BoxInk::Shade(self.rectb(x0, y0, w, hgt), 0.50)],
            '▓' => vec![BoxInk::Shade(self.rectb(x0, y0, w, hgt), 0.75)],
            '▔' => vec![r(x0, y0, w, hgt / 8.0)],
            '▕' => vec![r(x1 - w / 8.0, y0, w / 8.0, hgt)],
            '▖' => vec![ll()],
            '▗' => vec![lr()],
            '▘' => vec![ul()],
            '▙' => vec![ul(), ll(), lr()],
            '▚' => vec![ul(), lr()],
            '▛' => vec![ul(), ur(), ll()],
            '▜' => vec![ul(), ur(), lr()],
            '▝' => vec![ur()],
            '▞' => vec![ur(), ll()],
            '▟' => vec![ur(), ll(), lr()],
            _ => return None,
        })
    }
}

fn arms_of(c: char) -> Option<(Arm, Arm, Arm, Arm)> {
    use Arm::{Heavy as H, Light as L, None as N};
    Some(match c {
        '─' => (N, N, L, L),
        '━' => (N, N, H, H),
        '│' => (L, L, N, N),
        '┃' => (H, H, N, N),
        '┌' => (N, L, N, L),
        '┍' => (N, L, N, H),
        '┎' => (N, H, N, L),
        '┏' => (N, H, N, H),
        '┐' => (N, L, L, N),
        '┑' => (N, L, H, N),
        '┒' => (N, H, L, N),
        '┓' => (N, H, H, N),
        '└' => (L, N, N, L),
        '┕' => (L, N, N, H),
        '┖' => (H, N, N, L),
        '┗' => (H, N, N, H),
        '┘' => (L, N, L, N),
        '┙' => (L, N, H, N),
        '┚' => (H, N, L, N),
        '┛' => (H, N, H, N),
        '├' => (L, L, N, L),
        '┝' => (L, L, N, H),
        '┞' => (H, L, N, L),
        '┟' => (L, H, N, L),
        '┠' => (H, H, N, L),
        '┡' => (H, L, N, H),
        '┢' => (L, H, N, H),
        '┣' => (H, H, N, H),
        '┤' => (L, L, L, N),
        '┥' => (L, L, H, N),
        '┦' => (H, L, L, N),
        '┧' => (L, H, L, N),
        '┨' => (H, H, L, N),
        '┩' => (H, L, H, N),
        '┪' => (L, H, H, N),
        '┫' => (H, H, H, N),
        '┬' => (N, L, L, L),
        '┭' => (N, L, H, L),
        '┮' => (N, L, L, H),
        '┯' => (N, L, H, H),
        '┰' => (N, H, L, L),
        '┱' => (N, H, H, L),
        '┲' => (N, H, L, H),
        '┳' => (N, H, H, H),
        '┴' => (L, N, L, L),
        '┵' => (L, N, H, L),
        '┶' => (L, N, L, H),
        '┷' => (L, N, H, H),
        '┸' => (H, N, L, L),
        '┹' => (H, N, H, L),
        '┺' => (H, N, L, H),
        '┻' => (H, N, H, H),
        '┼' => (L, L, L, L),
        '┽' => (L, L, H, L),
        '┾' => (L, L, L, H),
        '┿' => (L, L, H, H),
        '╀' => (H, L, L, L),
        '╁' => (L, H, L, L),
        '╂' => (H, H, L, L),
        '╃' => (H, L, H, L),
        '╄' => (H, L, L, H),
        '╅' => (L, H, H, L),
        '╆' => (L, H, L, H),
        '╇' => (H, L, H, H),
        '╈' => (L, H, H, H),
        '╉' => (H, H, H, L),
        '╊' => (H, H, L, H),
        '╋' => (H, H, H, H),
        '╴' => (N, N, L, N),
        '╵' => (L, N, N, N),
        '╶' => (N, N, N, L),
        '╷' => (N, L, N, N),
        '╸' => (N, N, H, N),
        '╹' => (H, N, N, N),
        '╺' => (N, N, N, H),
        '╻' => (N, H, N, N),
        '╼' => (N, N, L, H),
        '╽' => (L, H, N, N),
        '╾' => (N, N, H, L),
        '╿' => (H, L, N, N),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_entire_unicode_box_and_block_range() {
        let cell = Bounds::new(point(px(10.0), px(20.0)), size(px(9.0), px(21.0)));
        for cp in 0x2500u32..=0x259f {
            let c = char::from_u32(cp).unwrap();
            assert!(
                glyph(c, cell, 1.25).is_some(),
                "U+{cp:04X} ({c}) was not covered by boxdraw"
            );
        }
    }
}
