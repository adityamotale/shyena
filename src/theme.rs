use gpui::{rgb, rgba, Rgba};

/// Catppuccin Mocha palette and semantic token system (Dark Matte)
#[allow(dead_code)]
pub struct Mocha;

#[allow(dead_code)]
impl Mocha {
    pub const NAME: &'static str = "Catppuccin Mocha";

    #[inline(always)]
    pub fn rosewater() -> Rgba { rgb(0xf5e0dc) }
    #[inline(always)]
    pub fn flamingo() -> Rgba { rgb(0xf2cdcd) }
    #[inline(always)]
    pub fn pink() -> Rgba { rgb(0xf5c2e7) }
    #[inline(always)]
    pub fn mauve() -> Rgba { rgb(0xcba6f7) }
    #[inline(always)]
    pub fn red() -> Rgba { rgb(0xf38ba8) }
    #[inline(always)]
    pub fn maroon() -> Rgba { rgb(0xeba0ac) }
    #[inline(always)]
    pub fn peach() -> Rgba { rgb(0xfab387) }
    #[inline(always)]
    pub fn yellow() -> Rgba { rgb(0xf9e2af) }
    #[inline(always)]
    pub fn green() -> Rgba { rgb(0xa6e3a1) }
    #[inline(always)]
    pub fn teal() -> Rgba { rgb(0x94e2d5) }
    #[inline(always)]
    pub fn sky() -> Rgba { rgb(0x89dceb) }
    #[inline(always)]
    pub fn sapphire() -> Rgba { rgb(0x74c7ec) }
    #[inline(always)]
    pub fn blue() -> Rgba { rgb(0x89b4fa) }
    #[inline(always)]
    pub fn lavender() -> Rgba { rgb(0xb4befe) }

    #[inline(always)]
    pub fn text() -> Rgba { rgb(0xcdd6f4) }
    #[inline(always)]
    pub fn subtext1() -> Rgba { rgb(0xbac2de) }
    #[inline(always)]
    pub fn subtext0() -> Rgba { rgb(0xa6adc8) }
    #[inline(always)]
    pub fn overlay2() -> Rgba { rgb(0x9399b2) }
    #[inline(always)]
    pub fn overlay1() -> Rgba { rgb(0x7f849c) }
    #[inline(always)]
    pub fn overlay0() -> Rgba { rgb(0x6c7086) }
    #[inline(always)]
    pub fn surface2() -> Rgba { rgb(0x585b70) }
    #[inline(always)]
    pub fn surface1() -> Rgba { rgb(0x45475a) }
    #[inline(always)]
    pub fn surface0() -> Rgba { rgb(0x313244) }

    #[inline(always)]
    pub fn base_solid() -> Rgba { rgb(0x1e1e2e) }
    #[inline(always)]
    pub fn mantle_solid() -> Rgba { rgb(0x181825) }
    #[inline(always)]
    pub fn crust_solid() -> Rgba { rgb(0x11111b) }

    #[inline(always)]
    pub fn border_frame() -> Rgba { rgba(0xa6e3a166) }
    #[inline(always)]
    pub fn border_subtle() -> Rgba { rgba(0x31324499) }
    #[inline(always)]
    pub fn border_focus() -> Rgba { Self::green() }

    #[inline(always)]
    pub fn terminal_bg() -> Rgba { rgb(0x0c0e14) }
    #[inline(always)]
    pub fn terminal_header() -> Rgba { rgb(0x13161f) }
    #[inline(always)]
    pub fn terminal_frame() -> Rgba { rgba(0xa6e3a180) }
    #[inline(always)]
    pub fn terminal_bracket() -> Rgba { rgba(0xa6e3a1cc) }

    #[inline(always)]
    pub fn item_active_bg() -> Rgba { rgba(0x45475a88) }
    #[inline(always)]
    pub fn item_hover_bg() -> Rgba { rgba(0x31324466) }
    #[inline(always)]
    pub fn keycap_bg() -> Rgba { rgba(0x181825cc) }
    #[inline(always)]
    pub fn keycap_border() -> Rgba { rgba(0x45475acc) }
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
    pub fn border_frame() -> Rgba { rgba(0xacb0beaa) }
    #[inline(always)]
    pub fn border_subtle() -> Rgba { rgba(0xccd0daaa) }
    #[inline(always)]
    pub fn border_focus() -> Rgba { Self::blue() }

    #[inline(always)]
    pub fn item_active_bg() -> Rgba { rgba(0xccd0da80) }
    #[inline(always)]
    pub fn item_hover_bg() -> Rgba { rgba(0xccd0da40) }
    #[inline(always)]
    pub fn keycap_bg() -> Rgba { rgba(0xe6e9efcc) }
    #[inline(always)]
    pub fn keycap_border() -> Rgba { rgba(0xccd0dacc) }
    #[inline(always)]
    pub fn search_glyph() -> Rgba { Self::blue() }
}
