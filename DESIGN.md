# Shyena System Design Specification

---

## 1. Executive Summary

Shyena is a minimal, hardware-accelerated spotlight launcher and automation runner for Windows, built with GPUI.
It replaces shell alias scripts, hotkey daemons, and traditional Windows search with a sub-millisecond, keyboard-driven interface.

### Core Architecture Pillars
- **Zero-Latency Fast Path**: Native application execution bypasses intermediate shells entirely;
- **Async Automation Pipeline**: Long-running PowerShell automation executes off-thread without degrading frame presentation;
- **Native Script Configuration**: Replaces static configuration serialization formats with native script-first declarations (`.shyena`);
- **Keyboard-Centric Interaction**: Predictable prefix scoping, single-keystroke selection, and deterministic ranking;

---

## 2. Design System & Visual Presentation

### Typography Pipeline & Embedded Assets
- Monospace font family: **GeistMono Nerd Font Mono** (400 weight only)
- Asset layout under `assets/fonts/`:
  - `GeistMonoNerdFontMono-Regular.otf` — Regular base weight (400)

### In-Memory Font Loading
- Embedded into the binary via `include_bytes!` to eliminate system font installation requirements;
- Loaded at GPUI startup using `cx.text_system().add_fonts(...)`;
- Cascades from root container via `.font_family("GeistMono Nerd Font Mono")`;
- Strict 1-cell monospace icon width guarantees alignment across ASCII/TUI framing (`┌─[`, `─┐`), prompts (`❯`), and custom cursor blocks (`█`);

### Layout Geometry & Aesthetics
- **Width**: `660.0 px` fixed width, centered horizontally;
- **Base Height**: `62.0 px` bar height (header pill + input line);
- **Candidate Row Height**: `32.0 px` per matched candidate item;
- **Candidate Ceiling**: `MAX_VISIBLE_ITEMS: 0x06`; window dynamically resizes up to `254.0 px`;
- **Border Radius**: `0.0 px` sharp corners throughout container and list items;
- **Color Palette**: Catppuccin Macchiato semantic token palette (dark matte default) with Latte
  support for light mode; `@app` and search accents use Macchiato green in dark mode and Latte blue
  in light mode; candidate rows use palette-derived accent tints;

---

## 3. Search Bar Interface & State Machine

### Dual-Token Syntax Contract
Queries follow a clean tokenizer contract:
- **First Token (`tokens[0]`)**: Resolves the action key;
- **Subsequent Tokens (`tokens[1..]`)**: Forwarded dynamically to the target command or script;

### Prefix Routing & Scoping
- `@<query>`: Scopes search candidates exclusively to registered applications;
- `#<query>`: Scopes search candidates exclusively to PowerShell automation functions;
- Plain input evaluates across both domains simultaneously;

### Idle State & Candidate Presentation
- **Startup State**: Search bar opens with an empty query and zero listed candidates;
  window bounds remain locked to base height (`62.0 px`);
- **Dynamic Expansion**: Candidate evaluation triggers on input; window resizes dynamically
  up to `MAX_VISIBLE_ITEMS: 0x06` (`254.0 px`);
- **Input Placeholder**: `@ for apps, # for commands...` conveys prefix filter syntax;

### Action Tag State Machine
- **Idle**: Empty tag area when query is empty;
- **Application (`@app`)**: Clean `[ LAUNCH ]` badge with green accent;
- **Command / Automation (`#ps`)**: Clean `[ EXEC ]` badge with peach accent;
- **Resolution Precedence**: Scoped prefix (`@`/`#`) -> selected candidate -> exact
  dictionary key match -> fallback execution;
- **Error**: A failed command replaces the bar in place with a one-line diagnostic; `[Esc]`
  dismisses it and returns to an empty `Idle` bar (see §9). Configuration errors are fatal
  for the session: `[Esc]` closes the launcher instead of revealing a search bar;

### Candidate Ranking Precedence
1. Exact match on target key (case-insensitive);
2. Prefix match on target key (case-insensitive);
3. Lexicographical ascending order;

