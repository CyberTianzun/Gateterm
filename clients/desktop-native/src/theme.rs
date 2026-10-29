//! Terminal palettes & UI color derivation aligned 1:1 with
//! `clients/web/src/themes/builtin.ts`, `clients/web/src/appearance.ts`,
//! and `clients/android/.../appearance/UiPalette.kt`.

use gateterm_protocol::TermColors;
use gpui::{App, Global, Hsla, Rgba, rgb};
use gpui_component::{Theme, ThemeMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalPaletteId {
    #[default]
    Gate2026Dark,
    Gate2026Light,
    JetBrainsIslandsDark,
    TokyoNight,
    Dracula,
    Nord,
    GruvboxDark,
    CatppuccinMocha,
    SolarizedDark,
    SolarizedLight,
    OneHalfLight,
}

impl TerminalPaletteId {
    pub const ALL: [Self; 11] = [
        Self::Gate2026Dark,
        Self::Gate2026Light,
        Self::JetBrainsIslandsDark,
        Self::TokyoNight,
        Self::Dracula,
        Self::Nord,
        Self::GruvboxDark,
        Self::CatppuccinMocha,
        Self::SolarizedDark,
        Self::SolarizedLight,
        Self::OneHalfLight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Gate2026Dark => "2026 Dark",
            Self::Gate2026Light => "2026 Light",
            Self::JetBrainsIslandsDark => "JetBrains Islands Dark",
            Self::TokyoNight => "TokyoNight",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::SolarizedDark => "iTerm2 Solarized Dark",
            Self::SolarizedLight => "iTerm2 Solarized Light",
            Self::OneHalfLight => "One Half Light",
        }
    }

    pub fn is_dark(self) -> bool {
        !is_light_rgb(self.palette().bg)
    }

    pub fn palette(self) -> TerminalPalette {
        match self {
            // Exact match with clients/web/src/themes/builtin.ts: "2026 Dark"
            Self::Gate2026Dark => TerminalPalette {
                id: self,
                fg: 0x00CC_CCCC,
                bg: 0x0019_1A1B,
                cursor: 0x00BF_BFBF,
                selection_bg: 0x0039_94BC,
                selection_alpha: 0x33 as f32 / 255.0,
                ansi: [
                    0x0000_0000, 0x00CD_3131, 0x000D_BC79, 0x00E5_E510,
                    0x0024_72C8, 0x00BC_3FBC, 0x0011_A8CD, 0x00E5_E5E5,
                    0x0066_6666, 0x00F1_4C4C, 0x0023_D18B, 0x00F5_F543,
                    0x003B_8EEA, 0x00D6_70D6, 0x0029_B8DB, 0x00E5_E5E5,
                ],
            },
            // Exact match with clients/web/src/themes/builtin.ts: "2026 Light"
            Self::Gate2026Light => TerminalPalette {
                id: self,
                fg: 0x003B_3B3B,
                bg: 0x00FA_FAFD,
                cursor: 0x0020_2020,
                selection_bg: 0x0000_69CC,
                selection_alpha: 0x26 as f32 / 255.0,
                ansi: [
                    0x0000_0000, 0x00CD_3131, 0x0010_7C10, 0x0094_9800,
                    0x0004_51A5, 0x00BC_05BC, 0x0005_98BC, 0x0055_5555,
                    0x0066_6666, 0x00CD_3131, 0x0014_CE14, 0x00B5_BA00,
                    0x0004_51A5, 0x00BC_05BC, 0x0005_98BC, 0x00A5_A5A5,
                ],
            },
            Self::JetBrainsIslandsDark => TerminalPalette {
                id: self,
                fg: 0x00BC_BEC4,
                bg: 0x0019_1A1C,
                cursor: 0x00CE_D0D6,
                selection_bg: 0x002A_4371,
                selection_alpha: 0.55,
                ansi: [
                    0x0019_1A1C, 0x00F7_5464, 0x006A_AB73, 0x00CF_8E6D,
                    0x0056_A8F5, 0x00C7_7DBB, 0x002A_ACB8, 0x00BC_BEC4,
                    0x007A_7E85, 0x00F5_7E84, 0x006D_B083, 0x00F0_AC81,
                    0x0054_8AF7, 0x00B1_89F5, 0x0016_BAAC, 0x00FF_FFFF,
                ],
            },
            Self::TokyoNight => TerminalPalette {
                id: self,
                fg: 0x00C0_CAF5,
                bg: 0x001A_1B26,
                cursor: 0x00C0_CAF5,
                selection_bg: 0x0033_467C,
                selection_alpha: 0.55,
                ansi: [
                    0x0015_161E, 0x00F7_768E, 0x009E_CE6A, 0x00E0_AF68,
                    0x007A_A2F7, 0x00BB_9AF7, 0x007D_CFFF, 0x00A9_B1D6,
                    0x0041_4868, 0x00F7_768E, 0x009E_CE6A, 0x00E0_AF68,
                    0x007A_A2F7, 0x00BB_9AF7, 0x007D_CFFF, 0x00C0_CAF5,
                ],
            },
            Self::Dracula => TerminalPalette {
                id: self,
                fg: 0x00F8_F8F2,
                bg: 0x0028_2A36,
                cursor: 0x00F8_F8F2,
                selection_bg: 0x0044_475A,
                selection_alpha: 0.55,
                ansi: [
                    0x0021_222C, 0x00FF_5555, 0x0050_FA7B, 0x00F1_FA8C,
                    0x00BD_93F9, 0x00FF_79C6, 0x008B_E9FD, 0x00F8_F8F2,
                    0x0062_72A4, 0x00FF_6E6E, 0x0069_FF94, 0x00FF_FFA5,
                    0x00D6_ACFF, 0x00FF_92DF, 0x00A4_FFFF, 0x00FF_FFFF,
                ],
            },
            Self::Nord => TerminalPalette {
                id: self,
                fg: 0x00D8_DEE9,
                bg: 0x002E_3440,
                cursor: 0x00EC_EFF4,
                selection_bg: 0x0043_4C5E,
                selection_alpha: 0.55,
                ansi: [
                    0x003B_4252, 0x00BF_616A, 0x00A3_BE8C, 0x00EB_CB8B,
                    0x0081_A1C1, 0x00B4_8EAD, 0x0088_C0D0, 0x00E5_E9F0,
                    0x0059_6377, 0x00BF_616A, 0x00A3_BE8C, 0x00EB_CB8B,
                    0x0081_A1C1, 0x00B4_8EAD, 0x008F_BCBB, 0x00EC_EFF4,
                ],
            },
            Self::GruvboxDark => TerminalPalette {
                id: self,
                fg: 0x00EB_DBB2,
                bg: 0x0028_2828,
                cursor: 0x00EB_DBB2,
                selection_bg: 0x0066_5C54,
                selection_alpha: 0.55,
                ansi: [
                    0x0028_2828, 0x00CC_241D, 0x0098_971A, 0x00D7_9921,
                    0x0045_8588, 0x00B1_6286, 0x0068_9D6A, 0x00A8_9984,
                    0x0092_8374, 0x00FB_4934, 0x00B8_BB26, 0x00FA_BD2F,
                    0x0083_A598, 0x00D3_869B, 0x008E_C07C, 0x00EB_DBB2,
                ],
            },
            Self::CatppuccinMocha => TerminalPalette {
                id: self,
                fg: 0x00CD_D6F4,
                bg: 0x001E_1E2E,
                cursor: 0x00F5_E0DC,
                selection_bg: 0x0045_475A,
                selection_alpha: 0.55,
                ansi: [
                    0x0045_475A, 0x00F3_8BA8, 0x00A6_E3A1, 0x00F9_E2AF,
                    0x0089_B4FA, 0x00F5_C2E7, 0x0094_E2D5, 0x00BA_C2DE,
                    0x0058_5B70, 0x00F7_AEC2, 0x00C2_ECBF, 0x00FC_D682,
                    0x00AE_CCFC, 0x00F3_98DA, 0x00B1_EAE1, 0x00A6_ADC8,
                ],
            },
            Self::SolarizedDark => TerminalPalette {
                id: self,
                fg: 0x0083_9496,
                bg: 0x0000_2B36,
                cursor: 0x0083_9496,
                selection_bg: 0x0007_3642,
                selection_alpha: 0.65,
                ansi: [
                    0x0007_3642, 0x00DC_322F, 0x0085_9900, 0x00B5_8900,
                    0x0026_8BD2, 0x00D3_3682, 0x002A_A198, 0x00EE_E8D5,
                    0x0033_5E69, 0x00CB_4B16, 0x0058_6E75, 0x0065_7B83,
                    0x0083_9496, 0x006C_71C4, 0x0093_A1A1, 0x00FD_F6E3,
                ],
            },
            Self::SolarizedLight => TerminalPalette {
                id: self,
                fg: 0x0065_7B83,
                bg: 0x00FD_F6E3,
                cursor: 0x0065_7B83,
                selection_bg: 0x00EE_E8D5,
                selection_alpha: 0.65,
                ansi: [
                    0x0007_3642, 0x00DC_322F, 0x0085_9900, 0x00B5_8900,
                    0x0026_8BD2, 0x00D3_3682, 0x002A_A198, 0x00BB_B5A2,
                    0x0000_2B36, 0x00CB_4B16, 0x0058_6E75, 0x0065_7B83,
                    0x0083_9496, 0x006C_71C4, 0x0093_A1A1, 0x00FD_F6E3,
                ],
            },
            Self::OneHalfLight => TerminalPalette {
                id: self,
                fg: 0x0038_3A42,
                bg: 0x00FA_FAFA,
                cursor: 0x00A5_B4E5,
                selection_bg: 0x00BF_CEFF,
                selection_alpha: 0.55,
                ansi: [
                    0x0038_3A42, 0x00E4_5649, 0x0050_A14F, 0x00C1_8401,
                    0x0001_84BC, 0x00A6_26A4, 0x0009_97B3, 0x00BA_BABA,
                    0x004F_525E, 0x00E0_6C75, 0x0098_C379, 0x00D8_B36E,
                    0x0061_AFEF, 0x00C6_78DD, 0x0056_B6C2, 0x00FF_FFFF,
                ],
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TerminalPalette {
    pub id: TerminalPaletteId,
    pub fg: i32,
    pub bg: i32,
    pub cursor: i32,
    pub selection_bg: i32,
    pub selection_alpha: f32,
    pub ansi: [i32; 16],
}

impl Global for TerminalPalette {}

impl TerminalPalette {
    pub fn get(id: TerminalPaletteId) -> Self {
        id.palette()
    }

    pub fn term_colors(&self) -> TermColors {
        TermColors {
            fg: format!("#{:06x}", self.fg & 0x00FF_FFFF),
            bg: format!("#{:06x}", self.bg & 0x00FF_FFFF),
        }
    }

    pub fn to_term_colors(&self) -> TermColors {
        self.term_colors()
    }

    pub fn fg_hsla(&self) -> Hsla {
        int_to_hsla(self.fg)
    }

    pub fn bg_hsla(&self) -> Hsla {
        int_to_hsla(self.bg)
    }

    pub fn cursor_hsla(&self) -> Hsla {
        int_to_hsla(self.cursor)
    }

    pub fn selection_hsla(&self) -> Hsla {
        let mut c = int_to_hsla(self.selection_bg);
        c.a = self.selection_alpha;
        c
    }

    pub fn resolve_cell_color(&self, rgb_val: i32, index: i32, is_fg: bool) -> Hsla {
        if index == gateterm_client_core::term::COLOR_INDEX_DEFAULT {
            if is_fg {
                self.fg_hsla()
            } else {
                self.bg_hsla()
            }
        } else if (0..16).contains(&index) {
            int_to_hsla(self.ansi[index as usize])
        } else {
            int_to_hsla(rgb_val)
        }
    }
}

pub fn int_to_hsla(color: i32) -> Hsla {
    rgb((color as u32) & 0x00FF_FFFF).into()
}

pub fn rgba_with_alpha(color: i32, alpha: f32) -> Hsla {
    let r = ((color >> 16) & 0xFF) as f32 / 255.0;
    let g = ((color >> 8) & 0xFF) as f32 / 255.0;
    let b = (color & 0xFF) as f32 / 255.0;
    Rgba { r, g, b, a: alpha }.into()
}

fn channel(rgb_val: i32, shift: u32) -> f64 {
    ((rgb_val >> shift) & 0xFF) as f64
}

/// Same relative luminance formula as `clients/web/src/appearance.ts` (`luminance`).
pub fn is_light_rgb(rgb_val: i32) -> bool {
    (0.2126 * channel(rgb_val, 16) + 0.7152 * channel(rgb_val, 8) + 0.0722 * channel(rgb_val, 0))
        / 255.0
        > 0.5
}

/// Linear RGB channel mix: `t = 0.0` is `b`, `t = 1.0` is `a`, matching `appearance.ts` / `UiPalette.kt`.
pub fn mix_rgb(a: i32, b: i32, t: f64) -> i32 {
    let mut out = 0i32;
    for shift in [16u32, 8, 0] {
        let ca = channel(a, shift);
        let cb = channel(b, shift);
        let mixed = (cb + (ca - cb) * t).round().clamp(0.0, 255.0) as i32;
        out |= mixed << shift;
    }
    out
}

fn wcag_luminance(rgb_val: i32) -> f64 {
    let lin = |shift: u32| {
        let s = channel(rgb_val, shift) / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(16) + 0.7152 * lin(8) + 0.0722 * lin(0)
}

pub fn contrast_ratio(first: i32, second: i32) -> f64 {
    let a = wcag_luminance(first);
    let b = wcag_luminance(second);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Same `readableAccent` logic as `clients/web/src/appearance.ts` and `UiPalette.kt`.
pub fn readable_accent(normal: i32, bright: i32, bg: i32) -> i32 {
    if contrast_ratio(normal, bg) >= 3.0 {
        normal
    } else if contrast_ratio(bright, bg) > contrast_ratio(normal, bg) {
        bright
    } else {
        normal
    }
}

/// Applies the terminal palette AND derives all UI chrome colors (`--bg`, `--panel`, `--panel-2`,
/// `--line`, `--text`, `--dim`, `--accent`, `--danger`) using the exact same formulas as
/// `clients/web/src/appearance.ts` (`deriveUiColors`) and `clients/android/.../UiPalette.kt`.
pub fn apply_palette(id: TerminalPaletteId, cx: &mut App) {
    let palette = id.palette();
    cx.set_global(palette);

    let light = is_light_rgb(palette.bg);
    let mode = if light {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    Theme::change(mode, None, cx);

    let bg_rgb = palette.bg;
    let fg_rgb = palette.fg;
    let panel_rgb = mix_rgb(fg_rgb, bg_rgb, if light { 0.05 } else { 0.07 });
    let panel2_rgb = mix_rgb(fg_rgb, bg_rgb, if light { 0.11 } else { 0.14 });
    let hover_rgb = mix_rgb(fg_rgb, bg_rgb, if light { 0.16 } else { 0.20 });
    let line_rgb = mix_rgb(fg_rgb, bg_rgb, if light { 0.20 } else { 0.22 });
    let dim_rgb = mix_rgb(fg_rgb, bg_rgb, 0.55);
    let accent_rgb = readable_accent(palette.ansi[4], palette.ansi[12], bg_rgb);
    let danger_rgb = readable_accent(palette.ansi[1], palette.ansi[9], bg_rgb);
    let success_rgb = readable_accent(palette.ansi[2], palette.ansi[10], bg_rgb);
    let warning_rgb = readable_accent(palette.ansi[3], palette.ansi[11], bg_rgb);

    let bg = int_to_hsla(bg_rgb);
    let fg = int_to_hsla(fg_rgb);
    let panel = int_to_hsla(panel_rgb);
    let panel2 = int_to_hsla(panel2_rgb);
    let hover = int_to_hsla(hover_rgb);
    let line = int_to_hsla(line_rgb);
    let dim = int_to_hsla(dim_rgb);
    let accent = int_to_hsla(accent_rgb);
    let accent_hover = int_to_hsla(mix_rgb(fg_rgb, accent_rgb, 0.14));
    let accent_active = int_to_hsla(mix_rgb(bg_rgb, accent_rgb, 0.14));
    let accent_fg = if is_light_rgb(accent_rgb) {
        int_to_hsla(0x111111)
    } else {
        int_to_hsla(0xFFFFFF)
    };
    let danger = int_to_hsla(danger_rgb);
    let success = int_to_hsla(success_rgb);
    let warning = int_to_hsla(warning_rgb);

    let theme = Theme::global_mut(cx);
    theme.background = bg;
    theme.foreground = fg;
    theme.sidebar = panel;
    theme.sidebar_foreground = fg;
    theme.sidebar_border = line;
    theme.sidebar_accent = panel2;
    theme.sidebar_accent_foreground = fg;
    theme.sidebar_primary = accent;
    theme.sidebar_primary_foreground = accent_fg;
    theme.title_bar = panel;
    theme.title_bar_border = line;
    theme.tab_bar = panel;
    theme.tab_bar_segmented = panel2;
    theme.tab = panel;
    theme.tab_active = bg;
    theme.tab_foreground = dim;
    theme.tab_active_foreground = fg;
    theme.popover = panel;
    theme.popover_foreground = fg;
    theme.group_box = panel;
    theme.group_box_foreground = fg;
    theme.secondary = panel2;
    theme.secondary_hover = hover;
    theme.secondary_active = line;
    theme.secondary_foreground = fg;
    theme.muted = panel2;
    theme.muted_foreground = dim;
    theme.border = line;
    theme.window_border = line;
    theme.input = line;
    theme.primary = accent;
    theme.primary_hover = accent_hover;
    theme.primary_active = accent_active;
    theme.primary_foreground = accent_fg;
    theme.button_primary = accent;
    theme.button_primary_hover = accent_hover;
    theme.button_primary_active = accent_active;
    theme.button_primary_foreground = accent_fg;
    theme.accent = panel2;
    theme.accent_foreground = fg;
    theme.ring = accent;
    theme.link = accent;
    theme.link_hover = accent_hover;
    theme.link_active = accent_active;
    theme.selection = palette.selection_hsla();
    theme.caret = palette.cursor_hsla();
    theme.danger = danger;
    theme.danger_hover = int_to_hsla(mix_rgb(fg_rgb, danger_rgb, 0.14));
    theme.danger_active = int_to_hsla(mix_rgb(bg_rgb, danger_rgb, 0.14));
    theme.success = success;
    theme.warning = warning;
    theme.colors.list = bg;
    theme.list_head = panel;
    theme.list_hover = panel2;
    theme.list_active = palette.selection_hsla();
    theme.list_active_border = accent;
    theme.table = bg;
    theme.table_head = panel;
    theme.table_hover = panel2;
    theme.table_row_border = line;
    theme.scrollbar_thumb = int_to_hsla(mix_rgb(fg_rgb, bg_rgb, 0.30));
    theme.scrollbar_thumb_hover = int_to_hsla(mix_rgb(fg_rgb, bg_rgb, 0.45));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_formats_term_colors_hex() {
        let p = TerminalPaletteId::Gate2026Dark.palette();
        let tc = p.term_colors();
        assert_eq!(tc.fg, "#cccccc");
        assert_eq!(tc.bg, "#191a1b");

        let tn = TerminalPaletteId::TokyoNight.palette();
        let tn_tc = tn.term_colors();
        assert_eq!(tn_tc.fg, "#c0caf5");
        assert_eq!(tn_tc.bg, "#1a1b26");
    }

    #[test]
    fn ui_derivation_matches_web_appearance_ts() {
        let p = TerminalPaletteId::Gate2026Dark.palette();
        assert!(!is_light_rgb(p.bg));
        // mix(#cccccc, #191a1b, 0.07) -> #262627
        assert_eq!(mix_rgb(p.fg, p.bg, 0.07), 0x262627);
        // mix(#cccccc, #191a1b, 0.14) -> #323334
        assert_eq!(mix_rgb(p.fg, p.bg, 0.14), 0x323334);
        // mix(#cccccc, #191a1b, 0.22) -> #404142
        assert_eq!(mix_rgb(p.fg, p.bg, 0.22), 0x404142);
        // readableAccent(#2472c8, #3b8eea, #191a1b) -> #2472c8 (contrast >= 3)
        assert_eq!(readable_accent(p.ansi[4], p.ansi[12], p.bg), 0x2472C8);
    }

    #[test]
    fn palette_resolves_default_and_ansi_colors() {
        let p = TerminalPaletteId::Gate2026Dark.palette();
        let fg = p.resolve_cell_color(0, gateterm_client_core::term::COLOR_INDEX_DEFAULT, true);
        assert_eq!(fg, p.fg_hsla());
        let ansi1 = p.resolve_cell_color(0, 1, true);
        assert_eq!(ansi1, int_to_hsla(p.ansi[1]));
    }
}
