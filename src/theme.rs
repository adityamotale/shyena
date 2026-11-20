//! Colour tokens for the launcher UI.
//!
//! The UI renders a fixed set of semantic roles (accent, chrome, text states),
//! not an arbitrary palette. Keeping only those roles as data makes a theme
//! switch a single match and leaves no unused accessor API to maintain.

use gpui::{Rgba, rgb, rgba};

/// Colours the launcher actually draws, named by role rather than by hue so UI
/// code never has to know which Catppuccin shade a theme happens to use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Primary action colour: cursor, prompt, `LAUNCH`/`APP` badge.
    pub accent: Rgba,
    /// PowerShell action colour: `EXEC`/`PS` badge.
    pub ps: Rgba,
    /// In-process command colour: `SYSTEM`/`SYS` badge.
    pub internal: Rgba,
    /// Errors and the error-panel border.
    pub error: Rgba,
    /// Primary foreground text.
    pub text: Rgba,
    /// Secondary text for candidate details; tuned per theme for readability.
    pub detail_text: Rgba,
    /// Low-emphasis key hints in the header.
    pub hint: Rgba,
    /// Placeholder text in the empty input field.
    pub placeholder: Rgba,
    /// Window background.
    pub bg: Rgba,
    /// Header strip background.
    pub header: Rgba,
    /// Window border and card outline.
    pub frame: Rgba,
    /// ASCII bracket glyphs in the header.
    pub bracket: Rgba,
    /// Hairline separators.
    pub border_subtle: Rgba,
    /// Selected-row and mode-tag background.
    pub surface0: Rgba,
    /// Mode-tag and badge border.
    pub surface1: Rgba,
}

/// Runtime theme selection, backed by `Set-Theme` in `.shyena`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Macchiato,
    Latte,
    EverforestLight,
    EverforestDark,
}

impl Theme {
    /// Resolves the token set for this theme. The result is copied once per
    /// frame; `Palette` is small enough that this stays cheaper than indirection.
    #[inline]
    pub fn palette(self) -> Palette {
        match self {
            Self::Macchiato => Palette {
                accent: rgb(0x00a6_da95), // Catppuccin green
                ps: rgb(0x00f5_a97f),
                internal: rgb(0x008a_adf4),
                error: rgb(0x00ed_8796),
                text: rgb(0x00ca_d3f5),
                detail_text: rgb(0x00a5_adcb), // subtext0
                hint: rgb(0x00a5_adcb),
                placeholder: rgb(0x006e_738d), // overlay0
                bg: rgb(0x001b_1d2b),
                header: rgb(0x0020_2233),
                frame: rgba(0xa6da_9580),
                bracket: rgba(0xa6da_95cc),
                border_subtle: rgba(0x363a_4f99),
                surface0: rgb(0x0036_3a4f),
                surface1: rgb(0x0049_4d64),
            },
            Self::Latte => Palette {
                accent: rgb(0x001e_66f5), // Catppuccin blue
                ps: rgb(0x00fe_640b),
                internal: rgb(0x001e_66f5),
                error: rgb(0x00d2_0f39),
                text: rgb(0x004c_4f69),
                detail_text: rgb(0x005c_5f77), // subtext1
                hint: rgb(0x006c_6f85),        // subtext0
                placeholder: rgb(0x009c_a0b0), // overlay0
                bg: rgb(0x00ef_f1f5),
                header: rgb(0x00e6_e9ef),
                frame: rgba(0x1e66_f580),
                bracket: rgba(0x1e66_f5cc),
                border_subtle: rgba(0xacb0_be88),
                surface0: rgb(0x00cc_d0da),
                surface1: rgb(0x00bc_c0cc),
            },
            Self::EverforestLight => Palette {
                accent: rgb(0x008d_a101),
                ps: rgb(0x00f5_7d26),
                internal: rgb(0x003a_94c5),
                error: rgb(0x00f8_5552),
                text: rgb(0x005c_6a72),
                detail_text: rgb(0x0082_9181),
                hint: rgb(0x0093_9f91),
                placeholder: rgb(0x00a6_b0a0),
                bg: rgb(0x00fd_f6e3),
                header: rgb(0x00f4_f0d9),
                frame: rgba(0x8da1_0180),
                bracket: rgba(0x8da1_01cc),
                border_subtle: rgba(0xbdc3_af88),
                surface0: rgb(0x00ef_ebd4),
                surface1: rgb(0x00e6_e2cc),
            },
            Self::EverforestDark => Palette {
                accent: rgb(0x00a7_c080),
                ps: rgb(0x00e6_9875),
                internal: rgb(0x007f_bbb3),
                error: rgb(0x00e6_7e80),
                text: rgb(0x00d3_c6aa),
                detail_text: rgb(0x009d_a9a0),
                hint: rgb(0x0085_9289),
                placeholder: rgb(0x007a_8478),
                bg: rgb(0x002d_353b),
                header: rgb(0x0023_2a2e),
                frame: rgba(0xa7c0_8080),
                bracket: rgba(0xa7c0_80cc),
                border_subtle: rgba(0x4752_5899),
                surface0: rgb(0x0034_3f44),
                surface1: rgb(0x003d_484d),
            },
        }
    }