### Dual-Engine Execution Pipeline
- **Native Binary Fast-Path (`@app`)**:
  - Spawned directly via `std::process::Command` / Win32 `CreateProcessW`;
  - Sub-5ms invocation latency; static flags applied without dynamic token expansion;
- **PowerShell Async Pipeline (`#ps`)**:
  - Dispatched to worker threads via an `async-channel` queue;
  - Non-blocking execution preserves 60+ FPS UI interaction;
  - Forwards CLI parameters directly to script functions;

---

## 4. Configuration Language Specification (`.shyena`)

Shyena uses a dedicated configuration file format based on valid PowerShell syntax, removing TOML multi-line escaping overhead.

### Target Path
- Primary path: `~/.config/.shyena`;

### Language Construct Rules
1. **Theme Selection (`Set-Theme`)**:
   Top-level quoted value selecting the palette: `'dark'` maps to Catppuccin Macchiato,
   `'light'` maps to Catppuccin Latte; values are case-insensitive and omission defaults to `'dark'`:
   ```powershell
   Set-Theme 'dark'
   ```
2. **Application Registration (`Register-App`)**:
   Static definitions for binaries and fixed arguments with zero dynamic parameter logic:
   ```powershell
   Register-App 'code' @{
       Path = 'C:/Users/froze/AppData/Local/Programs/Microsoft VS Code/Code.exe'
       Args = @('--reuse-window')
   }
   ```
3. **PowerShell Automation Functions (`function <name>`)**:
   Commands requiring dynamic parameters, system control, or multi-step logic are written as standard PowerShell functions:
   ```powershell
   function brightness {
       param(
           [Parameter(Mandatory = $true)]
           [ValidateRange(0, 100)]
           [int]$Level
       )

       $initial = [Math]::Max(0, $Level - 10)
       $wmi = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods
       # Execution logic...
   }
   ```

Functions that require dynamic tokens should declare them as mandatory parameters (as shown in the
`brightness` example), so PowerShell rejects a zero-argument invocation instead of coercing it to a
default value. Shyena additionally rejects zero-token invocations of functions that index
`$args[...]` (see §9).

---

## 5. Schema Evolution & Backward Compatibility

*Status: Strict validation implemented*

### Implemented Rules
- Unknown top-level tokens (`app`, `ps`, `config`, etc.) and stray characters (`@`, `;`, `$`, `=`,
  digit-leading identifiers) are rejected with a line/column diagnostic;
- `Set-Theme` accepts only quoted `'dark'` / `'light'` values (case-insensitive); any other value
  produces `invalid theme: expected 'dark' or 'light'`;
- Unterminated block comments (`<# ...`) and trailing junk after a directive are parse errors;
- `#`/`<# ... #>` comments remain valid and are ignored by the scanner.

### Architectural Considerations
- Need for explicit header directives (e.g. `# shyena:v1`);
- Migration strategies for legacy configuration files;

---

## 6. File I/O Subsystem

*Status: Implemented*

### Finalized Architecture: Raw Win32 Single-Pass Buffered Read

The configuration pipeline targets `~/.config/.shyena`,
housing native PowerShell definitions (`Register-App` and `function <name>`);
sub-millisecond cold start and `#config-reload` latency are primary design objectives

Empirical benchmarks across 5 Win32 file-reading strategies on Windows 11 (NT 10.0.26200,
Intel i5-10300H) yielded the following latency characteristics across 4 KB, 32 KB, and 256 KB:
- **Raw Win32 Single-Pass Buffered Read**: 142.7 µs median at 4 KB (~28% faster than
  `std::fs::read_to_string`); 203.5 µs median at 256 KB (>2x faster than `memmap2`);
- **`memmap2` regression**: 433.2 µs at 256 KB caused by 0x40 minor page faults and
  `UnmapViewOfFile` TLB shootdowns; memory mapping is strictly avoided;
- **`std::fs` overhead**: eliminates redundant `GetFileInformationByHandleEx` metadata queries
  on the hot path by speculatively issuing a single `ReadFile` into a pre-allocated 64 KB buffer;

