<img src="assets/banner.svg" width="100%" alt="Shyena" />

[![Release](https://img.shields.io/github/v/release/adityamotale/shyena?style=flat-square&logo=github)](https://github.com/adityamotale/shyena/releases)
[![Tests](https://img.shields.io/github/actions/workflow/status/adityamotale/shyena/tests.yaml?style=flat-square&logo=github&label=tests)](https://github.com/adityamotale/shyena/actions/workflows/tests.yaml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078d4?style=flat-square&logo=windows)](https://www.microsoft.com/windows)
[![Rust: edition 2024](https://img.shields.io/badge/Rust-edition%202024-fe640b?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![UI: GPUI](https://img.shields.io/badge/UI-GPUI-8839ef?style=flat-square)](https://gpui.rs)

# Shyena 🦅

Keyboard-first launcher and transient PowerShell automation runtime for Windows

## Index

- [Overview](#overview)
- [Runtime Guarantees](#runtime-guarantees)
- [Features](#features)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [Build from Source](#build-from-source)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [Acknowledgements](#acknowledgements)
- [License](#license)

## Overview

Shyena is an always-on, low-latency Windows launcher engineered for keyboard-driven workflows;
it maps global hotkey <kbd>Alt</kbd> + <kbd>`</kbd> directly to instant process dispatch and
transient PowerShell automation.

Resident execution operates behind a lightweight 1×1 hidden Win32 anchor proxy;
configuration is parsed once at initialization into memory, requiring zero file-system watcher
threads and zero ambient background processes while idle.

## Runtime Guarantees

> [!NOTE]
> **Zero Idle Overhead**
>
> When dismissed, working memory trims down into low megabytes;
> no background PowerShell runspaces or host processes persist while idle;
> script automation runs strictly on transient worker threads spawned on demand.

> [!NOTE]
> **Deterministic Window Anchoring**
>
> Hotkey registration remains bound to a permanent hidden 1×1 Win32 anchor;
> toggling the search surface creates or drops foreground presentation without unregistering
> global hotkey hooks or fighting OS window focus loss.

## Features

- **Global Hotkey Dispatch** — immediate activation via global <kbd>Alt</kbd> + <kbd>`</kbd>
- **Tri-Scope Prefix Routing** — `@` for binaries, `#` for PowerShell functions, `:` for internal
  routines, or unadorned input to query across all registered candidates
- **Direct Parameter Forwarding** — trailing arguments pass directly into target processes or
  PowerShell function invocation blocks
- **Type-Ahead Candidate Ranking** — exact-match indexing prioritized over prefix candidates, with
  <kbd>Tab</kbd> auto-completion preserving scope
- **Native PowerShell DSL** — configuration files evaluate standard PowerShell expressions with
  `Register-App` primitives and native function definitions
- **Embedded Typography** — bundled GeistMono Nerd Font provides crisp monospaced glyphs without
  external font dependencies
- **Atomic Guided Installer** — non-destructive bootstrap verifying release SHA-256 signatures

## Installation

Run the bootstrap installer inside an interactive PowerShell console:

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/adityamotale/shyena/master/install.ps1 -OutFile install.ps1
Set-ExecutionPolicy -Scope Process Bypass
.\install.ps1
```

> [!IMPORTANT]
> Target environment requires Windows 10 or 11 (build 19041 / 0x4A61 or newer) and PowerShell 5.1+;
> administrative elevation is neither requested nor required.

The installer displays execution stages prior to applying mutations;
in non-interactive or redirected pipelines, pass `-Yes` to accept defaults:

| Flag | Description |
|:---|:---|
| `-DryRun` | Emits planned operations without applying filesystem or registry changes |
| `-Yes` | Bypasses interactive prompts with default selections (required when stdin redirects) |
| `-Version <tag>` | Pulls explicit release tag instead of latest stable release |
| `-NoAutostart` | Omits per-user Run key configuration in HKCU |
| `-NoLaunch` | Suppresses automatic process launch upon completion |

Binaries unpack to `%LOCALAPPDATA%\Programs\shyena\shyena.exe`;
the initial config seeds to `~/.config/.shyena` only if absent to preserve existing user setups;
re-running `install.ps1` performs an in-place version upgrade.

To purge the installation, execute `uninstall.ps1`;
pass `-PurgeConfig` to remove configuration state under `~/.config/.shyena`.

## Quick Start

Trigger the launcher via <kbd>Alt</kbd> + <kbd>`</kbd>, then supply a routing prefix or plain query:

| Scope | Category | Invocation Pattern |
|:---|:---|:---|
| `@` | Registered Executable | `@terminal`, `@files C:\dev` |
| `#` | PowerShell Function | `# volume 40`, `# uuid` |
| `:` | Internal Control | `:reload` |
| None | Universal Search | `calc`, `stamp` |

Both compact (`@app`) and spaced (`@ app`) prefixes are valid;
tokens following the identifier forward directly as command arguments:

| Key | Operation |
|:---|:---|
| <kbd>↑</kbd> / <kbd>↓</kbd> | Cycle highlighted match candidate |
| <kbd>Tab</kbd> | Complete active token while retaining prefix scope |
| <kbd>Enter</kbd> | Dispatch highlighted item or exact matching key |
| <kbd>Esc</kbd> | Dismiss interface; reset active command errors |
| <kbd>Backspace</kbd> | Remove preceding character |

Execution failures emit inline error diagnostics cleared via <kbd>Esc</kbd>;
syntax issues in configuration surface target file lines on startup.

## Configuration

Production configuration resides at `~/.config/.shyena` (local debug builds read root [`.shyena`](.shyena));
syntax follows declarative PowerShell source evaluated once at boot:

```powershell
Set-Theme 'light'       # 'dark' = Macchiato (default), 'light' = Latte
Set-Suggestions 'on'    # 'off' = exact key + Enter only, no dropdown

Register-App 'terminal' @{
    Path = 'wt.exe'
    Args = @()
}

function uuid { Set-Clipboard -Value ([guid]::NewGuid().ToString()) }
```

Issue `:reload` within the launcher interface to parse configuration changes into memory;
inspect [`.shyena`](.shyena) for complete examples spanning clipboard, audio, and process commands.

## Build from Source

Requirements:
- Stable Rust (MSRV 1.85+, edition 2024)
- Windows SDK with MSVC C++ toolchain

```powershell
git clone https://github.com/adityamotale/shyena.git
cd shyena
cargo build --release
```

Release targets compile as headless GUI binaries (`subsystem = "windows"`);
debug profiles retain console output for diagnostics and interruptible tracing via <kbd>Ctrl</kbd> + <kbd>C</kbd>.

## Documentation

- [Architecture Guide](docs/ARCHITECTURE.md) — internal process mechanics, state machines, and benchmarks
- [Smoke Test Protocol](docs/SMOKE_TEST.md) — manual verification for hotkey traps, DWM composition, and IPC
- [Reference Configuration](.shyena) — seed template containing media and process recipes

## Contributing

Ensure static analysis and headless UI test suites remain green prior to pull request submission:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

UI tests run headlessly through GPUI `TestAppContext` abstractions;
system dependencies (Win32 APIs and subprocesses) isolate behind an injectable effects boundary;
review [ARCHITECTURE.md](docs/ARCHITECTURE.md#extension-points) for extending built-in commands and configuration directives.

## Acknowledgements

- [Catppuccin](https://catppuccin.com/) — palette definitions powering dark and light themes
- [GPUI](https://gpui.rs) — hardware-accelerated desktop UI engine
- [Geist Mono](https://github.com/vercel/geist-font) patched with [Nerd Fonts](https://github.com/ryanoasis/nerd-fonts) — embedded monospace typography
- [global-hotkey](https://github.com/tauri-apps/global-hotkey) — low-level Win32 hotkey hooking

## License

MIT License — see [LICENSE](LICENSE) for full legal text
