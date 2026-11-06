use gpui::{rgb, rgba, Rgba};

/// Catppuccin Latte palette and semantic token system
#[allow(dead_code)]
pub struct Latte;

#[allow(dead_code)]
impl Latte {
    // ------------------------------------------------------------------------
    // Raw Catppuccin Latte Palette
    // ------------------------------------------------------------------------
    #[inline(always)]
    pub fn rosewater() -> Rgba {
        rgb(0xdc8a78)
    }

    #[inline(always)]
    pub fn flamingo() -> Rgba {
        rgb(0xdd7878)
    }

    #[inline(always)]
    pub fn pink() -> Rgba {
        rgb(0xea76cb)
    }

    #[inline(always)]
    pub fn mauve() -> Rgba {
        rgb(0x8839ef)
    }

    #[inline(always)]
    pub fn red() -> Rgba {
        rgb(0xd20f39)
    }

    #[inline(always)]
    pub fn maroon() -> Rgba {
        rgb(0xe64553)
    }

    #[inline(always)]
    pub fn peach() -> Rgba {
        rgb(0xfe640b)
    }

    #[inline(always)]
    pub fn yellow() -> Rgba {
        rgb(0xdf8e1d)
    }

    #[inline(always)]
    pub fn green() -> Rgba {
        rgb(0x40a02b)
    }

    #[inline(always)]
    pub fn teal() -> Rgba {
        rgb(0x179299)
    }

    #[inline(always)]
    pub fn sky() -> Rgba {
        rgb(0x04a5e5)
    }

    #[inline(always)]
    pub fn sapphire() -> Rgba {
        rgb(0x209fb5)
    }

    #[inline(always)]
    pub fn blue() -> Rgba {
        rgb(0x1e66f5)
    }

    #[inline(always)]
    pub fn lavender() -> Rgba {
        rgb(0x7287fd)
    }

    #[inline(always)]
    pub fn text() -> Rgba {
        rgb(0x4c4f69)
    }

    #[inline(always)]
    pub fn subtext1() -> Rgba {
        rgb(0x5c5f77)
    }

    #[inline(always)]
    pub fn subtext0() -> Rgba {
        rgb(0x6c6f85)
    }

    #[inline(always)]
    pub fn overlay2() -> Rgba {
        rgb(0x7c7f93)
    }

    #[inline(always)]
    pub fn overlay1() -> Rgba {
        rgb(0x8c8fa1)
    }

    #[inline(always)]
    pub fn overlay0() -> Rgba {
        rgb(0x9ca0b0)
    }

    #[inline(always)]
    pub fn surface2() -> Rgba {
        rgb(0xacb0be)
    }

    #[inline(always)]
    pub fn surface1() -> Rgba {
        rgb(0xbcc0cc)
    }

    #[inline(always)]
    pub fn surface0() -> Rgba {
        rgb(0xccd0da)
    }

    #[inline(always)]
    pub fn base_solid() -> Rgba {
        rgb(0xeff1f5)
    }

    #[inline(always)]
    pub fn mantle_solid() -> Rgba {
        rgb(0xe6e9ef)
    }

    #[inline(always)]
    pub fn crust_solid() -> Rgba {
        rgb(0xdce0e8)
    }

    // ------------------------------------------------------------------------
    // Backgrounds & Acrylic Layers (high-translucency with strong legibility)
    // ------------------------------------------------------------------------
    #[inline(always)]
    pub fn base_acrylic() -> Rgba {
        rgba(0xeff1f5e6)
    }

    #[inline(always)]
    pub fn mantle_acrylic() -> Rgba {
        rgba(0xe6e9eff0)
    }

    #[inline(always)]
    pub fn crust_acrylic() -> Rgba {
        rgba(0xdce0e8f0)
    }

    // ------------------------------------------------------------------------
    // Borders & Dividers
    // ------------------------------------------------------------------------
    #[inline(always)]
    pub fn border_glass() -> Rgba {
        rgba(0xacb0be80)
    }

    #[inline(always)]
    pub fn border_subtle() -> Rgba {
        rgba(0xccd0da70)
    }

    #[inline(always)]
    pub fn border_focus() -> Rgba {
        Self::blue()
    }

    // ------------------------------------------------------------------------
    // Interactive Elements & Surfaces
    // ------------------------------------------------------------------------
    #[inline(always)]
    pub fn item_active_bg() -> Rgba {
        rgba(0xccd0da66)
    }

    #[inline(always)]
    pub fn item_active_badge_bg() -> Rgba {
        rgba(0x1e66f51a)
    }

    #[inline(always)]
    pub fn keycap_bg() -> Rgba {
        rgba(0xe6e9efa6)
    }

    #[inline(always)]
    pub fn keycap_border() -> Rgba {
        rgba(0xccd0da99)
    }
}
