//! GPU-rendered terminal element (`TerminalElement`) built on GPUI and `alacritty_terminal`
//! (`gateterm-client-core::term::ClientTerm`), inspired by `tty7`.
//!
//! Renders cell backgrounds, contiguous text runs with ligatures disabled,
//! wide characters (CJK/emoji), bold/italic/underline/strikethrough/inverse/dim styles,
//! viewport selection highlight, IME composition preedit, block/hollow cursor,
//! and scrollback indicator.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use gateterm_client_core::graphics::ViewportImage;
use gateterm_client_core::term::attr;
use gpui::{
    App, BorderStyle, Bounds, ContentMask, Corners, Element, ElementId, ElementInputHandler,
    Entity, Font, FontFeatures, FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior,
    Hsla, IntoElement, LayoutId, Pixels, RenderImage, SharedString, ShapedLine, StrikethroughStyle,
    Style, TextAlign, TextRun, UnderlineStyle, Window, fill, outline, point, px, relative, size,
};

use super::boxdraw;
use super::view::TerminalView;
use crate::theme::TerminalPalette;

const DIM_ALPHA: f32 = 0.65;
const MIN_CONTRAST_RATIO: f32 = 2.5;
const MAX_CACHED_SHAPED_ROWS: usize = 512;
const MAX_BRIDGED_SPACES: usize = 3;

pub struct TerminalElement {
    view: Entity<TerminalView>,
}

impl TerminalElement {
    pub fn new(view: Entity<TerminalView>) -> Self {
        Self { view }
    }
}

pub struct TermPrepaintState {
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub cols: u16,
    pub rows: u16,
    _hitbox: Hitbox,
}

