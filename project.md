# Shyena

A minimal, low-latency spotlight search bar and launcher built with GPUI for Windows

---

## Typography & Embedded Fonts

### Embedded Assets
* Monospace font: **GeistMono Nerd Font Mono** (400 weight only)
* Files located in `assets/fonts/`:
  * `GeistMonoNerdFontMono-Regular.otf` — base weight 400 regular
  * `GeistMonoNerdFontMono-Italic.otf` — base weight 400 italic
  * `LICENSE-GeistMono.txt` — SIL Open Font License 1.1 (OFL)

### Font Loading Mechanism
* Loaded at startup in `src/main.rs` via `cx.text_system().add_fonts(...)` using `include_bytes!`
* The `DirectWrite` text system in GPUI registers font data in-memory without system installs
* Root container in `render()` applies `.font_family("GeistMono Nerd Font Mono")`
* Font styling automatically cascades to all descendants in the component hierarchy

### Monospace / Nerd Font Guarantee
* The `NerdFontMono` variant enforces strict 1-cell monospace icon width
* Prevents misalignment in ASCII/TUI borders (`┌─[`, `─┐`), prompts (`❯`), and block cursors (`█`)

---

## Shipping & Distribution Requirements

### OFL 1.1 Compliance
* The SIL Open Font License allows bundling, embedding, and commercial distribution
* Requirement: Copyright notices and OFL terms must be preserved with any redistribution

### Release Packaging Checklist
* Source tree: Retain `assets/fonts/LICENSE-GeistMono.txt` next to the font assets
* Binary release (zip / installer):
  * Bundle `LICENSE-GeistMono.txt` into root distribution folder or `THIRD_PARTY_LICENSES`
  * No requirement to embed license text into the executable binary itself
