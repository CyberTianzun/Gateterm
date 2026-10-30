//! GPU-rendered terminal element (`TerminalElement`) built on GPUI and `alacritty_terminal`
//! (`gateterm-client-core::term::ClientTerm`), inspired by `tty7`.
//!
//! Renders cell backgrounds, contiguous text runs with ligatures disabled,
//! wide characters (CJK/emoji), bold/italic/underline/strikethrough/inverse/dim styles,
//! viewport selection highlight, IME composition preedit, block/hollow cursor,
//! and scrollback indicator.

use std::sync::Arc;

use gateterm_client_core::graphics::ViewportImage;
use gateterm_client_core::term::attr;
use gpui::{
    App, BorderStyle, Bounds, ContentMask, Corners, Element, ElementId, ElementInputHandler,
    Entity, Font, FontFeatures, FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior,
    Hsla, IntoElement, LayoutId, Pixels, RenderImage, SharedString, StrikethroughStyle, Style,
    TextAlign, TextRun, UnderlineStyle, Window, fill, outline, point, px, relative, size,
};

use super::boxdraw;
use super::view::TerminalView;
use crate::theme::TerminalPalette;

const DIM_ALPHA: f32 = 0.65;

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

#[derive(Clone)]
struct RenderCell {
    ch: char,
    fg: Hsla,
    bg: Hsla,
    draw_bg: bool,
    bold: bool,
    italic: bool,
    underline: bool,
    double_underline: bool,
    undercurl: bool,
    dotted_underline: bool,
    dashed_underline: bool,
    hyperlink: bool,
    strike: bool,
    width: u8,
    selected: bool,
    search_match: Option<bool>,
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
                    if hidden {
                        fg = bg;
                    }
                    if (c.attrs & attr::DIM) != 0 {
                        fg.a *= DIM_ALPHA;
                    }
                    let ch = char::from_u32(c.codepoint).unwrap_or(' ');
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

        let scale = window.scale_factor().max(1.0);
        let snap_px = |v: f32| -> f32 { (v * scale).round() / scale };
        let span_rect = |start_col: usize, end_col: usize, row: usize| -> Bounds<Pixels> {
            let x0 = snap_px(origin.x.as_f32() + prepaint.cell_width.as_f32() * (start_col as f32));
            let x1 = snap_px(origin.x.as_f32() + prepaint.cell_width.as_f32() * (end_col as f32));
            let y0 = snap_px(origin.y.as_f32() + prepaint.line_height.as_f32() * (row as f32));
            let y1 = snap_px(origin.y.as_f32() + prepaint.line_height.as_f32() * ((row + 1) as f32));
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
                    while sx < cols && (cells[row_offset + sx].selected || cells[row_offset + sx].width == 0) {
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

            // 2. Paint procedural box-drawing/block glyphs, text runs & fancy underlines row by row
            for y in 0..rows {
                let row_offset = y * cols;
                let row_y = origin.y + prepaint.line_height * (y as f32);
                let row_bottom = row_y + prepaint.line_height;
                let mut x = 0;

                while x < cols {
                    let cell = &cells[row_offset + x];
                    if cell.width == 0
                        || (cell.ch == ' ' && !cell.underline && !cell.undercurl && !cell.strike)
                    {
                        x += 1;
                        continue;
                    }

                    // Procedural pixel-snapped Box-Drawing (U+2500..=U+257F) & Block Elements (U+2580..=U+259F)
                    if boxdraw::is_box_or_block(cell.ch) {
                        let span_cols = (cell.width.max(1)) as usize;
                        let cell_rect = span_rect(x, x + span_cols, y);
                        boxdraw::paint_box_or_block(cell.ch, cell_rect, cell.fg, scale, window);
                        x += 1;
                        continue;
                    }

                    let font_idx = (cell.bold as usize) | ((cell.italic as usize) << 1);
                    let font = fonts[font_idx].clone();
                    let underline = if cell.undercurl {
                        Some(UnderlineStyle {
                            thickness: px(1.0),
                            color: Some(cell.fg),
                            wavy: true,
                        })
                    } else if cell.underline
                        && !cell.double_underline
                        && !cell.dotted_underline
                        && !cell.dashed_underline
                    {
                        Some(UnderlineStyle {
                            thickness: px(1.0),
                            color: Some(cell.fg),
                            wavy: false,
                        })
                    } else {
                        None
                    };
                    let strikethrough = cell.strike.then_some(StrikethroughStyle {
                        thickness: px(1.0),
                        color: Some(cell.fg),
                    });

                    // Batch contiguous single-width ASCII cells with identical style
                    if cell.width == 1 && cell.ch.is_ascii() {
                        let start = x;
                        let mut text = String::new();
                        while x < cols {
                            let c = &cells[row_offset + x];
                            if c.width == 1
                                && c.ch.is_ascii()
                                && c.fg == cell.fg
                                && c.bold == cell.bold
                                && c.italic == cell.italic
                                && c.underline == cell.underline
                                && c.double_underline == cell.double_underline
                                && c.undercurl == cell.undercurl
                                && c.dotted_underline == cell.dotted_underline
                                && c.dashed_underline == cell.dashed_underline
                                && c.strike == cell.strike
                            {
                                text.push(c.ch);
                                x += 1;
                            } else {
                                break;
                            }
                        }
                        if text.trim().is_empty()
                            && !cell.underline
                            && !cell.undercurl
                            && !cell.strike
                        {
                            continue;
                        }
                        let shared = SharedString::from(text);
                        let run = [TextRun {
                            len: shared.len(),
                            font,
                            color: cell.fg,
                            background_color: None,
                            underline,
                            strikethrough,
                        }];
                        let shaped = window.text_system().shape_line(
                            shared,
                            font_size,
                            &run,
                            Some(prepaint.cell_width),
                        );
                        let _ = shaped.paint(
                            point(origin.x + prepaint.cell_width * (start as f32), row_y),
                            prepaint.line_height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    } else {
                        // Wide or non-ASCII character
                        let span_cols = (cell.width.max(1)) as f32;
                        let mut s = String::with_capacity(4);
                        s.push(cell.ch);
                        let shared = SharedString::from(s);
                        let run = [TextRun {
                            len: shared.len(),
                            font,
                            color: cell.fg,
                            background_color: None,
                            underline,
                            strikethrough,
                        }];
                        let shaped = window.text_system().shape_line(
                            shared,
                            font_size,
                            &run,
                            Some(prepaint.cell_width * span_cols),
                        );
                        let _ = shaped.paint(
                            point(origin.x + prepaint.cell_width * (x as f32), row_y),
                            prepaint.line_height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                        x += 1;
                    }
                }

                // Fancy underline styles (DOUBLE_UNDERLINE, UNDERCURL on spaces, DOTTED, DASHED, HYPERLINK)
                let mut ux = 0;
                while ux < cols {
                    let c = &cells[row_offset + ux];
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
                        let cn = &cells[row_offset + ux];
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
}
