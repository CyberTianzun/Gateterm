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
    TokyoNight,
    CatppuccinMocha,
    SolarizedDark,
}

impl TerminalPaletteId {
    pub const ALL: [Self; 5] = [
        Self::Gate2026Dark,
        Self::Gate2026Light,
        Self::TokyoNight,
        Self::CatppuccinMocha,
        Self::SolarizedDark,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Gate2026Dark => "2026 Dark",
            Self::Gate2026Light => "2026 Light",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::SolarizedDark => "Solarized Dark",
        }
    }

    pub fn is_dark(self) -> bool {
        !matches!(self, Self::Gate2026Light)
    }

    pub fn palette(self) -> TerminalPalette {
        match self {
            Self::Gate2026Dark => TerminalPalette {
                id: self,
                fg: 0x00E4_E7F0,
                bg: 0x0011_131A,
                cursor: 0x007A_A2F7,
                selection_bg: 0x002E_3C64,
                ansi: [
                    0x0015_1722, 0x00F7_768E, 0x009E_CE6A, 0x00E0_AF68,
                    0x007A_A2F7, 0x00BB_9AF7, 0x007D_CFFF, 0x00C0_CAF5,
                    0x0056_5F89, 0x00FF_899D, 0x00AF_D77A, 0x00EB_BC75,
                    0x008D_B0FF, 0x00C8_ACFF, 0x0093_D8FF, 0x00E4_E7F0,
                ],
            },
            Self::Gate2026Light => TerminalPalette {
                id: self,
                fg: 0x001E_2330,
                bg: 0x00F7_F8FB,
                cursor: 0x002E_5BFF,
                selection_bg: 0x00CF_DCFF,
                ansi: [
                    0x001E_2330, 0x00D1_2F4B, 0x0026_7F3E, 0x009A_6700,
                    0x0025_57D6, 0x007C_3AED, 0x000E_7490, 0x005B_6478,
                    0x006E_778C, 0x00E1_1D48, 0x0015_803D, 0x00B4_5309,
                    0x001D_4ED8, 0x006D_28D9, 0x0008_91B2, 0x0011_1827,
                ],
            },
            Self::TokyoNight => TerminalPalette {
                id: self,
                fg: 0x00C0_CAF5,
                bg: 0x001A_1B26,
                cursor: 0x00C0_CAF5,
                selection_bg: 0x0033_467C,
                ansi: [
                    0x0015_161E, 0x00F7_768E, 0x009E_CE6A, 0x00E0_AF68,
                    0x007A_A2F7, 0x00BB_9AF7, 0x007D_CFFF, 0x00A9_B1D6,
                    0x0041_4868, 0x00F7_768E, 0x009E_CE6A, 0x00E0_AF68,
                    0x007A_A2F7, 0x00BB_9AF7, 0x007D_CFFF, 0x00C0_CAF5,
                ],
            },
            Self::CatppuccinMocha => TerminalPalette {
                id: self,
                fg: 0x00CD_D6F4,
                bg: 0x001E_1E2E,
                cursor: 0x00F5_E0DC,
                selection_bg: 0x0045_475A,
                ansi: [
                    0x0045_475A, 0x00F3_8BA8, 0x00A6_E3A1, 0x00F9_E2AF,
                    0x0089_B4FA, 0x00F5_C2E7, 0x0094_E2D5, 0x00BA_C2DE,
                    0x0058_5B70, 0x00F3_8BA8, 0x00A6_E3A1, 0x00F9_E2AF,
                    0x0089_B4FA, 0x00F5_C2E7, 0x0094_E2D5, 0x00A6_ADC8,
                ],
            },
            Self::SolarizedDark => TerminalPalette {
                id: self,
                fg: 0x0083_9496,
                bg: 0x0000_2B36,
                cursor: 0x0093_A1A1,
                selection_bg: 0x0007_3642,
                ansi: [
                    0x0007_3642, 0x00DC_322F, 0x0085_9900, 0x00B5_8900,
                    0x0026_8BD2, 0x00D3_3682, 0x002A_A198, 0x00EE_E8D5,
                    0x0000_2B36, 0x00CB_4B16, 0x0058_6E75, 0x0065_7B83,
                    0x0083_9496, 0x006C_71C4, 0x0093_A1A1, 0x00FD_F6E3,
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
        c.a = 0.55;
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

pub fn apply_palette(id: TerminalPaletteId, cx: &mut App) {
    let palette = id.palette();
    cx.set_global(palette);
    let mode = if id.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, None, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_formats_term_colors_hex() {
        let p = TerminalPaletteId::TokyoNight.palette();
        let tc = p.term_colors();
        assert_eq!(tc.fg, "#c0caf5");
        assert_eq!(tc.bg, "#1a1b26");
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
