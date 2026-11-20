# Changelog

All notable user-facing changes to Shyena are documented in this file;

## [0.0.101] - 2026-11-20

### Added
- PowerShell functions that produce output now display their results in a panel below the search bar, dismissible with <kbd>Esc</kbd>;
- New `:quit` internal command to close Shyena for the current session (it restarts automatically at the next login);
- New default `calc` function for evaluating arithmetic expressions (e.g. `# calc 2 + 2`);
- New `shutdown` and `restart` functions in the default configuration, each guarded by a confirmation prompt;
- New `light` function to set display brightness (e.g. `# light 40`);

### Changed
- The default configuration is now general-purpose and portable across Windows machines: machine-specific launcher examples are left as comments, and several utility functions (`volume-up`, `volume-down`, `setclip`, `uuid`, `stamp`, `flushdns`, `hash`, `killproc`) were removed — copy any you need into your own config;
- Arrow-key navigation through suggestions now wraps within the visible list instead of jumping to off-screen items;
- Duplicate app or function names in a configuration file are now reported as an error instead of being silently accepted;

## [0.0.100] - 2026-11-19

### Added
- Keyboard-first launcher summoned via global hotkey (<kbd>Alt</kbd> + <kbd>`</kbd>);
- Fast centered search bar with acrylic backdrop blur and Catppuccin theme support;
- Command routing scopes for executables (`@`), PowerShell automation (`#`), and controls (`:`);
- Candidate suggestions dropdown with keyboard navigation and scope-preserving tab completion;
- Dynamic argument passing to registered applications and PowerShell functions;
- Inline execution error reporting with immediate keyboard dismissal;
- Built-in `:reload` command for configuration updates and `:update` for in-place upgrades;
- Declarative `.shyena` configuration supporting custom themes and suggestions toggling;
- One-line PowerShell installer, updater with SHA256 integrity checks, and uninstaller;
