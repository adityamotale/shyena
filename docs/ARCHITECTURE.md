# Shyena architecture

Shyena is a long-running, logon-started Windows GUI process built with GPUI. It is a
keyboard-driven spotlight launcher and automation runner: native apps launch through a
zero-latency fast path, PowerShell automation runs off the UI thread, and configuration
is expressed as native PowerShell declarations in `.shyena`. This document is the single
source of truth for the runtime architecture, the reasoning behind its key decisions,
and the deployment contract.

## Process model

Shyena is a long-running Windows GUI process started at login. GPUI owns the
event loop; a hidden 1×1 `AnchorWindow` keeps that loop alive after the launcher
is dismissed, so the global hotkey and error channels never need re-initialising.
There is at most one launcher window, tracked by `ui::ActiveWindow` — a shared
`Rc<RefCell<Option<WindowHandle<SearchBar>>>>`.

The binary never writes `.shyena`. Installation seeds the installed copy once;
`ConfigStore` reads it at startup and serves every open from memory. Debug
builds resolve `Config::config_path()` to the repository-root `.shyena` so
development reads the canonical file instead of the installed config. Only
`:reload` and recovery from a failed load touch the disk.

## Module map

| Module | Responsibility |
|---|---|
| `bootstrap` | Hotkey registration and the two async pumps; owns shared handles. |
| `ui` | Window lifecycle, keyboard input, rendering, activation of commands. |
| `dispatch` | Scope parsing, query resolution, and the executable command model. |
| `commands` | Registry of built-in `:` commands (in-process, no spawn). |
| `config` | File I/O, parser, in-memory cache, reload contract. |
| `exec` | PowerShell invocation and error normalisation. |
| `theme` | Semantic colour tokens for dark/light themes. |
| `platform` | Win32 window styling and working-set trimming. |
| `effects` | Side-effect boundary (Win32, process spawn, worker threads). |

`dispatch::CommandIntent` is the single executable representation. Candidates
shown in the dropdown are presentation data (`CandidateAction`) and are converted
to an intent only when a row is activated. This keeps rendering concerns out of
execution and avoids fabricating throwaway values at the point of use.

## Interaction and presentation

### Query scoping and resolution

A query is an optional scope prefix (`@`, `#`, `:`) followed by whitespace-separated
tokens; both the compact (`@app`) and spaced (`@ app`) forms are accepted. `tokens[0]`
is the action key after the prefix is stripped, and `tokens[1..]` are forwarded to the
target app, function, or built-in command.

- `@` scopes candidates to registered applications;
- `#` scopes candidates to PowerShell automation functions;
- `:` scopes candidates to built-in commands (e.g. `:reload`);
- plain input searches apps and functions together; built-ins stay behind `:` so they
  never clutter normal results.

Resolution precedence is scoped prefix → exact key match → selected candidate →
explicit error. Candidates rank by case-insensitive exact key match, then
case-insensitive prefix match, then lexicographic order. Enter activates the selected
candidate, which is what makes "type a prefix, press Enter" work. The active scope
renders an action tag — `[ LAUNCH ]` (green) for `@`, `[ EXEC ]` (peach) for `#`,
`[ SYSTEM ]` (blue) for `:` — and the placeholder `@ apps, # commands, : system...`
advertises the three prefixes. Startup is an empty query with no candidates; the window
sits at base height until input produces candidates.

### Presentation

GeistMono Nerd Font Mono is the only font (just the 400-weight asset is shipped). It is
embedded with `include_bytes!` and registered via `cx.text_system().add_fonts(...)` at
startup, so no system font install is needed; strict one-cell glyph widths keep the
ASCII/TUI framing (`┌─[`, `─┐`), prompt (`❯`) and cursor block (`█`) aligned. It cascades
from the root container through `.font_family("GeistMono Nerd Font Mono")`.

Geometry is fixed at 660 px wide and centred, with a 62 px base bar (header pill + input
line). Candidate rows are 32 px each and the window grows to at most `MAX_VISIBLE_ITEMS
= 6` rows (~254 px); corners are square. Colour comes from a semantic Catppuccin palette
— Macchiato is the dark default and Latte is the light mode; `@app`/search accents are
Macchiato green in dark mode and Latte blue in light mode. UI code reads palette tokens
by role, never raw colours.

### Error states

- **Idle:** normal search bar with query, action tag and candidate list.
- **Error:** a failed command replaces the bar with a red-framed `shyena::error` panel
  showing one condensed line; input is ignored until `[Esc]` clears it and returns to an
  empty Idle bar in the same window.
