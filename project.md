# Shyena

A minimal, low-latency spotlight search bar and launcher built with GPUI for Windows

---

## Core Mission & Architecture

Shyena replaces shell alias scripts and Windows search with a hardware-accelerated,
sub-millisecond launcher;

### 1. Configuration Specification (`~/.config/shyena.toml`)
* Stored at `~/.config/shyena.toml`
* Validated via JSON Schema modeline hosted on GitHub raw content
* Schema URL pinned to release tags (e.g. `v0.1.0`) to avoid breaking older clients
* Schema generated at build time via `schemars` from Rust configuration structs
* Two core action categories: `[apps]` and `[ps]`

```toml
#:schema https://raw.githubusercontent.com/adityamotale/shyena/v0.1.0/schema.json

[apps.br]
path = "C:/Program Files/BraveSoftware/Brave-Browser/Application/brave.exe"
args = ["--profile-directory=Profile 2"]

[apps.code]
path = "C:/Users/froze/AppData/Local/Programs/Microsoft VS Code/Code.exe"
args = ["--reuse-window"]

[ps.set-light]
script = """
$wmi = Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods
$wmi.WmiSetBrightness(1, $args[0])
"""

[ps.yeet]
script = "Remove-Item -Path $args[0] -Recurse -Force"
```

### 2. Dual-Engine Execution Pipeline

#### Native Binary Fast-Path (`[apps]`)
* Dispatched via `std::process::Command` / Win32 `CreateProcessW`
* Zero shell overhead; invocation latency under `0x05` ms
* Prepends static `args` from config and appends dynamic runtime search arguments
* Enables direct browser navigation (`br https://github.com`) or profile switching

#### PowerShell Async Script Engine (`[ps]`)
* Dispatched to `pwsh` / `powershell.exe` in background worker threads via `async-channel`
* Completely non-blocking; never delays the GPUI frame loop
* Forwards dynamic search tokens to PowerShell script as `$args` array
* Used for WMI hardware calls, system management, and multi-step automation

### 3. Input Tokenization Contract
* First token (`tokens[0]`) matches action key in `[apps]` or `[ps]`
* Remaining tokens (`tokens[1..]`) forwarded as dynamic runtime arguments:
  * For `[apps]`: concatenated to CLI argument list passed to target binary
  * For `[ps]`: bound to `$args[0]`, `$args[1]`, etc. in script context

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
* DirectWrite text system in GPUI registers font data in-memory without system installs
* Root container in `render()` applies `.font_family("GeistMono Nerd Font Mono")`
* Font styling cascades to all descendants in the component hierarchy

### Monospace / Nerd Font Guarantee
* The `NerdFontMono` variant enforces strict 1-cell monospace icon width
* Prevents misalignment in ASCII/TUI borders (`┌─[`, `─┐`), prompts (`❯`), and block cursors (`█`)

---

## Shipping & Distribution Requirements

### OFL 1.1 Compliance
* The SIL Open Font License allows bundling, embedding, and commercial distribution;
* Requirement: Copyright notices and OFL terms must be preserved with any redistribution

### Release Packaging Checklist
* Source tree: Retain `assets/fonts/LICENSE-GeistMono.txt` next to the font assets
* Binary release (zip / installer):
  * Bundle `LICENSE-GeistMono.txt` into root distribution folder or `THIRD_PARTY_LICENSES`
  * No requirement to embed license text into the executable binary itself
