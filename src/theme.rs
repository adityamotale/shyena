use gpui::{rgb, rgba, Rgba};

/// Catppuccin Macchiato palette and semantic token system (Dark Matte)
#[allow(dead_code)]
pub struct Macchiato;

#[allow(dead_code)]
impl Macchiato {
    pub const NAME: &'static str = "Catppuccin Macchiato";

    #[inline(always)]
    pub fn rosewater() -> Rgba { rgb(0xf4dbd6) }
    #[inline(always)]
    pub fn flamingo() -> Rgba { rgb(0xf0c6c6) }
    #[inline(always)]
    pub fn pink() -> Rgba { rgb(0xf5bde6) }
    #[inline(always)]
    pub fn mauve() -> Rgba { rgb(0xc6a0f6) }
    #[inline(always)]
    pub fn red() -> Rgba { rgb(0xed8796) }
    #[inline(always)]
    pub fn maroon() -> Rgba { rgb(0xee99a0) }
    #[inline(always)]
    pub fn peach() -> Rgba { rgb(0xf5a97f) }
    #[inline(always)]
    pub fn yellow() -> Rgba { rgb(0xeed49f) }
    #[inline(always)]
    pub fn green() -> Rgba { rgb(0xa6da95) }
    #[inline(always)]
    pub fn teal() -> Rgba { rgb(0x8bd5ca) }
    #[inline(always)]
    pub fn sky() -> Rgba { rgb(0x91d7e3) }
    #[inline(always)]
    pub fn sapphire() -> Rgba { rgb(0x7dc4e4) }
    #[inline(always)]
    pub fn blue() -> Rgba { rgb(0x8aadf4) }
    #[inline(always)]
    pub fn lavender() -> Rgba { rgb(0xb7bdf8) }

    #[inline(always)]
    pub fn text() -> Rgba { rgb(0xcad3f5) }
    #[inline(always)]
    pub fn subtext1() -> Rgba { rgb(0xb8c0e0) }
    #[inline(always)]
    pub fn subtext0() -> Rgba { rgb(0xa5adcb) }
    #[inline(always)]
    pub fn overlay2() -> Rgba { rgb(0x939ab7) }
    #[inline(always)]
    pub fn overlay1() -> Rgba { rgb(0x8087a2) }
    #[inline(always)]
    pub fn overlay0() -> Rgba { rgb(0x6e738d) }
    #[inline(always)]
    pub fn surface2() -> Rgba { rgb(0x5b6078) }
    #[inline(always)]
    pub fn surface1() -> Rgba { rgb(0x494d64) }
    #[inline(always)]
    pub fn surface0() -> Rgba { rgb(0x363a4f) }

    #[inline(always)]
    pub fn base_solid() -> Rgba { rgb(0x24273a) }
    #[inline(always)]
    pub fn mantle_solid() -> Rgba { rgb(0x1e2030) }
    #[inline(always)]
    pub fn crust_solid() -> Rgba { rgb(0x181926) }

    #[inline(always)]
    pub fn border_frame() -> Rgba { rgba(0xa6da9566) }
    #[inline(always)]
    pub fn border_subtle() -> Rgba { rgba(0x363a4f99) }
    #[inline(always)]
    pub fn border_focus() -> Rgba { Self::green() }

    #[inline(always)]
    pub fn terminal_bg() -> Rgba { rgb(0x1b1d2b) }
    #[inline(always)]
    pub fn terminal_header() -> Rgba { rgb(0x202233) }
    #[inline(always)]
    pub fn terminal_frame() -> Rgba { rgba(0xa6da9580) }
    #[inline(always)]
    pub fn terminal_bracket() -> Rgba { rgba(0xa6da95cc) }

    #[inline(always)]
    pub fn item_active_bg() -> Rgba { rgba(0x494d6488) }
    #[inline(always)]
    pub fn item_hover_bg() -> Rgba { rgba(0x363a4f66) }
    #[inline(always)]
    pub fn keycap_bg() -> Rgba { rgba(0x1e2030cc) }
    #[inline(always)]
    pub fn keycap_border() -> Rgba { rgba(0x494d64cc) }
    #[inline(always)]
    pub fn search_glyph() -> Rgba { Self::green() }
}

/// Catppuccin Latte palette and semantic token system (Light Matte)
#[allow(dead_code)]
pub struct Latte;

#[allow(dead_code)]
impl Latte {
    pub const NAME: &'static str = "Catppuccin Latte";

    #[inline(always)]
    pub fn rosewater() -> Rgba { rgb(0xdc8a78) }
    #[inline(always)]
    pub fn flamingo() -> Rgba { rgb(0xdd7878) }
    #[inline(always)]
    pub fn pink() -> Rgba { rgb(0xea76cb) }
    #[inline(always)]
    pub fn mauve() -> Rgba { rgb(0x8839ef) }
    #[inline(always)]
    pub fn red() -> Rgba { rgb(0xd20f39) }
    #[inline(always)]
    pub fn maroon() -> Rgba { rgb(0xe64553) }
    #[inline(always)]
    pub fn peach() -> Rgba { rgb(0xfe640b) }
    #[inline(always)]
    pub fn yellow() -> Rgba { rgb(0xdf8e1d) }
    #[inline(always)]
    pub fn green() -> Rgba { rgb(0x40a02b) }
    #[inline(always)]
    pub fn teal() -> Rgba { rgb(0x179299) }
    #[inline(always)]
    pub fn sky() -> Rgba { rgb(0x04a5e5) }
    #[inline(always)]
    pub fn sapphire() -> Rgba { rgb(0x209fb5) }
    #[inline(always)]
    pub fn blue() -> Rgba { rgb(0x1e66f5) }
    #[inline(always)]
    pub fn lavender() -> Rgba { rgb(0x7287fd) }

