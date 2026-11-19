# Shyena architecture

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
