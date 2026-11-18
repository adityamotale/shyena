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
  - `GeistMonoNerdFontMono-Italic.otf` — Italic base weight (400)
  - `LICENSE-GeistMono.txt` — SIL Open Font License 1.1 (OFL)

### In-Memory Font Loading
- Embedded into the binary via `include_bytes!` to eliminate system font installation requirements;
- Loaded at GPUI startup using `cx.text_system().add_fonts(...)`;
- Cascades from root container via `.font_family("GeistMono Nerd Font Mono")`;
- Strict 1-cell monospace icon width guarantees alignment across ASCII/TUI framing (`┌─[`, `─┐`), prompts (`❯`), and custom cursor blocks (`█`);

### OFL 1.1 Compliance & Distribution
- SIL Open Font License requires preserving copyright and license notices upon redistribution;
- Source tree retains `assets/fonts/LICENSE-GeistMono.txt`;
- Binary distributions must bundle license text within the distribution archive;

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
  dismisses it and returns to an empty `Idle` bar (see §9);

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

*Status: Not Yet Decided*

### Architectural Considerations
- Need for explicit header directives (e.g. `# shyena:v1`);
- Graceful handling of unknown configuration directives;
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

*Status: Implemented (execution errors)*

### Error State Machine
- **Idle**: Normal search bar with query, action tag, and candidate list;
- **Error**: A failed command replaces the bar in place with a red-framed `shyena::error` panel
  showing one condensed message line; input is ignored while this state is active;
- **Dismissal**: `[Esc]` clears the error and returns to an empty `Idle` bar in the same window,
  ready for a retry;
- **Late Errors**: If the launcher was already dismissed when the worker reports the failure, a
  new window is opened directly in the `Error` state;

### Reported Failures
- **Missing dynamic arguments**: Zero-token invocations of functions that index `$args[...]` are
  rejected locally; mandatory `param(...)` declarations produce PowerShell's binding error;
- **PowerShell failures**: Non-zero exit status / `stderr` (including Windows PowerShell 5.1
  CLIXML records) is reduced to its first meaningful line, whitespace-collapsed, truncated to
  120 characters, and prefixed with the command name;
- **Configuration errors**: `Config::load` parse failures currently log to `stderr` and fall back
  to an empty config; the §10 watcher emits `ReloadEvent::Error`, but routing it through this
  overlay is still pending;

---

## 10. Configuration Hot-Reloading

*Status: Implemented*

### Architectural Design

Shyena implements zero-dependency, low-latency configuration hot-reloading backed by native Win32
`ReadDirectoryChangesW` directory monitoring and a dedicated trailing-edge debounce pipeline;
it coordinates file system event listening, debounced parsing, and GPUI snapshot swaps.

### Directory Watching & Atomic Save Resilience

Modern code editors (VS Code, Neovim, Zed) save files via atomic swaps: writing to an ephemeral
file before deleting/renaming the target; direct file handle watching fails across replacements.

Shyena watches the parent directory (`~/.config/`) using `CreateFileW` with `FILE_LIST_DIRECTORY`,
`OPEN_EXISTING`, and `FILE_FLAG_BACKUP_SEMANTICS`; notifications are received in a 4-byte aligned
4 KB buffer (`[u32; 1024]`), filtering `FILE_NOTIFY_INFORMATION` records for `.shyena`.

### Trailing-Edge Debouncing Pipeline

Rapid write bursts (e.g. truncation followed by write and atomic rename within ~10 ms) are
coalesced using a 75 ms trailing-edge debouncer on a dedicated background thread:

1. **Burst Absorption**: An initial `FsChanged` arm resets a 75 ms timeout window;
2. **Quiescence Execution**: Once the 75 ms window expires with no further I/O activity, the
   debouncer invokes `ConfigReader::read()` and `Scanner::parse()`;
3. **Owned Snapshot Conversion**: Borrows from `ConfigReader` are transformed into an owned
   `ConfigSnapshot` (`OwnedAppDef`, `OwnedFunctionDef`) dispatched across an `async-channel`;
4. **Diagnostic Isolation**: In case of I/O or `ParseError`, a `ReloadEvent::Error` variant is
   emitted to display UI diagnostics while keeping the previous valid snapshot active.

### Reserved `#config-reload` Command

A thread-safe `ReloadTrigger` handle and reserved action string (`RESERVED_RELOAD_CMD = "config-reload"`)
allow instant manual reloads without awaiting timer expiration.

### Implementation Details
- **Module Location**: `src/config/watcher.rs`, exposed via `src/config/mod.rs`;
- **Core Types**:
  - `ConfigSnapshot`: owned snapshot containing `apps: Vec<OwnedAppDef>` and `functions: Vec<OwnedFunctionDef>`;
  - `ReloadError`: encapsulates I/O errors and zero-copy `ParseError` diagnostics;
  - `ReloadEvent`: enum of `Updated(ConfigSnapshot)` and `Error(ReloadError)`;
  - `ReloadTrigger`: `mpsc::Sender<WatchSignal>` wrapper for instantaneous manual triggers;
  - `spawn_config_watcher()`: spawns watcher and debouncer threads, returning trigger and event receiver.

---

## 11. Installation & System Provisioning

*Status: Not Yet Decided*

### Architectural Considerations
- Clean, one-liner PowerShell installation script (`install.ps1`);
- Deterministic installation directories (e.g. `%LOCALAPPDATA%\shyena\`);
- Seeding default `.shyena` configuration templates upon initial installation;

---

## 12. Update Mechanism

*Status: Not Yet Decided*

### Architectural Considerations
- In-place binary replacement strategy on Windows without encountering locked-executable errors (`ERROR_ACCESS_DENIED`);
- Release asset verification and automated fetching from release channels;

---

## 13. Uninstallation & System Cleanup

*Status: Not Yet Decided*

### Architectural Considerations
- Dedicated uninstallation script (`uninstall.ps1`);
- Complete removal of autostart keys, registered shortcuts, cached binaries, and configuration folders;
