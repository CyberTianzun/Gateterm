//! GPU-rendered terminal element (`TerminalElement`) built on GPUI and `alacritty_terminal`
//! (`gateterm-client-core::term::ClientTerm`), inspired by `tty7`.
//!
//! Renders cell backgrounds, contiguous text runs with ligatures disabled,
//! wide characters (CJK/emoji), bold/italic/underline/strikethrough/inverse/dim styles,
//! viewport selection highlight, IME composition preedit, block/hollow cursor,
//! and scrollback indicator.

use gateterm_client_core::term::attr;
use gpui::{
    App, BorderStyle, Bounds, ContentMask, Corners, Element, ElementId, ElementInputHandler,
    Entity, Font, FontFeatures, FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior,
    Hsla, IntoElement, LayoutId, Pixels, SharedString, StrikethroughStyle, Style,
    TextAlign, TextRun, UnderlineStyle, Window, fill, outline, point, px, relative, size,
};

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
    strike: bool,
    width: u8,
    selected: bool,
    search_match: Option<bool>,
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

            (
                view.focus_handle.clone(),
                view.font_family.clone(),
                view.font_size,
                st,
                grid_cells,
                view.ime_marked_text.clone(),
                cur_rect,
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

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            // 1. Paint non-default cell backgrounds & selection runs
            let sel_bg = palette.selection_hsla();
            for y in 0..rows {
                let row_offset = y * cols;
                let row_y = origin.y + prepaint.line_height * (y as f32);

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
                    let rect = Bounds::new(
                        point(origin.x + prepaint.cell_width * (start as f32), row_y),
                        size(
                            prepaint.cell_width * ((x - start) as f32),
                            prepaint.line_height,
                        ),
                    );
                    window.paint_quad(fill(rect, bg));
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
                    let rect = Bounds::new(
                        point(origin.x + prepaint.cell_width * (start as f32), row_y),
                        size(
                            prepaint.cell_width * ((sx - start) as f32),
                            prepaint.line_height,
                        ),
                    );
                    window.paint_quad(fill(rect, sel_bg));
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
                    let rect = Bounds::new(
                        point(origin.x + prepaint.cell_width * (start as f32), row_y),
                        size(
                            prepaint.cell_width * ((mx - start) as f32),
                            prepaint.line_height,
                        ),
                    );
                    let match_color = if is_current {
                        gpui::hsla(0.11, 0.95, 0.55, 0.45)
                    } else {
                        gpui::hsla(0.14, 0.85, 0.50, 0.25)
                    };
                    window.paint_quad(fill(rect, match_color));
                }
            }

            // 2. Paint text runs row by row
            for y in 0..rows {
                let row_offset = y * cols;
                let row_y = origin.y + prepaint.line_height * (y as f32);
                let mut x = 0;

                while x < cols {
                    let cell = &cells[row_offset + x];
                    if cell.width == 0
                        || (cell.ch == ' ' && !cell.underline && !cell.strike)
                    {
                        x += 1;
                        continue;
                    }

                    let font_idx = (cell.bold as usize) | ((cell.italic as usize) << 1);
                    let font = fonts[font_idx].clone();
                    let underline = cell.underline.then_some(UnderlineStyle {
                        thickness: px(1.0),
                        color: Some(cell.fg),
                        wavy: false,
                    });
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
                                && c.strike == cell.strike
                            {
                                text.push(c.ch);
                                x += 1;
                            } else {
                                break;
                            }
                        }
                        if text.trim().is_empty() && !cell.underline && !cell.strike {
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
            }

            // 3. Paint cursor when at bottom of scrollback and cursor is visible
            if term_state.cursor_visible && term_state.scroll_offset == 0 {
                let cx_idx = (term_state.cursor_x as usize).min(cols.saturating_sub(1));
                let cy_idx = (term_state.cursor_y as usize).min(rows.saturating_sub(1));
                let cur_cell = &cells[cy_idx * cols + cx_idx];
                let cur_w = prepaint.cell_width * (cur_cell.width.max(1) as f32);
                let cur_rect = Bounds::new(
                    point(
                        origin.x + prepaint.cell_width * (cx_idx as f32),
                        origin.y + prepaint.line_height * (cy_idx as f32),
                    ),
                    size(cur_w, prepaint.line_height),
                );
                let cursor_color = palette.cursor_hsla();

                if focused {
                    window.paint_quad(fill(cur_rect, cursor_color));
                    if cur_cell.ch != ' ' {
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
