# Manual smoke test

Automated tests cover the UI, command dispatch, window lifecycle, async pumps,
and all side effects through `RecordingEffects` (see `cargo test --all-targets`).
The following require a real Windows desktop and are intentionally not automated:

- registering a global hotkey with the OS,
- DWM shadow, corner and topmost behaviour,
- spawning real applications and PowerShell processes,
- visual appearance of both themes.

Run `cargo build --release` and start the binary, then work through each case.
Note: release builds read the installed `~/.config/.shyena`, while debug builds
(`cargo run`) read the repository-root `.shyena` instead. Edit the file matching
your build when testing configuration reload.

## 1. Hotkey and window lifecycle

1. Press **Alt+`**. The launcher appears centred near the top of the screen.
2. Press it again. The launcher disappears; the process stays alive.
3. Press it a third time. It reopens without startup delay.

## 2. Query and keyboard flow

1. Type `@calc` — the candidate list narrows to the registered app.
2. Press **Tab** — the query is completed and the scope prefix is preserved.
3. Press **Enter** — Calculator starts and the launcher closes.
4. Reopen, type `#brightness 50`, press **Enter** — screen brightness changes;
   no error panel appears.
5. Reopen, type `#brightness` with no argument and press **Enter** — the
   mandatory-parameter error appears in a single line. Press **Esc** — the
   launcher returns to an empty bar rather than closing.

## 3. Configuration reload

1. Open `~/.config/.shyena` and change `Set-Theme` to the opposite theme.
2. In the launcher, type `:reload` and press **Enter** — the window closes.
3. Reopen — the new theme is visible.
4. Introduce a syntax error, reopen the launcher — a fatal config panel appears.
   **Esc** must close the launcher entirely.
5. Fix the file, press the hotkey — the launcher recovers without a restart.

## 4. Suggestions off

1. Set `Set-Suggestions 'off'`, reload.
2. Type `@calc` — no dropdown appears, but Enter still launches the app.

## 5. Focus behaviour

1. Open the launcher and click another application — the launcher closes.
2. Reopen and press **Esc** — the launcher closes, process remains running.

## Results

Record any deviation from the expected behaviour above, including the config
file contents and the PowerShell version (`$PSVersionTable.PSVersion`), before
filing an issue.