    #[inline(always)]
    pub fn text() -> Rgba { rgb(0x4c4f69) }
    #[inline(always)]
    pub fn subtext1() -> Rgba { rgb(0x5c5f77) }
    #[inline(always)]
    pub fn subtext0() -> Rgba { rgb(0x6c6f85) }
    #[inline(always)]
    pub fn overlay2() -> Rgba { rgb(0x7c7f93) }
    #[inline(always)]
    pub fn overlay1() -> Rgba { rgb(0x8c8fa1) }
    #[inline(always)]
    pub fn overlay0() -> Rgba { rgb(0x9ca0b0) }
    #[inline(always)]
    pub fn surface2() -> Rgba { rgb(0xacb0be) }
    #[inline(always)]
    pub fn surface1() -> Rgba { rgb(0xbcc0cc) }
    #[inline(always)]
    pub fn surface0() -> Rgba { rgb(0xccd0da) }

    #[inline(always)]
    pub fn base_solid() -> Rgba { rgb(0xeff1f5) }
    #[inline(always)]
    pub fn mantle_solid() -> Rgba { rgb(0xe6e9ef) }
    #[inline(always)]
    pub fn crust_solid() -> Rgba { rgb(0xdce0e8) }

    #[inline(always)]
    pub fn border_frame() -> Rgba { rgba(0x1e66f566) }
    #[inline(always)]
    pub fn border_subtle() -> Rgba { rgba(0xacb0be88) }
    #[inline(always)]
    pub fn border_focus() -> Rgba { Self::blue() }

    #[inline(always)]
    pub fn terminal_bg() -> Rgba { Self::base_solid() }
    #[inline(always)]
    pub fn terminal_header() -> Rgba { Self::mantle_solid() }
    #[inline(always)]
    pub fn terminal_frame() -> Rgba { rgba(0x1e66f580) }
    #[inline(always)]
    pub fn terminal_bracket() -> Rgba { rgba(0x1e66f5cc) }

    #[inline(always)]
    pub fn item_active_bg() -> Rgba { rgba(0x1e66f51f) }
    #[inline(always)]
    pub fn item_hover_bg() -> Rgba { rgba(0x1e66f512) }
    #[inline(always)]
    pub fn keycap_bg() -> Rgba { rgba(0x1e66f51f) }
    #[inline(always)]
    pub fn keycap_border() -> Rgba { rgba(0x1e66f540) }
    #[inline(always)]
    pub fn search_glyph() -> Rgba { Self::blue() }
}

/// Runtime theme selection backed by `Set-Theme` in `.shyena`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

#[allow(dead_code)]
impl Theme {
    #[inline]
    pub fn from_config_str(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("dark") {
            Some(Self::Dark)
        } else if value.eq_ignore_ascii_case("light") {
            Some(Self::Light)
        } else {
            None
        }
    }

    #[inline]
    pub fn as_config_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    #[inline]
    pub fn name(self) -> &'static str {
        match self {
            Self::Dark => Macchiato::NAME,
            Self::Light => Latte::NAME,
        }
    }

    /// Semantic accent: green in dark mode, vivid blue in light mode.
    #[inline]
    pub fn accent(self) -> Rgba {
        self.search_glyph()
    }

    /// Candidate detail text: stronger in light mode for readability on base.
    #[inline]
    pub fn detail_text(self) -> Rgba {
        match self {
            Self::Dark => Macchiato::subtext0(),
            Self::Light => Latte::subtext1(),
        }
    }
}

macro_rules! delegate_theme {
    ($($method:ident),* $(,)?) => {
        #[allow(dead_code)]
        impl Theme {
            $(
                #[inline(always)]
                pub fn $method(self) -> Rgba {
                    match self {
                        Self::Dark => Macchiato::$method(),
                        Self::Light => Latte::$method(),
                    }
                }
            )*
        }
    };
}

delegate_theme!(
    rosewater,
    flamingo,
    pink,
    mauve,
    red,
    maroon,
    peach,
    yellow,
    green,
    teal,
    sky,
    sapphire,
    blue,
    lavender,
    text,
    subtext1,
    subtext0,
    overlay2,
    overlay1,
    overlay0,
    surface2,
    surface1,
    surface0,
    base_solid,
    mantle_solid,
    crust_solid,
    border_frame,
    border_subtle,
    border_focus,
    terminal_bg,
    terminal_header,
    terminal_frame,
    terminal_bracket,
    item_active_bg,
    item_hover_bg,
    keycap_bg,
    keycap_border,
    search_glyph,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_config_strings() {
        assert_eq!(Theme::default(), Theme::Dark);
        assert_eq!(Theme::from_config_str("dark"), Some(Theme::Dark));
        assert_eq!(Theme::from_config_str("LIGHT"), Some(Theme::Light));
        assert_eq!(Theme::from_config_str("mocha"), None);
        assert_eq!(Theme::Light.as_config_str(), "light");
    }

    #[test]
    fn test_theme_delegates_to_palette() {
        assert_eq!(Theme::Dark.text(), Macchiato::text());
        assert_eq!(Theme::Dark.green(), rgb(0xa6da95));
        assert_eq!(Theme::Dark.accent(), Macchiato::green());
        assert_eq!(Theme::Dark.detail_text(), Macchiato::subtext0());

        assert_eq!(Theme::Light.text(), Latte::text());
        assert_eq!(Theme::Light.terminal_bg(), Latte::base_solid());
        assert_eq!(Theme::Light.terminal_header(), Latte::mantle_solid());
        assert_eq!(Theme::Light.accent(), Latte::blue());
        assert_eq!(Theme::Light.detail_text(), Latte::subtext1());
    }
}