- **Config error:** a load/reload failure shows a `shyena::config_error` panel marked
  `[ ESC: close ]`. It is non-dismissible: `[Esc]` closes the launcher so the search bar
  cannot reappear while the config is invalid.
- **Late errors:** if the launcher was already dismissed when the worker reports a
  failure, a new window opens directly in the Error state (see the error pump below).

Failures are condensed to one line: missing dynamic arguments are rejected locally for
functions indexing `$args[...]` (mandatory `param(...)` declarations get PowerShell's
own binding error); non-zero exit / `stderr` (including PowerShell 5.1 CLIXML records)
is reduced to its first meaningful line, whitespace-collapsed, truncated to 120
characters and prefixed with the command name. Config errors name the exact resolved
path so the file to repair is unambiguous.

## Event flows

**Hotkey toggle.** Alt+Backquote is registered by a dedicated OS thread
(`bootstrap::setup_global_hotkey`), which pushes unit events onto an async
channel. `spawn_hotkey_pump` receives them and marshals each onto GPUI's UI
thread, where `ui::toggle_search_window` either removes the current window or
opens a new one.

**Query → execution.** Keyboard input mutates `SearchBar::query` and recomputes
the candidate list. Enter calls `dispatch::resolve_command`, which applies the
scope rules (`@` app, `#` PowerShell, `:` built-in), falls back to the selected
candidate for the "type a prefix, press Enter" flow, and returns a
`CommandIntent`. `SearchBar::activate` runs the intent: apps and PowerShell
functions spawn and the window closes; in-process commands may keep the window
open to show an error.

**Async PowerShell failures.** `ui::launch_ps` spawns a worker thread; failures
are sent over a channel as `PsError`. `bootstrap::spawn_error_pump` opens a
window if needed and renders the message in the existing window, so an error
never disappears just because the launcher was dismissed.

**Config reload.** `:reload` calls `ConfigStore::reload`. On success the new
`Arc<Config>` and theme replace the cached ones and the window closes. On
failure the last known-good config stays active and a fatal config panel is
shown, dismissible only with Esc (which closes the launcher).

**Memory trim.** `ui::schedule_trim` waits a short delay, then asks Windows to
evict idle pages only if no launcher window is open. The delay keeps the syscall
off the teardown/next-frame path and lets a quick reopen cancel it.

## Configuration pipeline

### Language and validation

`.shyena` is valid PowerShell syntax, which avoids the multi-line escaping overhead of
static serialisation formats. Its top-level constructs are:

- `Set-Theme 'dark' | 'light'` — case-insensitive, default `dark` (Macchiato); `light`
  selects Latte.
- `Set-Suggestions 'on' | 'off'` — case-insensitive, default `on`. `off` skips candidate
  evaluation and hides the dropdown; the bar stays at base height and exact-key + Enter
  resolution still works.
- `Register-App 'name' @{ Path = '...'; Args = @() }` — static binary plus fixed args,
  with no dynamic parameter logic.
- `function name { ... }` — ordinary PowerShell functions for dynamic parameters,
  system control and multi-step logic. Functions needing dynamic tokens should declare
  mandatory parameters so PowerShell rejects a zero-argument call rather than coercing
  it; Shyena additionally rejects zero-token calls to functions indexing `$args[...]`.

The repository-root `.shyena` is embedded via `include_str!` for tests and is the
template `install.ps1` copies to `~/.config/.shyena`; the binary never creates it.
Validation is strict at the top level: unknown tokens (`app`, `ps`, `config`, ...), stray
characters (`@`, `;`, `$`, `=`, digit-leading identifiers), unterminated block comments
and trailing junk all produce line/column diagnostics. Omitted directives fall back to
`Config::default()` (dark theme, suggestions on). A legacy `# shyena:v1` header, if
present, is informational — the scanner treats it as a comment, so files with or
without it remain valid.

### File I/O

The objective is sub-millisecond cold start and `:reload`. Config is read with a raw
Win32 single-pass buffered read rather than `std::fs` or memory mapping. Benchmarks
(Windows 11 NT 10.0.26200, i5-10300H, median latency across 4/32/256 KB files) drove
that choice:

| Strategy | Result | Decision |
|---|---|---|
| Raw Win32 single-pass buffered read | 142.7 µs at 4 KB (~28% faster than `std::fs`), 203.5 µs at 256 KB | Chosen |
| `memmap2` | 433.2 µs at 256 KB — minor page faults and `UnmapViewOfFile` TLB shootdowns | Avoided |
| `std::fs` | redundant metadata queries on the hot path | Replaced by one speculative `ReadFile` into a pre-allocated 64 KB buffer |