### Implementation Details
- **Sharing Semantics**: uses `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE` to prevent
  `ERROR_SHARING_VIOLATION` when external editors (VS Code, Neovim) perform atomic file swaps;
- **Zero-Allocation Hot Path**: `ConfigReader` retains pre-encoded wide path (`Vec<u16>`) and
  reusable heap buffer (`Vec<u8>`), returning borrowed `&str` for zero-copy parsing;
- **Two-Pass Handling**: 64 KB fast path handles >99% of user configs in a single syscall;
  slow path queries `GetFileSizeEx` and resizes buffer only when file size exceeds 64 KB;
- **Sanitization**: strips UTF-8 byte order marks (`0xEF, 0xBB, 0xBF`) and validates UTF-8
  in-place prior to parser consumption;

---

## 7. Parser Engine

*Status: Implemented*

### Architectural Decision

Shyena implements a **Hand-Rolled Zero-Copy Byte Scanner** operating directly over the UTF-8 `&str`
provided by the Section 6 File I/O subsystem; it rejects parser combinator frameworks (`winnow`,
`nom`) to achieve sub-10 µs parsing latency, zero heap allocations, and zero external dependencies.

### Parsing Strategy: Top-Level Extraction

The engine does *not* build an Abstract Syntax Tree (AST) for the entire PowerShell language;
instead, it performs **Top-Level Boundary Extraction**:

1. **Directive Scanning**: Scans for `# shyena:v1` to establish schema rules;
2. **Block Lexing**: Identifies `Register-App` and `function` keywords;
3. **Zero-Copy Slicing**: Uses a string-, comment-, and escape-aware brace counter to find the
   exact closing `}` of a block; it slices the original `&'a str` and passes the unmodified script
   block to the PowerShell automation worker.

### PowerShell Lexical Compliance Rules

To accurately track balanced braces (`{` / `}`) without being tricked by characters inside strings
or comments, the scanner strictly implements these PowerShell escaping rules:

* **Double-Quoted Strings (`"..."`)**: Can contain braces; the escape character is the backtick
  (``` ` ```); an escaped quote (``` `" ```) does not terminate the string;
* **Single-Quoted Strings (`'...'`)**: Can contain braces; backticks are literal here; the escape
  sequence is a doubled single quote (`''`);
* **Line Comments (`#`)**: Terminate at `\n` or `\r\n`;
* **Block Comments (`<# ... #>`)**: Can span multiple lines and swallow braces.

### Diagnostic Subsystem (Section 9 Integration)

The parser tracks `line` and `col` coordinates; when encountering a malformed `Register-App`
hashtable or an unclosed function block, it yields a `ParseError` containing the exact span,
allowing the GPUI UI to render an inline red diagnostic banner without crashing the application.

### Implementation Details
- **Module Location**: `src/config/parser.rs`, exposed via `src/config/mod.rs`;
- **Core Types**:
  - `Scanner<'a>`: zero-copy parser maintaining `&'a str`, byte slice, line, and line start tracking;
  - `ConfigAst<'a>`: contains extracted `apps: Vec<AppDef<'a>>` and `functions: Vec<FunctionDef<'a>>`;
  - `AppDef<'a>`: stores raw literal name and unmodified hashtable block span; provides `unquoted_name()`;
  - `FunctionDef<'a>`: stores function identifier and balanced `{ ... }` block span;
  - `ParseError`: structured diagnostic carrying 1-based `line`, `col`, and static `message`;
- **Zero Allocations on Hot Path**: string and block extraction produces borrowed slices (`&'a str`)
  directly from the underlying buffer provided by `ConfigReader`;

---

## 8. Process Lifecycle & Windows System Integration

*Status: Partially Implemented*

### Implemented Subsystems
- **Global Hotkey Registration**: `global-hotkey` crate registers `Alt + Backquote` (`Alt + ~`) at
  application initialization; background thread captures events and dispatches activation pulses;
- **DWM Frame & Geometry**: `DwmExtendFrameIntoClientArea` with 1px margin triggers Windows
  hardware drop shadow; `DWMWCP_DONOTROUND` enforces 0px sharp corners; `HWND_TOPMOST` maintains
  foreground window focus;
