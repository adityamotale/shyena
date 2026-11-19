<img src="assets/banner.png" width="100%" alt="Shyena" />

[![Release](https://img.shields.io/github/v/release/adityamotale/shyena?style=flat-square&logo=github)](https://github.com/adityamotale/shyena/releases)
[![Tests](https://img.shields.io/github/actions/workflow/status/adityamotale/shyena/tests.yaml?style=flat-square&logo=github&label=tests)](https://github.com/adityamotale/shyena/actions/workflows/tests.yaml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078d4?style=flat-square&logo=windows)](https://www.microsoft.com/windows)
[![Rust: edition 2024](https://img.shields.io/badge/Rust-edition%202024-fe640b?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![UI: GPUI](https://img.shields.io/badge/UI-GPUI-8839ef?style=flat-square)](https://gpui.rs)

Śyena is a Keyboard-first launcher and transient PowerShell automation runtime for Windows

## Index

- [Overview](#overview)
- [Installation](#installation)
- [Uninstall](#uninstall)
- [Quick Start](#quick-start)
- [Etymology](#etymology)
- [Runtime Guarantees](#runtime-guarantees)
- [Configuration](#configuration)
- [Contributing](#contributing)
- [Acknowledgements](#acknowledgements)

## Overview

Shyena is an always-on, low-latency Windows launcher engineered for keyboard-driven workflows;
it maps global hotkey <kbd>Alt</kbd> + <kbd>`</kbd> directly to instant process dispatch and
transient PowerShell automation.

Resident execution operates behind a lightweight 1×1 hidden Win32 anchor proxy;
configuration is parsed once at initialization into memory, requiring zero file-system watcher
threads and zero ambient background processes while idle.

## Installation

Run the bootstrap installer inside an interactive PowerShell console:

```powershell
irm https://raw.githubusercontent.com/adityamotale/shyena/master/install.ps1 | iex
```

> [!IMPORTANT]
> Target environment requires Windows 10 or 11 (build 19041 / 0x4A61 or newer) and PowerShell 5.1+.
>
> Administrative elevation is neither requested nor required

## Uninstall

Run the uninstaller inside an interactive PowerShell console:

```powershell
irm https://raw.githubusercontent.com/adityamotale/shyena/master/uninstall.ps1 | iex
```

Retains `~/.config/.shyena` by default; pass `-PurgeConfig` to remove configuration files

## Quick Start

Trigger the launcher via <kbd>Alt</kbd> + <kbd>`</kbd>, then supply a routing prefix or plain query,

| Scope | Category              | Invocation Pattern           |
|:------|:----------------------|:-----------------------------|
| `@`   | Registered Executable | `@terminal`, `@files C:\dev` |
| `#`   | PowerShell Function   | `# volume 40`, `# uuid`      |
| `:`   | Internal Control      | `:reload`                    |
| None  | Universal Search      | `calc`, `stamp`              |

Both compact (`@app`) and spaced (`@ app`) prefixes are valid.

Tokens following the identifier forward directly as command arguments,

| Key                         | Operation                                          |
|:----------------------------|:---------------------------------------------------|
| <kbd>↑</kbd> / <kbd>↓</kbd> | Cycle highlighted match candidate                  |
| <kbd>Tab</kbd>              | Complete active token while retaining prefix scope |
| <kbd>Enter</kbd>            | Dispatch highlighted item or exact matching key    |
| <kbd>Esc</kbd>              | Dismiss interface; reset active command errors     |
| <kbd>Backspace</kbd>        | Remove preceding character                         |

Execution failures emit inline error diagnostics cleared via <kbd>Esc</kbd>; , while syntax
issues in configuration surface target file lines on startup.

## Etymology

_Śyena_ (Sanskrit _श्येन_) is the name of a falcon. In the *Ṛgveda*, *Śyena* is the divine bird of prey that
soared into the heavens, broke through its defenses, and brought the sacred *Soma* down to earth.

Just like its namesake, this project is defiant, fast, and built for precision.

## Runtime Guarantees

> [!NOTE]
> When dismissed, working memory trims down into low megabytes, and no background PowerShell runspaces
> or host processes persist while idle/
>
> Script automation runs strictly on transient worker threads spawned on demand.

> [!NOTE]
> Hotkey registration remains bound to a permanent hidden 1×1 Win32 anchor.
>
> Toggling the search surface creates or drops foreground presentation without unregistering global hotkey
> hooks or fighting OS window focus loss.

## Configuration

Production configuration resides at `~/.config/.shyena` (local debug builds read root [`.shyena`](.shyena));
syntax follows declarative PowerShell source evaluated once at boot:

```powershell
Set-Theme 'light'       # 'dark' = Macchiato (default), 'light' = Latte
Set-Suggestions 'on'    # 'off' = exact key + Enter only, no dropdown
```

Issue `:reload` within the launcher interface to parse configuration changes into memory. Inspect the
[`.shyena`](.shyena) for complete examples spanning clipboard, audio, and process commands.

## Contributing

Ensure static analysis and headless UI test suites remain green prior to pull request submission,

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

UI tests run headlessly through GPUI `TestAppContext` abstractions and system dependencies (Win32 APIs and
subprocesses) isolate behind an injectable effects boundary & review
[ARCHITECTURE.md](docs/ARCHITECTURE.md#extension-points) for extending built-in commands and configuration directives.

## Acknowledgements

- [GPUI](https://gpui.rs) — hardware-accelerated desktop UI engine
- [Catppuccin](https://catppuccin.com/) — palette definitions powering dark and light themes
- [global-hotkey](https://github.com/tauri-apps/global-hotkey) — low-level Win32 hotkey hooking
- [Geist Mono](https://github.com/vercel/geist-font) patched with [Nerd Fonts](https://github.com/ryanoasis/nerd-fonts) — embedded monospace typography
