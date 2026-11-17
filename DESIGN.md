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
- **Color Palette**: Catppuccin Mocha semantic token palette (dark matte default) with Latte support;
  green accents for `@app` and search glyphs;

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
- Primary path: `~/.config/.shyena` (or `~/.config/.shyena.ps1`);

### Language Construct Rules
1. **Application Registration (`Register-App`)**:
   Static definitions for binaries and fixed arguments with zero dynamic parameter logic:
   ```powershell
   Register-App 'code' @{
       Path = 'C:/Users/froze/AppData/Local/Programs/Microsoft VS Code/Code.exe'
       Args = @('--reuse-window')
   }
   ```
2. **PowerShell Automation Functions (`function <name>`)**:
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

The configuration pipeline targets `~/.config/.shyena` (fallback `~/.config/.shyena.ps1`),
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
- **Detached Execution**: Worker thread processes `PsTask` queue, spawning `pwsh` with
  `CREATE_NO_WINDOW` (`0x0800_0000`) and automatic fallback to `powershell`;
- **Native Process Spawn**: Direct invocation of binary paths with static and dynamic arguments
  via `std::process::Command::spawn()`;

### Pending Considerations
- Dedicated system tray icon and background daemon lifecycle without persistent console windows;
- Avoiding Session 0 isolation issues during elevated background operation;
- Logon autostart strategy (Registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` vs
  Scheduled Task with interactive elevation);

---

## 9. Error Diagnostics & Visual Reporting

*Status: Not Yet Decided*

### Architectural Considerations
- Layout and styling of in-place error presentation inside the search bar interface;
- Handling configuration syntax/parse errors gracefully during startup and reloading;
- Capturing PowerShell execution errors/tracebacks and presenting them cleanly with `[Esc]` dismissal behavior;

---

## 10. Configuration Hot-Reloading

*Status: Not Yet Decided*

### Architectural Considerations
- In-memory reload triggered by a reserved `#config-reload` action;
- Automated background filesystem watching using Win32 `ReadDirectoryChangesW`;
- Debounce mechanisms to prevent parsing intermediate write states;

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