`ConfigReader` opens with `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE` so
external editors doing atomic swaps cannot cause `ERROR_SHARING_VIOLATION`. It retains
a pre-encoded wide path and a reusable buffer and returns borrowed `&str` slices (zero
allocations on the hot path). The 64 KB fast path handles >99% of configs in one syscall;
a `GetFileSizeEx` slow path resizes only for larger files. UTF-8 BOMs are stripped and
the bytes validated in place before parsing.

### Parser

A hand-rolled zero-copy byte scanner operates directly on the `&str` from the reader.
Parser combinators (`winnow`, `nom`) were rejected to get sub-10 µs parsing, zero heap
allocations and no dependencies. It does top-level boundary extraction rather than a
full PowerShell AST: scan the schema marker, lex `Register-App`/`function`, then use a
string-, comment- and escape-aware brace counter to find each closing `}` and slice the
original text into the PS task unchanged.

To avoid being fooled by braces inside strings or comments it honours PowerShell
escaping: double-quoted strings may contain braces, escape with a backtick (including
`` `" ``); single-quoted strings treat backticks literally and escape a quote by doubling
it (`''`); `#` line comments end at a newline; `<# ... #>` block comments may span lines
and swallow braces. The scanner tracks 1-based `line`/`col`, so a malformed hashtable or
unclosed block yields a `ParseError` that the UI renders as an inline diagnostic instead
of crashing. Core types live in `src/config/parser.rs`: `Scanner<'a>`,
`ConfigAst<'a>` (`apps`/`functions`), `AppDef<'a>` (`unquoted_name()`),
`FunctionDef<'a>` and `ParseError`.

### Load and reload

`ConfigStore::load()` runs in `main.rs` before the hotkey loop with the path from
`Config::config_path()` (debug = repository root, release = installed path). A missing
file, I/O failure or parse error is cached as a diagnostic while `Config::default()`
keeps the UI renderable; missing-file diagnostics name the resolved path. Every healthy
open is served from the in-memory `Arc<Config>` — no filesystem I/O, watcher threads or
periodic timers. `:reload` re-reads synchronously on the UI thread (~200 µs), swaps the
cached config and theme, and closes the launcher; a failure keeps the last known-good
config and shows the fatal config panel. The cached diagnostic is retried on the next
open, so creating or repairing the file recovers without a restart. A trace at the
single read choke point confirmed one read at startup, none across healthy opens, one
per `:reload` and one per open while a diagnostic is set (recovery retry). External
edits are not observed automatically.

## Windows platform integration

Windows 10/11 exclusively; the Win32 layer has no non-Windows fallback and is not
`cfg`-gated.

**Hotkey and DWM.** Alt+Backquote is registered through the `global-hotkey` crate on a
dedicated thread (see Event flows). DWM styling uses `DwmExtendFrameIntoClientArea` with
a 1 px margin for the hardware drop shadow, `DWMWCP_DONOTROUND` for square corners, and
`HWND_TOPMOST` plus `SetForegroundWindow` to keep the launcher focused.

**PowerShell execution.** Each invocation runs on a transient `shyena-ps-task` thread,
so no PowerShell thread exists while idle. It builds a self-contained
`& { function ... }` block, encodes the payload to UTF-16LE Base64, and runs it via
Windows PowerShell 5.1 (`pwsh` as fallback) with `-ExecutionPolicy Bypass` and
`CREATE_NO_WINDOW` (`0x0800_0000`). `stderr` is captured through redirected temp files
(avoiding GUI-child pipe hangs) and reduced to a condensed `PsError` sent over
`async-channel` to the overlay. Native apps are spawned directly with
`std::process::Command::spawn()`.

**Working-set trimming.** `K32EmptyWorkingSet(GetCurrentProcess())` is deferred ~2 s
after startup and ~200 ms after dismissal so it stays off the teardown/next-frame path
and a quick reopen can cancel it. It only lowers the resident set — committed memory and
DirectX allocations are unchanged, and evicted pages fault back in on the next open.

The configuration/PowerShell/trimming work was measured point-in-time against the
pre-work baseline (release builds, i5-10300H, Windows 11 build 26200; idle CPU is a 60 s
`TotalProcessorTime` delta, other values are medians of three runs after a 10 s settle):

| Metric | Baseline `ab47d86` | At `4a86a77` |
|---|---|---|
| Total threads, idle | 9 | 7 |
| Shyena-owned threads, idle | 4 (hotkey, config reloader, Win32 watcher, PS worker) | 1 (hotkey) |
| Idle CPU, % of one core | 0.81 % | 0.68 % |
| Working set, 10 s after start | 64.8 MB | 3.1 MB |
| Working set, 5 s after dismiss | 74.1 MB | 0.6 MB |
| Working set after 25 open/close cycles | 79.1 MB | 0.6 MB |
| Private bytes, idle | 51.6 MB | 50.4 MB |
| Handles, idle → after 25 cycles | 314 → 353 | 316 → 354 |
| Config reads per healthy open | 0 (watcher cache) | 0 (startup cache) |
| Threads during a PowerShell call | 13, constant | +1 transient |

Total thread count and idle CPU are dominated by GPUI's VSync, background-work pool and
DXGI/renderer threads in both builds, so the shyena-owned share is reported explicitly.
Trimming leaves private bytes and handles unchanged while the resident set drops into
the low megabytes, and is skipped when a window reopened in the meantime so it never
sits on the activation path. The I/O trace used for these numbers was removed before the
profile was taken.

## Deployment and maintenance

`install.ps1` and `uninstall.ps1` are versioned in the repository root and run in a
non-elevated user context (UIPI parity with desktop apps, no elevated/Session 0 daemon).
Both scripts are interactive installers in the style of an `init` wizard: they print a
banner, animate downloads with a spinner, show a `PLAN` summary, ask for a simple
yes/no before each mutating action, and confirm once more with a final `Proceed?`
prompt before applying anything. Declining a core step (binary install, a missing
config) aborts; declining an optional step (Path entry, autostart, launch) only skips
that step. `-Yes` auto-accepts every default for unattended use; without it the
scripts refuse to run when stdin is redirected. Interactive runs wait for Enter
before returning so a dedicated console window or tab stays open; exit codes are
`0` success, `1` failure and `2` user-aborted, and apply to non-interactive runs.
`-DryRun` prints the same plan with defaults applied but makes no changes and asks
no questions.

`install.ps1` is also the update path — re-running it replaces the binary:

1. Validate 64-bit OS, build ≥ 19041, non-elevated token and PowerShell ≥ 5.1.
2. Download `SHA256SUMS` plus the release assets and verify SHA256 checksums before
   deploying anything.
3. Ask permission for each action, then install to
   `%LOCALAPPDATA%\Programs\shyena\shyena.exe`, optionally append that directory to
   the user `Path`, and stop a running instance first if one is found.
4. Seed `~/.config/.shyena` from the `.shyena` asset (`default.shyena` for older tags)
   **only if it does not already exist** — an existing config is never overwritten;
   the installer reports it with a `[SKIP] already exists` notice.
5. Optionally register `HKCU\Software\Microsoft\Windows\CurrentVersion\Run -> Shyena`
   with a strictly quoted exe path (`-NoAutostart` forces a skip), then launch detached
   (`-NoLaunch` forces a skip).

`uninstall.ps1` asks before each removal: stopping running instances, the autostart
key, the user `Path` entry and the install directory. A running instance is matched
by its installed executable path, and declining to close it aborts before anything
is changed, so a half-uninstall is never left behind. It retains `~/.config/.shyena`
by default and only purges it when `-PurgeConfig` is passed (or the user opts in).

## Testing strategy

`cargo test --all-targets` runs two automated tiers headlessly:

- **UI integration tests** (`ui.rs`) use GPUI's `TestAppContext` and drive real
  input with `simulate_input` / `simulate_keystrokes` / `simulate_mouse_down`.
  They assert both visible state (query, candidates, error panel) and side
  effects recorded by `test_support::RecordingEffects`.
- **Pump tests** (`bootstrap.rs`) feed the hotkey and error channels directly,
  never registering an OS-level hotkey, and assert window open/close/reuse.

Everything that touches the outside world goes through `effects::LauncherEffects`:
the binary uses `SystemEffects`, tests use `RecordingEffects`. `TestConfig`
creates an isolated temp config per test so reloads are real file operations.
`docs/SMOKE_TEST.md` covers the remaining desktop-only checks (global hotkey,
DWM visuals, real process launches).

## Extension points

- **New built-in command:** add an `InternalCommand` variant, a `CommandSpec`
  row, and a match arm in `SearchBar::run_internal`.
- **New config directive:** add a field to `ConfigAst`/`Config`, parse it in
  `config::parser`, and map it in `Config::from_ast`.
- **New theme role:** add a field to `theme::Palette` and a value in both theme
  branches; UI code reads tokens by role, never by raw colour.