    /// Parses the `Set-Theme` directive value; unknown values are rejected by
    /// the config parser rather than silently falling back to the default.
    #[inline]
    pub fn from_config_str(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("macchiato") || value.eq_ignore_ascii_case("dark") {
            Some(Self::Macchiato)
        } else if value.eq_ignore_ascii_case("latte") || value.eq_ignore_ascii_case("light") {
            Some(Self::Latte)
        } else if value.eq_ignore_ascii_case("everforest-light") {
            Some(Self::EverforestLight)
        } else if value.eq_ignore_ascii_case("everforest-dark")
            || value.eq_ignore_ascii_case("everforest")
        {
            Some(Self::EverforestDark)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_strings_are_case_insensitive() {
        assert_eq!(Theme::default(), Theme::Macchiato);
        assert_eq!(Theme::from_config_str("dark"), Some(Theme::Macchiato));
        assert_eq!(Theme::from_config_str("LIGHT"), Some(Theme::Latte));
        assert_eq!(Theme::from_config_str("macchiato"), Some(Theme::Macchiato));
        assert_eq!(Theme::from_config_str("LATTE"), Some(Theme::Latte));
        assert_eq!(
            Theme::from_config_str("everforest-light"),
            Some(Theme::EverforestLight)
        );
        assert_eq!(
            Theme::from_config_str("Everforest-Dark"),
            Some(Theme::EverforestDark)
        );
        assert_eq!(
            Theme::from_config_str("everforest"),
            Some(Theme::EverforestDark)
        );
        assert_eq!(Theme::from_config_str("mocha"), None);
    }

    #[test]
    fn each_theme_selects_its_own_palette() {
        let macchiato = Theme::Macchiato.palette();
        let latte = Theme::Latte.palette();
        let ef_light = Theme::EverforestLight.palette();
        let ef_dark = Theme::EverforestDark.palette();

        assert_eq!(macchiato.accent, rgb(0x00a6_da95));
        assert_eq!(macchiato.detail_text, rgb(0x00a5_adcb));
        assert_eq!(macchiato.hint, rgb(0x00a5_adcb));
        assert_eq!(macchiato.placeholder, rgb(0x006e_738d));
        assert_eq!(macchiato.bg, rgb(0x001b_1d2b));
        assert_eq!(macchiato.header, rgb(0x0020_2233));

        assert_eq!(latte.accent, rgb(0x001e_66f5));
        assert_eq!(latte.detail_text, rgb(0x005c_5f77));
        assert_eq!(latte.hint, rgb(0x006c_6f85));
        assert_eq!(latte.placeholder, rgb(0x009c_a0b0));
        assert_eq!(latte.bg, rgb(0x00ef_f1f5));
        assert_eq!(latte.header, rgb(0x00e6_e9ef));

        assert_eq!(ef_light.accent, rgb(0x008d_a101));
        assert_eq!(ef_light.bg, rgb(0x00fd_f6e3));
        assert_eq!(ef_dark.accent, rgb(0x00a7_c080));
        assert_eq!(ef_dark.bg, rgb(0x002d_353b));

        assert_ne!(macchiato.accent, latte.accent);
        assert_ne!(ef_light.accent, ef_dark.accent);
        assert_ne!(macchiato.bg, ef_dark.bg);
    }
}