#[derive(Clone, Debug)]
pub(crate) struct RenderCell {
    pub(crate) ch: char,
    pub(crate) fg: Hsla,
    pub(crate) bg: Hsla,
    pub(crate) draw_bg: bool,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) double_underline: bool,
    pub(crate) undercurl: bool,
    pub(crate) dotted_underline: bool,
    pub(crate) dashed_underline: bool,
    pub(crate) hyperlink: bool,
    pub(crate) strike: bool,
    pub(crate) width: u8,
    pub(crate) selected: bool,
    pub(crate) search_match: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowRunSpec {
    pub len: usize,
    pub font_idx: usize,
    pub fg: Hsla,
    pub undercurl: bool,
    pub simple_underline: bool,
    pub strike: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowSegmentSpec {
    pub start_col: usize,
    pub cell_span: u8,
    pub text: SharedString,
    pub runs: Vec<RowRunSpec>,
}

pub(crate) struct CachedShapedSegment {
    pub start_col: usize,
    pub shaped: ShapedLine,
}

#[derive(Default)]
pub(crate) struct RowShapeCache {
    font_family: SharedString,
    font_size_bits: u32,
    cell_width_bits: u32,
    entries: HashMap<u64, Arc<Vec<CachedShapedSegment>>>,
    pub(crate) hits: usize,
    pub(crate) misses: usize,
}

impl RowShapeCache {
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.hits = 0;
        self.misses = 0;
    }

    pub(crate) fn prepare(
        &mut self,
        font_family: &SharedString,
        font_size: Pixels,
        cell_width: Pixels,
    ) {
        let fs_bits = font_size.as_f32().to_bits();
        let cw_bits = cell_width.as_f32().to_bits();
        if self.font_family != *font_family
            || self.font_size_bits != fs_bits
            || self.cell_width_bits != cw_bits
        {
            self.font_family = font_family.clone();
            self.font_size_bits = fs_bits;
            self.cell_width_bits = cw_bits;
            self.entries.clear();
        }
    }

    pub(crate) fn get(&mut self, row_hash: u64) -> Option<Arc<Vec<CachedShapedSegment>>> {
        if let Some(found) = self.entries.get(&row_hash).cloned() {
            self.hits = self.hits.wrapping_add(1);
            Some(found)
        } else {
            self.misses = self.misses.wrapping_add(1);
            None
        }
    }

    pub(crate) fn insert(&mut self, row_hash: u64, segments: Arc<Vec<CachedShapedSegment>>) {
        if self.entries.len() >= MAX_CACHED_SHAPED_ROWS {
            self.entries.clear();
        }
        self.entries.insert(row_hash, segments);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Returns true for characters whose foreground color should not be modified by
/// minimum contrast enforcement: spaces, box-drawing & block elements (`U+2500..=U+259F`),
/// and Private Use Area / Powerline / Nerd Font glyphs (`U+E000..=U+F8FF`, Supplementary PUA).
pub fn is_contrast_exempt_glyph(ch: char) -> bool {
    ch == ' '
        || boxdraw::is_box_or_block(ch)
        || ('\u{2500}'..='\u{259F}').contains(&ch)
        || ('\u{E000}'..='\u{F8FF}').contains(&ch)
        || ('\u{F0000}'..='\u{FFFFD}').contains(&ch)
        || ('\u{100000}'..='\u{10FFFD}').contains(&ch)
}

fn hsl_to_srgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(1.0);
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    if s <= 1e-5 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue_to_rgb = |t: f32| -> f32 {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (
        hue_to_rgb(h + 1.0 / 3.0),
        hue_to_rgb(h),
        hue_to_rgb(h - 1.0 / 3.0),
    )
}

fn srgb_channel_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn relative_luminance(color: Hsla) -> f32 {
    let (r, g, b) = hsl_to_srgb(color.h, color.s, color.l);
    0.2126 * srgb_channel_to_linear(r)
        + 0.7152 * srgb_channel_to_linear(g)
        + 0.0722 * srgb_channel_to_linear(b)
}

pub fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Nudges `fg.l` (lightness) when `contrast_ratio(fg, bg) < min_ratio` so text remains
/// legible against `bg` without altering hue, saturation, or alpha.
pub fn ensure_minimum_contrast(fg: Hsla, bg: Hsla, min_ratio: f32) -> Hsla {
    if min_ratio <= 1.0 || contrast_ratio(fg, bg) >= min_ratio {
        return fg;
    }

    let bg_lum = relative_luminance(bg);
    // Prefer lightening on dark backgrounds and darkening on light backgrounds.
    let target_l = if bg_lum < 0.5 { 1.0f32 } else { 0.0f32 };

    let mut lo = fg.l.clamp(0.0, 1.0);
    let mut hi = target_l;
    let mut best = Hsla { l: hi, ..fg };

    if contrast_ratio(best, bg) < min_ratio {
        // Fallback to the opposite extreme if target_l cannot reach min_ratio.
        let alt = Hsla {
            l: 1.0 - target_l,
            ..fg
        };
        if contrast_ratio(alt, bg) > contrast_ratio(best, bg) {
            hi = alt.l;
            best = alt;
        } else {
            return best;
        }
    }

    for _ in 0..10 {
        let mid = 0.5 * (lo + hi);
        let candidate = Hsla { l: mid, ..fg };
        if contrast_ratio(candidate, bg) >= min_ratio {
            best = candidate;
            hi = mid;
        } else {
            lo = mid;
        }
    }
    best
}

pub(crate) fn hash_row_cells(row_cells: &[RenderCell]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    row_cells.len().hash(&mut hasher);
    for c in row_cells {
        c.ch.hash(&mut hasher);
        c.width.hash(&mut hasher);
        if c.width > 0
            && !boxdraw::is_box_or_block(c.ch)
            && !(c.ch == ' ' && !c.underline && !c.undercurl && !c.strike)
        {
            c.fg.h.to_bits().hash(&mut hasher);
            c.fg.s.to_bits().hash(&mut hasher);
            c.fg.l.to_bits().hash(&mut hasher);
            c.fg.a.to_bits().hash(&mut hasher);
            let flags: u8 = (c.bold as u8)
                | ((c.italic as u8) << 1)
                | ((c.underline as u8) << 2)
                | ((c.double_underline as u8) << 3)
                | ((c.undercurl as u8) << 4)
                | ((c.dotted_underline as u8) << 5)
                | ((c.dashed_underline as u8) << 6)
                | ((c.strike as u8) << 7);
            flags.hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn is_unstyled_space(c: &RenderCell) -> bool {
    c.ch == ' ' && !c.underline && !c.undercurl && !c.strike
}

fn cell_style_tuple(c: &RenderCell) -> (usize, Hsla, bool, bool, bool) {
    let font_idx = (c.bold as usize) | ((c.italic as usize) << 1);
    let simple_underline = c.underline
        && !c.double_underline
        && !c.dotted_underline
        && !c.dashed_underline
        && !c.undercurl;
    (font_idx, c.fg, c.undercurl, simple_underline, c.strike)
}

fn push_char_to_runs(runs: &mut Vec<RowRunSpec>, ch: char, style: (usize, Hsla, bool, bool, bool)) {
    let byte_len = ch.len_utf8();
    let (font_idx, fg, undercurl, simple_underline, strike) = style;
    if let Some(last) = runs.last_mut()
        && last.font_idx == font_idx
        && last.fg == fg
        && last.undercurl == undercurl
        && last.simple_underline == simple_underline
        && last.strike == strike
    {
        last.len += byte_len;
    } else {
        runs.push(RowRunSpec {
            len: byte_len,
            font_idx,
            fg,
            undercurl,
            simple_underline,
            strike,
        });
    }
}

/// Groups a row of `RenderCell`s into contiguous text segments for `shape_line`.
///
/// - Contiguous 1-column characters (`width == 1`, both ASCII and Unicode 1-column glyphs like
///   Braille spinners or symbols) are merged into a single `RowSegmentSpec` with multiple
///   `RowRunSpec`s, bridging up to `MAX_BRIDGED_SPACES` unstyled spaces between words so a normal
///   code/CLI line shapes in a single `shape_line` call.
/// - Contiguous 2-column wide characters (`width == 2` followed by a `width == 0` spacer cell)
///   are also merged into a single `cell_span == 2` segment with multiple `RowRunSpec`s.
pub(crate) fn build_row_segment_specs(row_cells: &[RenderCell]) -> Vec<RowSegmentSpec> {
    let cols = row_cells.len();
    let mut segments = Vec::new();
    let mut x = 0;

    while x < cols {
        let cell = &row_cells[x];
        if cell.width == 0 || boxdraw::is_box_or_block(cell.ch) || is_unstyled_space(cell) {
            x += 1;
            continue;
        }

        if cell.width == 1 {
            let start_col = x;
            let mut text = String::new();
            let mut runs: Vec<RowRunSpec> = Vec::new();

            while x < cols {
                let c = &row_cells[x];
                if c.width != 1 || boxdraw::is_box_or_block(c.ch) {
                    break;
                }
                if is_unstyled_space(c) {
                    let mut space_end = x + 1;
                    while space_end < cols
                        && row_cells[space_end].width == 1
                        && !boxdraw::is_box_or_block(row_cells[space_end].ch)
                        && is_unstyled_space(&row_cells[space_end])
                    {
                        space_end += 1;
                    }
                    let space_count = space_end - x;
                    let has_following_narrow = space_end < cols
                        && row_cells[space_end].width == 1
                        && !boxdraw::is_box_or_block(row_cells[space_end].ch)
                        && !is_unstyled_space(&row_cells[space_end]);

                    if !has_following_narrow || space_count > MAX_BRIDGED_SPACES {
                        x = space_end;
                        break;
                    }

                    // Bridge short interior spaces using the previous run's font/color (without underline/strike)
                    let bridge_style = runs
                        .last()
                        .map(|r| (r.font_idx, r.fg, false, false, false))
                        .unwrap_or_else(|| (0, c.fg, false, false, false));
                    for _ in 0..space_count {
                        text.push(' ');
                        push_char_to_runs(&mut runs, ' ', bridge_style);
                    }
                    x = space_end;
                    continue;
                }

                text.push(c.ch);
                push_char_to_runs(&mut runs, c.ch, cell_style_tuple(c));
                x += 1;
            }

            if !text.is_empty() {
                segments.push(RowSegmentSpec {
                    start_col,
                    cell_span: 1,
                    text: SharedString::from(text),
                    runs,
                });
            }
        } else if cell.width == 2 && x + 1 < cols && row_cells[x + 1].width == 0 {
            let start_col = x;
            let mut text = String::new();
            let mut runs: Vec<RowRunSpec> = Vec::new();

            while x + 1 < cols {
                let c = &row_cells[x];
                if c.width != 2
                    || row_cells[x + 1].width != 0
                    || boxdraw::is_box_or_block(c.ch)
                    || is_unstyled_space(c)
                {
                    break;
                }
                text.push(c.ch);
                push_char_to_runs(&mut runs, c.ch, cell_style_tuple(c));
                x += 2;
            }

            if !text.is_empty() {
                segments.push(RowSegmentSpec {
                    start_col,
                    cell_span: 2,
                    text: SharedString::from(text),
                    runs,
                });
            }
        } else {
            let span_cols = cell.width.max(1);
            let mut s = String::with_capacity(4);
            s.push(cell.ch);
            let mut runs = Vec::with_capacity(1);
            push_char_to_runs(&mut runs, cell.ch, cell_style_tuple(cell));
            segments.push(RowSegmentSpec {
                start_col: x,
                cell_span: span_cols,
                text: SharedString::from(s),
                runs,
            });
            x += 1;
        }
    }

    segments
}

fn shape_row_segments(
    specs: &[RowSegmentSpec],
    fonts: &[Font; 4],
    font_size: Pixels,
    cell_width: Pixels,
    window: &mut Window,
) -> Vec<CachedShapedSegment> {
    let mut out = Vec::with_capacity(specs.len());
    let mut gpui_runs = Vec::new();
    for spec in specs {
        gpui_runs.clear();
        gpui_runs.reserve(spec.runs.len());
        for r in &spec.runs {
            let underline = if r.undercurl {
                Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(r.fg),
                    wavy: true,
                })
            } else if r.simple_underline {
                Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(r.fg),
                    wavy: false,
                })
            } else {
                None
            };
            let strikethrough = r.strike.then_some(StrikethroughStyle {
                thickness: px(1.0),
                color: Some(r.fg),
            });
            gpui_runs.push(TextRun {
                len: r.len,
                font: fonts[r.font_idx & 3].clone(),
                color: r.fg,
                background_color: None,
                underline,
                strikethrough,
            });
        }
        let force_width = Some(cell_width * (spec.cell_span.max(1) as f32));
        let shaped = window.text_system().shape_line(
            spec.text.clone(),
            font_size,
            &gpui_runs,
            force_width,
        );
        out.push(CachedShapedSegment {
            start_col: spec.start_col,
            shaped,
        });
    }
    out
}

pub fn image_cell_bounds(
    origin_x: Pixels,
    origin_y: Pixels,
    cell_w: Pixels,
    cell_h: Pixels,
    col: u32,
    row: i32,
    cols: u32,
    rows: u32,
) -> Bounds<Pixels> {
    Bounds::new(
        point(
            origin_x + cell_w * (col as f32),
            origin_y + cell_h * (row as f32),
        ),
        size(
            cell_w * (cols.max(1) as f32),
            cell_h * (rows.max(1) as f32),
        ),
    )
}

pub fn double_underline_bounds(
    origin_x: Pixels,
    row_bottom: Pixels,
    width: Pixels,
    thickness: Pixels,
) -> [Bounds<Pixels>; 2] {
    let h = thickness.max(px(1.0));
    [
        Bounds::new(point(origin_x, row_bottom - h * 3.0), size(width, h)),
        Bounds::new(point(origin_x, row_bottom - h), size(width, h)),
    ]
}

pub fn undercurl_segments(
    origin_x: Pixels,
    row_bottom: Pixels,
    width: Pixels,
    thickness: Pixels,
) -> Vec<Bounds<Pixels>> {
    let w = width.as_f32().max(0.0);
    let h = thickness.as_f32().max(1.0);
    let base_y = row_bottom.as_f32() - h * 2.0;
    let step = (h * 2.0).max(2.0);
    let mut out = Vec::new();
    let mut dx = 0.0f32;
    let mut idx = 0usize;
    while dx < w {
        let seg_w = (w - dx).min(step);
        let dy = match idx % 4 {
            0 => 0.0,
            1 => -h,
            2 => 0.0,
            _ => h,
        };
        out.push(Bounds::new(
            point(px(origin_x.as_f32() + dx), px(base_y + dy)),
            size(px(seg_w), px(h)),
        ));
        dx += step;
        idx += 1;
    }
    out
}

pub fn dotted_underline_segments(
    origin_x: Pixels,
    row_bottom: Pixels,
    width: Pixels,
    thickness: Pixels,
) -> Vec<Bounds<Pixels>> {
    let w = width.as_f32().max(0.0);
    let h = thickness.as_f32().max(1.0);
    let y = row_bottom.as_f32() - h * 2.0;
    let dot = (h * 2.0).max(2.0);
    let gap = dot;
    let mut out = Vec::new();
    let mut dx = 0.0f32;
    while dx < w {
        let seg_w = (w - dx).min(dot);
        out.push(Bounds::new(
            point(px(origin_x.as_f32() + dx), px(y)),
            size(px(seg_w), px(h)),
        ));
        dx += dot + gap;
    }
    out
}

pub fn dashed_underline_segments(
    origin_x: Pixels,
    row_bottom: Pixels,
    width: Pixels,
    thickness: Pixels,
) -> Vec<Bounds<Pixels>> {
    let w = width.as_f32().max(0.0);
    let h = thickness.as_f32().max(1.0);
    let y = row_bottom.as_f32() - h * 2.0;
    let dash = (h * 4.0).max(4.0);
    let gap = (h * 2.0).max(2.0);
    let mut out = Vec::new();
    let mut dx = 0.0f32;
    while dx < w {
        let seg_w = (w - dx).min(dash);
        out.push(Bounds::new(
            point(px(origin_x.as_f32() + dx), px(y)),
            size(px(seg_w), px(h)),
        ));
        dx += dash + gap;
    }
    out
}

fn disable_ligatures() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![
        ("calt".to_string(), 0),
        ("liga".to_string(), 0),
        ("clig".to_string(), 0),
    ]))
}