- **Diagnostic-Aware Execution**: Worker thread processes `PsTask` queue by constructing a
  self-contained execution block (`& { function ... }`); strictly encodes the payload to
  UTF-16LE Base64 and executes via Windows PowerShell 5.1 (`pwsh` as fallback) using
  `-ExecutionPolicy Bypass` and `CREATE_NO_WINDOW` (`0x0800_0000`);
- **Error Surfacing**: `dispatch_ps_function` captures `stderr` through redirected temp files
  (avoiding GUI-child pipe hangs) and returns it on non-zero exit; the worker sends a condensed
  `PsError` over an `async-channel` to the GPUI diagnostic overlay instead of writing a log file;
- **Missing-Argument Guard**: Functions indexing `$args[...]` are rejected before spawning
  PowerShell when no dynamic tokens were supplied, preventing `[int]$null` -> `0` coercion;
  functions with mandatory parameters rely on PowerShell's own binding error;
- **Native Process Spawn**: Direct invocation of binary paths with static and dynamic arguments
  via `std::process::Command::spawn()`;

### Pending Considerations
- Dedicated system tray icon and background daemon lifecycle without persistent console windows;
- Avoiding Session 0 isolation issues during elevated background operation;
- Logon autostart strategy (Registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` vs
  Scheduled Task with interactive elevation);

---

## 9. Error Diagnostics & Visual Reporting

*Status: Implemented (execution and configuration errors)*

### Error State Machine
- **Idle**: Normal search bar with query, action tag, and candidate list;
- **Error**: A failed command replaces the bar in place with a red-framed `shyena::error` panel
  showing one condensed message line; input is ignored while this state is active;
- **Config Error**: A config load/reload failure shows a `shyena::config_error` panel marked
  `[ ESC: close ]`. It is non-dismissible: `[Esc]` removes the window entirely so the search bar
  can never reappear while the configuration is invalid;
- **Dismissal**: `[Esc]` on a runtime error clears it and returns to an empty `Idle` bar in the same
  window, ready for a retry;
- **Late Errors**: If the launcher was already dismissed when the worker reports the failure, a
  new window is opened directly in the `Error` state;

### Reported Failures
- **Missing dynamic arguments**: Zero-token invocations of functions that index `$args[...]` are
  rejected locally; mandatory `param(...)` declarations produce PowerShell's binding error;
- **PowerShell failures**: Non-zero exit status / `stderr` (including Windows PowerShell 5.1
  CLIXML records) is reduced to its first meaningful line, whitespace-collapsed, truncated to
  120 characters, and prefixed with the command name;
- **Configuration errors**: `ConfigStore::load()` parse/IO failures are cached as a diagnostic and
  shown the first time the launcher opens; `#config-reload` failures render the same overlay, and a
  successful reload clears it;

---

## 10. Configuration Loading & Manual Reloading

*Status: Implemented*

### Architectural Design

Shyena is a long-running, logon-started process, so the configuration is read exactly once during
application initialization. The parsed `Config` is cached in memory (`ConfigStore`) and every
launcher activation is served from that cache: opening the search window performs no filesystem I/O,
spawns no watcher threads, and schedules no timers.

### Initial Load & First-Run Seeding

`ConfigStore::load()` runs in `main.rs` before the hotkey/event loop starts. If `~/.config/.shyena`
is absent, the default configuration is written and cached; otherwise the file is read with the §6
single-pass Win32 reader and parsed immediately. Parse/I/O failures are retained as a diagnostic
while an empty fallback config keeps the UI renderable.

### Manual Reload (`#config-reload`)

The reserved action string (`RESERVED_RELOAD_CMD = "config-reload"`) resolves before any config
lookup, so the command remains reachable even when the cached config is empty. Invoking it
synchronously re-reads and re-parses the file on the UI thread (~200 µs, §6), swaps the cached
`Arc<Config>` and theme, and closes the launcher. A failure leaves the last known-good config active
and renders the fatal config error panel in place.

### Failure Recovery

Input is ignored while a fatal config error is displayed, so a failed load is retried when the
launcher opens: `open_search_window` calls `ConfigStore::reload()` only when the cached diagnostic is
set. Fixing the file and reopening therefore recovers without restarting Shyena, while the healthy
path stays I/O-free.

### Trade-offs

- External edits are not observed automatically; apply them via `#config-reload` (or by repairing an
  invalid file and reopening the launcher).
- Manual reload is naturally compatible with atomic editor saves, since the file is reopened by path
  rather than watched through a handle.

### Implementation Details
- **Module Location**: `src/config/store.rs`, re-exported via `src/config/mod.rs`;
- **Core Types**:
  - `ConfigStore`: cached `Arc<Config>`, reusable `ConfigReader`, and the last-load diagnostic;
  - `ReloadOutcome`: `Updated(Arc<Config>)` or `Error(String)`;
- **UI Integration**: `SearchBar` holds an `Rc<RefCell<ConfigStore>>`; `#config-reload` calls
  `reload()` and either applies the new config or shows the fatal error panel.

---

## 11. Installation & System Provisioning

### Overview & Security Boundaries
- Non-elevated execution context (standard user token) to maintain User Interface Privilege
  Isolation (UIPI) parity with target desktop applications and window messaging;
- Deterministic installation root: `%LOCALAPPDATA%\Programs\shyena\shyena.exe`;
- User-level environment integration via User `PATH` and `HKCU` registry hives;

### Process Lifecycle & Installation Pipeline (`install.ps1`)
1. **Environment Validation**:
   - Verification of 64-bit architecture (`[Environment]::Is64BitOperatingSystem`);
   - Windows 10/11 build constraint verification (`Build >= 19041` for modern DWM/Composition APIs);
   - Elevation rejection: warns or prevents running under elevated Administrator token;
2. **Locked Executable Mitigation**:
   - Windows returns `ERROR_ACCESS_DENIED` (`0x05`) on writes to active PE files;
   - Terminate active `shyena` instances before replacement via `Stop-Process`;
3. **Binary Deployment**:
   - Copy `shyena.exe` to `$env:LOCALAPPDATA\Programs\shyena\shyena.exe`;
   - Append installation directory to user environment variable `Path`;
4. **Configuration Seeding**:
   - Primary target: `~/.config/.shyena` (`$env:USERPROFILE\.config\.shyena`);
   - Writes default theme and initial `@app` / `#ps` bindings only if the file is absent;
5. **Autostart Persistence (HKCU Run Key)**:
   - Registers entry under `HKCU:\Software\Microsoft\Windows\CurrentVersion\Run`:
     - Value Name: `"Shyena"`;
     - Value Type: `REG_SZ`;
     - Value Data: `"\"$env:LOCALAPPDATA\Programs\shyena\shyena.exe\""` (strictly quoted);
   - Native integration with Windows Task Manager Startup Apps tab without helper services;
   - Zero background polling or daemon overhead; executed once by `explorer.exe` at logon;
6. **Immediate Detached Launch**:
   - Invoked via `Start-Process -FilePath $binTarget -WorkingDirectory $installDir`;
   - Zero console flash when binary uses `#![windows_subsystem = "windows"]` (`0x02`);

---

## 12. Update Mechanism

### Architectural Strategy
- Self-contained updater routine or re-entrant `install.ps1`;
- Graceful termination of the active instance prior to binary overwrite;
- Validation of release assets against expected cryptographic checksums before replacement;

---

## 13. Uninstallation & System Cleanup

### Architectural Strategy (`uninstall.ps1`)
- Terminate any running `shyena.exe` processes;
- Unregister the autostart key from `HKCU:\Software\Microsoft\Windows\CurrentVersion\Run`;
- Remove the installation directory (`%LOCALAPPDATA%\Programs\shyena`);
- Clean user `Path` environment variable;
- Retain `~/.config/.shyena` by default to preserve user definitions unless explicit `-PurgeConfig`
  flag is provided;