fn make_font(family: &SharedString, bold: bool, italic: bool) -> Font {
    Font {
        family: family.clone(),
        features: disable_ligatures(),
        fallbacks: None,
        weight: if bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        },
        style: if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = TermPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        style.flex_grow = 1.0;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (font_family, font_size, line_height_mul) = {
            let v = self.view.read(cx);
            (v.font_family.clone(), v.font_size, v.line_height_mul)
        };
        let base_font = make_font(&font_family, false, false);
        let sample = window.text_system().shape_line(
            SharedString::new_static("M"),
            font_size,
            &[TextRun {
                len: 1,
                font: base_font,
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        );
        let cell_width = sample.width.max(px(6.0));
        let line_height = px((font_size.as_f32() * line_height_mul).round()).max(px(10.0));

        let pad_x = px(8.0);
        let pad_y = px(6.0);
        let avail_w = (bounds.size.width - pad_x * 2.0).max(cell_width);
        let avail_h = (bounds.size.height - pad_y * 2.0).max(line_height);

        let cols = (avail_w.as_f32() / cell_width.as_f32()).floor().clamp(2.0, 500.0) as u16;
        let rows = (avail_h.as_f32() / line_height.as_f32()).floor().clamp(1.0, 300.0) as u16;

        let inner_origin = point(bounds.origin.x + pad_x, bounds.origin.y + pad_y);
        self.view.update(cx, |view, cx| {
            view.update_viewport_geometry(inner_origin, cell_width, line_height, cols, rows, cx);
        });

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        TermPrepaintState {
            cell_width,
            line_height,
            cols,
            rows,
            _hitbox: hitbox,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let palette = cx
            .try_global::<TerminalPalette>()
            .copied()
            .unwrap_or_else(|| crate::theme::TerminalPaletteId::Gate2026Dark.palette());

        // Paint full terminal background
        window.paint_quad(fill(bounds, palette.bg_hsla()));

        let pad_x = px(8.0);
        let pad_y = px(6.0);
        let origin = point(bounds.origin.x + pad_x, bounds.origin.y + pad_y);

        let (
            focus_handle,
            font_family,
            font_size,
            term_state,
            cells,
            ime_marked,
            cursor_bounds,
            under_text_images,
            over_text_images,
            row_shape_cache,
        ) = {
            let view = self.view.read(cx);
            let st = view.term.state();
            let render_cols = st.cols as usize;
            let render_rows = st.rows as usize;
            let mut grid_cells = Vec::with_capacity(render_rows * render_cols);

            for y in 0..render_rows {
                for x in 0..render_cols {
                    let c = view.term.cell_scrolled(x, y);
                    let inverse = (c.attrs & attr::INVERSE) != 0;
                    let hidden = (c.attrs & attr::HIDDEN) != 0;
                    let mut fg = palette.resolve_cell_color(c.fg, c.fg_index, true);
                    let mut bg = palette.resolve_cell_color(c.bg, c.bg_index, false);
                    let mut draw_bg =
                        c.bg_index != gateterm_client_core::term::COLOR_INDEX_DEFAULT;
                    if inverse {
                        std::mem::swap(&mut fg, &mut bg);
                        draw_bg = true;
                    }
                    let ch = char::from_u32(c.codepoint).unwrap_or(' ');
                    if hidden {
                        fg = bg;
                    } else if !is_contrast_exempt_glyph(ch) {
                        fg = ensure_minimum_contrast(fg, bg, MIN_CONTRAST_RATIO);
                    }
                    if (c.attrs & attr::DIM) != 0 {
                        fg.a *= DIM_ALPHA;
                    }
                    grid_cells.push(RenderCell {
                        ch,
                        fg,
                        bg,
                        draw_bg,
                        bold: (c.attrs & attr::BOLD) != 0,
                        italic: (c.attrs & attr::ITALIC) != 0,
                        underline: (c.attrs & attr::UNDERLINE) != 0,
                        double_underline: (c.attrs & attr::DOUBLE_UNDERLINE) != 0,
                        undercurl: (c.attrs & attr::UNDERCURL) != 0,
                        dotted_underline: (c.attrs & attr::DOTTED_UNDERLINE) != 0,
                        dashed_underline: (c.attrs & attr::DASHED_UNDERLINE) != 0,
                        hyperlink: (c.attrs & attr::HYPERLINK) != 0,
                        strike: (c.attrs & attr::STRIKE) != 0,
                        width: c.width,
                        selected: view.is_cell_selected(x as i32, y as i32),
                        search_match: view.cell_search_match_kind(x, y),
                    });
                }
            }

            let cur_rect = Bounds::new(
                point(
                    origin.x + prepaint.cell_width * (st.cursor_x as f32),
                    origin.y + prepaint.line_height * (st.cursor_y as f32),
                ),
                size(prepaint.cell_width, prepaint.line_height),
            );

            let mut under_imgs: Vec<(ViewportImage, Arc<RenderImage>)> = Vec::new();
            let mut over_imgs: Vec<(ViewportImage, Arc<RenderImage>)> = Vec::new();
            for img in view.term.visible_images() {
                if let Some(render_img) = view.render_image_for(&img, cx) {
                    if img.z_index < 0 {
                        under_imgs.push((img, render_img));
                    } else {
                        over_imgs.push((img, render_img));
                    }
                }
            }

            (
                view.focus_handle.clone(),
                view.font_family.clone(),
                view.font_size,
                st,
                grid_cells,
                view.ime_marked_text.clone(),
                cur_rect,
                under_imgs,
                over_imgs,
                view.row_shape_cache.clone(),
            )
        };

        let focused = focus_handle.is_focused(window);
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(cursor_bounds, self.view.clone()),
            cx,
        );

        let cols = term_state.cols as usize;
        let rows = term_state.rows as usize;
        if cols == 0 || rows == 0 {
            return;
        }

        let fonts = [
            make_font(&font_family, false, false),
            make_font(&font_family, true, false),
            make_font(&font_family, false, true),
            make_font(&font_family, true, true),
        ];

        row_shape_cache
            .borrow_mut()
            .prepare(&font_family, font_size, prepaint.cell_width);

        let scale = window.scale_factor().max(1.0);
        let snap_px = |v: f32| -> f32 { (v * scale).round() / scale };
        let span_rect = |start_col: usize, end_col: usize, row: usize| -> Bounds<Pixels> {
            let x0 = snap_px(origin.x.as_f32() + prepaint.cell_width.as_f32() * (start_col as f32));
            let x1 = snap_px(origin.x.as_f32() + prepaint.cell_width.as_f32() * (end_col as f32));
            let y0 = snap_px(origin.y.as_f32() + prepaint.line_height.as_f32() * (row as f32));
            let y1 =
                snap_px(origin.y.as_f32() + prepaint.line_height.as_f32() * ((row + 1) as f32));
            Bounds::new(point(px(x0), px(y0)), size(px(x1 - x0), px(y1 - y0)))
        };

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            // 1. Paint non-default cell backgrounds & selection runs (run-length merged & pixel-snapped)
            let sel_bg = palette.selection_hsla();
            for y in 0..rows {
                let row_offset = y * cols;

                // Cell backgrounds
                let mut x = 0;
                while x < cols {
                    let cell = &cells[row_offset + x];
                    if !cell.draw_bg {
                        x += 1;
                        continue;
                    }
                    let bg = cell.bg;
                    let start = x;
                    while x < cols {
                        let c = &cells[row_offset + x];
                        if c.width == 0 || (c.draw_bg && c.bg == bg) {
                            x += 1;
                        } else {
                            break;
                        }
                    }
                    window.paint_quad(fill(span_rect(start, x, y), bg));
                }

                // Selection highlight runs
                let mut sx = 0;
                while sx < cols {
                    if !cells[row_offset + sx].selected {
                        sx += 1;
                        continue;
                    }
                    let start = sx;
                    while sx < cols
                        && (cells[row_offset + sx].selected || cells[row_offset + sx].width == 0)
                    {
                        sx += 1;
                    }
                    window.paint_quad(fill(span_rect(start, sx, y), sel_bg));
                }

                // Search match highlight runs
                let mut mx = 0;
                while mx < cols {
                    let Some(is_current) = cells[row_offset + mx].search_match else {
                        mx += 1;
                        continue;
                    };
                    let start = mx;
                    while mx < cols
                        && (cells[row_offset + mx].search_match == Some(is_current)
                            || cells[row_offset + mx].width == 0)
                    {
                        mx += 1;
                    }
                    let match_color = if is_current {
                        gpui::hsla(0.11, 0.95, 0.55, 0.45)
                    } else {
                        gpui::hsla(0.14, 0.85, 0.50, 0.25)
                    };
                    window.paint_quad(fill(span_rect(start, mx, y), match_color));
                }
            }

            // 1b. Paint Kitty graphics placements with z_index < 0 (below text, above cell backgrounds)
            for (img, render_img) in &under_text_images {
                let img_bounds = image_cell_bounds(
                    origin.x,
                    origin.y,
                    prepaint.cell_width,
                    prepaint.line_height,
                    img.col,
                    img.row,
                    img.cols,
                    img.rows,
                );
                let _ = window.paint_image(
                    img_bounds,
                    Corners::default(),
                    render_img.clone(),
                    0,
                    false,
                );
            }

            // 2. Paint procedural box-drawing/block glyphs, cached multi-run shaped text & fancy underlines row by row
            for y in 0..rows {
                let row_offset = y * cols;
                let row_slice = &cells[row_offset..row_offset + cols];
                let row_y = origin.y + prepaint.line_height * (y as f32);
                let row_bottom = row_y + prepaint.line_height;

                // 2a. Procedural pixel-snapped Box-Drawing (U+2500..=U+257F) & Block Elements (U+2580..=U+259F)
                for (x, cell) in row_slice.iter().enumerate() {
                    if cell.width > 0 && boxdraw::is_box_or_block(cell.ch) {
                        let span_cols = (cell.width.max(1)) as usize;
                        let cell_rect = span_rect(x, x + span_cols, y);
                        boxdraw::paint_box_or_block(cell.ch, cell_rect, cell.fg, scale, window);
                    }
                }

                // 2b. Row-level cached multi-run text shaping
                let row_hash = hash_row_cells(row_slice);
                let shaped_segments = {
                    let cached = row_shape_cache.borrow_mut().get(row_hash);
                    match cached {
                        Some(segs) => segs,
                        None => {
                            let specs = build_row_segment_specs(row_slice);
                            let shaped = Arc::new(shape_row_segments(
                                &specs,
                                &fonts,
                                font_size,
                                prepaint.cell_width,
                                window,
                            ));
                            row_shape_cache
                                .borrow_mut()
                                .insert(row_hash, shaped.clone());
                            shaped
                        }
                    }
                };

                for seg in shaped_segments.iter() {
                    let _ = seg.shaped.paint(
                        point(origin.x + prepaint.cell_width * (seg.start_col as f32), row_y),
                        prepaint.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }

                // 2c. Fancy underline styles (DOUBLE_UNDERLINE, DOTTED, DASHED, HYPERLINK)
                let mut ux = 0;
                while ux < cols {
                    let c = &row_slice[ux];
                    let has_fancy = c.double_underline
                        || c.dotted_underline
                        || c.dashed_underline
                        || (c.hyperlink
                            && !c.underline
                            && !c.double_underline
                            && !c.undercurl
                            && !c.dotted_underline
                            && !c.dashed_underline);
                    if c.width == 0 || !has_fancy {
                        ux += 1;
                        continue;
                    }
                    let start = ux;
                    let fg = c.fg;
                    let double_u = c.double_underline;
                    let dotted_u = c.dotted_underline;
                    let dashed_u = c.dashed_underline;
                    let link_u = c.hyperlink;
                    ux += 1;
                    while ux < cols {
                        let cn = &row_slice[ux];
                        if cn.width == 0
                            || (cn.fg == fg
                                && cn.double_underline == double_u
                                && cn.dotted_underline == dotted_u
                                && cn.dashed_underline == dashed_u
                                && cn.hyperlink == link_u)
                        {
                            ux += 1;
                        } else {
                            break;
                        }
                    }
                    let span_x = origin.x + prepaint.cell_width * (start as f32);
                    let span_w = prepaint.cell_width * ((ux - start) as f32);
                    if double_u {
                        for seg in double_underline_bounds(span_x, row_bottom, span_w, px(1.0)) {
                            window.paint_quad(fill(seg, fg));
                        }
                    } else if dotted_u {
                        for seg in dotted_underline_segments(span_x, row_bottom, span_w, px(1.0)) {
                            window.paint_quad(fill(seg, fg));
                        }
                    } else if dashed_u {
                        for seg in dashed_underline_segments(span_x, row_bottom, span_w, px(1.0)) {
                            window.paint_quad(fill(seg, fg));
                        }
                    } else if link_u {
                        let mut link_color = fg;
                        link_color.a *= 0.65;
                        for seg in dotted_underline_segments(span_x, row_bottom, span_w, px(1.0)) {
                            window.paint_quad(fill(seg, link_color));
                        }
                    }
                }
            }

            // 2b. Paint Kitty graphics placements with z_index >= 0 (above text, below cursor/IME)
            for (img, render_img) in &over_text_images {
                let img_bounds = image_cell_bounds(
                    origin.x,
                    origin.y,
                    prepaint.cell_width,
                    prepaint.line_height,
                    img.col,
                    img.row,
                    img.cols,
                    img.rows,
                );
                let _ = window.paint_image(
                    img_bounds,
                    Corners::default(),
                    render_img.clone(),
                    0,
                    false,
                );
            }

            // 3. Paint cursor when at bottom of scrollback and cursor is visible
            if term_state.cursor_visible && term_state.scroll_offset == 0 {
                let cx_idx = (term_state.cursor_x as usize).min(cols.saturating_sub(1));
                let cy_idx = (term_state.cursor_y as usize).min(rows.saturating_sub(1));
                let cur_cell = &cells[cy_idx * cols + cx_idx];
                let cur_cols = (cur_cell.width.max(1)) as usize;
                let cur_w = prepaint.cell_width * (cur_cols as f32);
                let cur_rect = span_rect(cx_idx, cx_idx + cur_cols, cy_idx);
                let cursor_color = palette.cursor_hsla();

                if focused {
                    window.paint_quad(fill(cur_rect, cursor_color));
                    if cur_cell.ch != ' ' {
                        if boxdraw::is_box_or_block(cur_cell.ch) {
                            boxdraw::paint_box_or_block(
                                cur_cell.ch,
                                cur_rect,
                                palette.bg_hsla(),
                                scale,
                                window,
                            );
                        } else {
                            let mut s = String::with_capacity(4);
                            s.push(cur_cell.ch);
                            let shared = SharedString::from(s);
                            let run = [TextRun {
                                len: shared.len(),
                                font: fonts[1].clone(),
                                color: palette.bg_hsla(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }];
                            let shaped = window.text_system().shape_line(
                                shared,
                                font_size,
                                &run,
                                Some(cur_w),
                            );
                            let _ = shaped.paint(
                                cur_rect.origin,
                                prepaint.line_height,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        }
                    }
                } else {
                    window.paint_quad(outline(cur_rect, cursor_color, BorderStyle::Solid));
                }
            }

            // 4. Paint IME marked (preedit) text if active
            if let Some(marked) = ime_marked.filter(|m| !m.is_empty()) {
                let shared = SharedString::from(marked);
                let char_cols = shared.chars().count().max(1) as f32;
                let preedit_bounds = Bounds::new(
                    cursor_bounds.origin,
                    size(prepaint.cell_width * char_cols, prepaint.line_height),
                );
                window.paint_quad(fill(preedit_bounds, palette.bg_hsla()));
                let run = [TextRun {
                    len: shared.len(),
                    font: fonts[0].clone(),
                    color: palette.fg_hsla(),
                    background_color: None,
                    underline: Some(UnderlineStyle {
                        thickness: px(1.5),
                        color: Some(palette.cursor_hsla()),
                        wavy: false,
                    }),
                    strikethrough: None,
                }];
                let shaped = window.text_system().shape_line(shared, font_size, &run, None);
                let _ = shaped.paint(
                    cursor_bounds.origin,
                    prepaint.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }

            // 5. Paint scrollbar thumb when scrollback history exists
            if term_state.max_scroll_offset > 0 {
                let total_lines = (rows as f32) + (term_state.max_scroll_offset as f32);
                let track_h = bounds.size.height - px(8.0);
                let thumb_h = ((rows as f32 / total_lines) * track_h.as_f32())
                    .clamp(20.0, track_h.as_f32());
                let max_off = term_state.max_scroll_offset.max(1) as f32;
                let ratio =
                    1.0 - (term_state.scroll_offset as f32 / max_off).clamp(0.0, 1.0);
                let thumb_y =
                    bounds.origin.y + px(4.0) + px((track_h.as_f32() - thumb_h) * ratio);
                let thumb_bounds = Bounds::new(
                    point(bounds.origin.x + bounds.size.width - px(6.0), thumb_y),
                    size(px(4.0), px(thumb_h)),
                );
                let mut thumb_color = palette.fg_hsla();
                thumb_color.a = if term_state.scroll_offset > 0 {
                    0.45
                } else {
                    0.18
                };
                window.paint_quad(
                    gpui::quad(
                        thumb_bounds,
                        Corners::all(px(2.0)),
                        thumb_color,
                        gpui::Edges::default(),
                        Hsla::default(),
                        BorderStyle::Solid,
                    ),
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cell(ch: char, width: u8, fg: Hsla, bold: bool) -> RenderCell {
        RenderCell {
            ch,
            fg,
            bg: gpui::hsla(0.0, 0.0, 0.05, 1.0),
            draw_bg: false,
            bold,
            italic: false,
            underline: false,
            double_underline: false,
            undercurl: false,
            dotted_underline: false,
            dashed_underline: false,
            hyperlink: false,
            strike: false,
            width,
            selected: false,
            search_match: None,
        }
    }

    #[test]
    fn computes_image_pixel_bounds_from_viewport_cells() {
        let bounds = image_cell_bounds(px(10.0), px(20.0), px(8.0), px(16.0), 2, 1, 4, 2);
        assert_eq!(bounds.origin.x, px(26.0));
        assert_eq!(bounds.origin.y, px(36.0));
        assert_eq!(bounds.size.width, px(32.0));
        assert_eq!(bounds.size.height, px(32.0));
    }

    #[test]
    fn generates_undercurl_and_fancy_underline_segments() {
        let wave = undercurl_segments(px(10.0), px(20.0), px(16.0), px(1.0));
        assert!(!wave.is_empty());
        assert_eq!(wave.first().unwrap().origin.x, px(10.0));

        let double = double_underline_bounds(px(10.0), px(30.0), px(24.0), px(1.0));
        assert_eq!(double.len(), 2);
        assert!(double[0].origin.y < double[1].origin.y);

        let dotted = dotted_underline_segments(px(0.0), px(20.0), px(12.0), px(1.0));
        assert!(!dotted.is_empty());

        let dashed = dashed_underline_segments(px(0.0), px(20.0), px(12.0), px(1.0));
        assert!(!dashed.is_empty());
    }

    #[test]
    fn minimum_contrast_adjusts_unreadable_fg_and_exempts_boxdraw_and_pua() {
        let dark_bg = gpui::hsla(0.6, 0.2, 0.06, 1.0);
        let dark_blue_fg = gpui::hsla(0.66, 0.9, 0.12, 1.0);
        assert!(contrast_ratio(dark_blue_fg, dark_bg) < MIN_CONTRAST_RATIO);

        let adjusted = ensure_minimum_contrast(dark_blue_fg, dark_bg, MIN_CONTRAST_RATIO);
        assert!(
            contrast_ratio(adjusted, dark_bg) >= MIN_CONTRAST_RATIO - 0.05,
            "expected contrast >= {MIN_CONTRAST_RATIO}, got {}",
            contrast_ratio(adjusted, dark_bg)
        );
        assert_eq!(adjusted.h, dark_blue_fg.h);

        // Already high-contrast colors remain untouched
        let readable_fg = gpui::hsla(0.3, 0.8, 0.75, 1.0);
        assert_eq!(
            ensure_minimum_contrast(readable_fg, dark_bg, MIN_CONTRAST_RATIO),
            readable_fg
        );

        // Box-drawing, block elements, spaces, and Nerd Font PUA glyphs are exempt
        assert!(is_contrast_exempt_glyph(' '));
        assert!(is_contrast_exempt_glyph('─'));
        assert!(is_contrast_exempt_glyph('█'));
        assert!(is_contrast_exempt_glyph('\u{E0B0}'));
        assert!(!is_contrast_exempt_glyph('A'));
        assert!(!is_contrast_exempt_glyph('⠋'));
    }

    #[test]
    fn merges_multi_style_narrow_runs_and_unicode_spinners_into_single_row_segment() {
        let red = gpui::hsla(0.0, 0.8, 0.6, 1.0);
        let white = gpui::hsla(0.0, 0.0, 0.9, 1.0);
        let mut row = Vec::new();
        // "⠋ err: ok" -> Unicode braille spinner (1 col) + space + bold red "err:" + space + white "ok" + trailing spaces
        row.push(test_cell('⠋', 1, red, false));
        row.push(test_cell(' ', 1, white, false));
        for ch in "err:".chars() {
            row.push(test_cell(ch, 1, red, true));
        }
        row.push(test_cell(' ', 1, white, false));
        for ch in "ok".chars() {
            row.push(test_cell(ch, 1, white, false));
        }
        for _ in 0..6 {
            row.push(test_cell(' ', 1, white, false));
        }

        let specs = build_row_segment_specs(&row);
        assert_eq!(
            specs.len(),
            1,
            "entire styled narrow line should coalesce into 1 shape_line segment"
        );
        assert_eq!(specs[0].start_col, 0);
        assert_eq!(specs[0].cell_span, 1);
        assert_eq!(specs[0].text.as_ref(), "⠋ err: ok");
        assert_eq!(specs[0].runs.len(), 3);
    }

    #[test]
    fn row_shape_cache_hits_on_identical_row_and_invalidates_on_metric_change() {
        let white = gpui::hsla(0.0, 0.0, 0.9, 1.0);
        let row = vec![
            test_cell('h', 1, white, false),
            test_cell('i', 1, white, false),
        ];
        let h = hash_row_cells(&row);
        let mut cache = RowShapeCache::default();
        let family = SharedString::new_static("JetBrains Mono");
        cache.prepare(&family, px(14.0), px(8.4));
        assert!(cache.get(h).is_none());
        assert_eq!(cache.misses, 1);

        cache.insert(h, Arc::new(Vec::new()));
        assert!(cache.get(h).is_some());
        assert_eq!(cache.hits, 1);

        // Changing font size invalidates cached entries
        cache.prepare(&family, px(16.0), px(9.6));
        assert_eq!(cache.len(), 0);
        assert!(cache.get(h).is_none());
    }
}
