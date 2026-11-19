#![windows_subsystem = "windows"]

//! Shyena: a global-hotkey launcher built on GPUI.
//!
//! The process starts at login and stays alive behind a hidden anchor window.
//! A hotkey thread opens the search bar, the UI resolves a query into an app,
//! PowerShell function, or in-process command, and worker-thread failures are
//! delivered back through an async channel for display in the same window.

use async_channel::{Receiver, Sender};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use gpui::{
    App, AppContext, Application, Bounds, Context, Div, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Render, Result, Rgba, Styled,
    Subscription, Timer, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, div, point, px, size,
};
use std::{cell::RefCell, rc::Rc, sync::Arc, thread, time::Duration};

mod commands;
mod config;
mod exec;
mod theme;
use commands::InternalCommand;
use config::{ConfigStore, ReloadOutcome};
use theme::{Palette, Theme};

const WIN_WIDTH: f32 = 660.0;
const BAR_HEIGHT: f32 = 62.0;
const ITEM_HEIGHT: f32 = 32.0;
const MAX_VISIBLE_ITEMS: usize = 6;
/// One-line cap for every error rendered in the 62 px panel.
pub(crate) const ERROR_LINE_MAX_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    App,
    Ps,
    Internal,
}

/// One resolvable entry in the candidate list (app, PowerShell function, or
/// built-in command). Kept `Clone` because the render closure owns a copy.
#[derive(Clone)]
pub struct CandidateAction {
    pub key: String,
    pub kind: ActionKind,
    pub detail: String,
}

/// Query prefix namespaces. `:` addresses built-in commands; apps and PowerShell
/// functions keep their existing `@` / `#` scopes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    App,
    Ps,
    Internal,
}

impl Scope {
    fn from_char(ch: char) -> Option<Self> {
        match ch {
            '@' => Some(Self::App),
            '#' => Some(Self::Ps),
            ':' => Some(Self::Internal),
            _ => None,
        }
    }

    fn prefix(self) -> char {
        match self {
            Self::App => '@',
            Self::Ps => '#',
            Self::Internal => ':',
        }
    }
}

/// Splits a single leading scope prefix from the query. Both `@app` and
/// `@ app` forms are accepted; callers trim the returned remainder as needed.
fn split_scope(input: &str) -> Option<(Scope, &str)> {
    let trimmed = input.trim_start();
    let ch = trimmed.chars().next()?;
    let scope = Scope::from_char(ch)?;
    Some((scope, &trimmed[ch.len_utf8()..]))
}

/// Win32 window styling and process-memory housekeeping.
///
/// All FFI calls here are best-effort: a failure must never prevent the
/// launcher from opening, so results are deliberately discarded.
mod platform {
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmExtendFrameIntoClientArea,
        DwmSetWindowAttribute,
    };
    use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
    use windows::Win32::System::Threading::GetCurrentProcess;
    use windows::Win32::UI::Controls::MARGINS;
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos,
    };

    pub fn setup_window(window: &Window) {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };

        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };

        let hwnd = HWND(win32.hwnd.get() as *mut std::ffi::c_void);

        // A 1px extended frame lets DWM draw its hardware drop shadow around
        // an otherwise borderless popup.
        let margins = MARGINS {
            cxLeftWidth: 1,
            cxRightWidth: 1,
            cyTopHeight: 1,
            cyBottomHeight: 1,
        };
        // SAFETY: `hwnd` is the live window handle provided by GPUI and
        // `margins` is a valid initialized struct passed by pointer.
        let _ = unsafe { DwmExtendFrameIntoClientArea(hwnd, &raw const margins) };

        // Square corners match the terminal aesthetic of the UI.
        let corner_pref = DWMWCP_DONOTROUND;
        // SAFETY: the pointer and byte count describe the live `corner_pref`
        // value and match the DWMWA_WINDOW_CORNER_PREFERENCE attribute type.
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const corner_pref).cast::<std::ffi::c_void>(),
                u32::try_from(std::mem::size_of_val(&corner_pref)).unwrap_or(u32::MAX),
            )
        };
    }

    pub fn make_always_on_top(window: &Window) -> bool {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return false;
        };

        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return false;
        };

        let hwnd = HWND(win32.hwnd.get() as *mut std::ffi::c_void);
        // SAFETY: `hwnd` is the live window handle supplied by GPUI. The flags
        // leave geometry untouched, so no invalid size/position is passed.
        unsafe {
            let res = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0x00,
                0x00,
                0x00,
                0x00,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
            let _ = SetForegroundWindow(hwnd);
            res.is_ok()
        }
    }

    /// Ask Windows to evict this process's idle pages back to standby. This
    /// lowers the working set only: committed memory and GPU allocations stay
    /// resident, and the pages fault back in when the launcher next opens.
    pub fn trim_working_set() {
        // SAFETY: `GetCurrentProcess` returns a pseudo-handle that requires no
        // closing; `K32EmptyWorkingSet` only touches this process's pages.
        unsafe {
            let _ = K32EmptyWorkingSet(GetCurrentProcess());
        }
    }
}

/// Trim only while no launcher window is open. The delay keeps the syscall off
/// the teardown/next-frame path, and the guard cancels a pending trim when the
/// user reopens the bar within that window.
fn schedule_trim(
    cx: &App,
    active_window: &Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
    delay: Duration,
) {
    let active = active_window.clone();
    cx.spawn(async move |_| {
        Timer::after(delay).await;
        if active.borrow().is_none() {
            platform::trim_working_set();
        }
    })
    .detach();
}

/// A PowerShell failure delivered from a worker thread to the UI channel.
#[derive(Clone)]
pub struct PsError {
    pub message: String,
}

/// Runtime failures are dismissible with [Esc]; config failures are fatal for
/// the current launcher session and close the window on [Esc] instead.
enum ErrorState {
    Runtime(String),
    Config(String),
}

impl ErrorState {
    fn message(&self) -> &str {
        match self {
            Self::Runtime(message) | Self::Config(message) => message,
        }
    }

    fn is_config(&self) -> bool {
        matches!(self, Self::Config(_))
    }
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum CommandTarget {
    App {
        key: String,
        args: Vec<String>,
    },
    Ps {
        key: String,
        args: Vec<String>,
    },
    Internal {
        command: InternalCommand,
        args: Vec<String>,
    },
}

fn candidate_target(candidate: &CandidateAction, args: Vec<String>) -> CommandTarget {
    match candidate.kind {
        ActionKind::App => CommandTarget::App {
            key: candidate.key.clone(),
            args,
        },
        ActionKind::Ps => CommandTarget::Ps {
            key: candidate.key.clone(),
            args,
        },
        ActionKind::Internal => {
            let spec = commands::find(&candidate.key).expect("internal candidate exists");
            CommandTarget::Internal {
                command: spec.command,
                args,
            }
        }
    }
}

/// Resolves a committed query into an executable target. Candidate fallback
/// preserves the existing "type a prefix, press Enter" flow; an unmatched
/// token becomes an explicit error instead of a silent stdout print.
fn resolve_command(
    config: &config::Config,
    query: &str,
    selected: Option<&CandidateAction>,
) -> Result<CommandTarget, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return selected
            .map(|candidate| candidate_target(candidate, Vec::new()))
            .ok_or_else(|| "empty command".to_string());
    }

    let (scope, rest) = match split_scope(trimmed) {
        Some((scope, rest)) => (Some(scope), rest.trim_start()),
        None => (None, trimmed),
    };

    let mut tokens = rest.split_whitespace();
    let key = tokens.next().unwrap_or("");
    if key.is_empty() {
        return selected
            .map(|candidate| candidate_target(candidate, Vec::new()))
            .ok_or_else(|| "expected command after prefix".to_string());
    }

    let args = tokens.map(str::to_string).collect::<Vec<_>>();

    if scope == Some(Scope::Internal) {
        if let Some(spec) = commands::find(key) {
            return Ok(CommandTarget::Internal {
                command: spec.command,
                args,
            });
        }

        return selected
            .filter(|candidate| candidate.kind == ActionKind::Internal)
            .map(|candidate| candidate_target(candidate, args))
            .ok_or_else(|| format!("unknown command '{key}'"));
    }

    if scope != Some(Scope::Ps) && config.apps.contains_key(key) {
        return Ok(CommandTarget::App {
            key: key.to_string(),
            args,
        });
    }

    if scope != Some(Scope::App) && config.ps.contains_key(key) {
        return Ok(CommandTarget::Ps {
            key: key.to_string(),
            args,
        });
    }

    if let Some(candidate) = selected {
        return Ok(candidate_target(candidate, args));
    }

    Err(format!("unknown command '{key}'"))
}

/// Runs one PowerShell function and reports failures through the shared error
/// channel. Blocking: callers must run this off the UI thread.
fn run_ps_task(
    function_name: &str,
    function_body: &str,
    dynamic_tokens: &[String],
    error_tx: &Sender<PsError>,
) {
    let args: Vec<&str> = dynamic_tokens.iter().map(String::as_str).collect();
    if let Err(err) = exec::dispatch_ps_function(function_name, function_body, &args) {
        let _ = error_tx.send_blocking(PsError {
            message: exec::format_ps_error(function_name, &err),
        });
    }
}

/// Hidden window that keeps GPUI's run loop (and the hotkey/error channels)
/// alive after the launcher window is dismissed.
struct AnchorWindow;

impl Render for AnchorWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

struct SearchBar {
    focus_handle: FocusHandle,
    query: String,
    was_active: bool,
    _subscription: Subscription,
    active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
    config: Arc<config::Config>,
    ps_error_tx: Sender<PsError>,
    store: Rc<RefCell<ConfigStore>>,
    theme: Theme,
    selected_index: usize,
    filtered_actions: Vec<CandidateAction>,
    error: Option<ErrorState>,
}

impl SearchBar {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
        ps_error_tx: Sender<PsError>,
        store: Rc<RefCell<ConfigStore>>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window);

        let config = store.borrow().current();
        let theme = config.theme;

        platform::setup_window(window);
        let _ = platform::make_always_on_top(window);

        let subscription = cx.observe_window_activation(window, |this, window, cx| {
            let active = window.is_window_active();
            if active {
                this.was_active = true;
            } else if this.was_active {
                this.close(window, cx);
            }
            cx.notify();
        });

        let mut instance = Self {
            focus_handle,
            query: String::new(),
            was_active: false,
            _subscription: subscription,
            active_window,
            config,
            ps_error_tx,
            store,
            theme,
            selected_index: 0,
            filtered_actions: Vec::new(),
            error: None,
        };
        instance.update_filtered_actions(window);
        instance
    }

    fn update_filtered_actions(&mut self, window: &mut Window) {
        let trimmed = self.query.trim();
        if trimmed.is_empty() {
            self.filtered_actions.clear();
            self.selected_index = 0;
            Self::resize_to_height(window, BAR_HEIGHT);
            return;
        }

        // Suggestions off: keep the bar at base height and skip candidate
        // evaluation. Exact-key resolution on Enter still works via
        // `resolve_command`.
        if !self.config.suggestions.enabled() {
            self.filtered_actions.clear();
            self.selected_index = 0;
            Self::resize_to_height(window, BAR_HEIGHT);
            return;
        }

        let (scope, rest) = match split_scope(trimmed) {
            Some((scope, rest)) => (Some(scope), rest.trim_start()),
            None => (None, trimmed),
        };
        let target_token = rest.split_whitespace().next().unwrap_or("");

        let mut candidates = Vec::new();
        if matches!(scope, None | Some(Scope::App)) {
            for (key, app) in &self.config.apps {
                if target_token.is_empty()
                    || key.to_lowercase().contains(&target_token.to_lowercase())
                {
                    candidates.push(CandidateAction {
                        key: key.clone(),
                        kind: ActionKind::App,
                        detail: app.path.display().to_string(),
                    });
                }
            }
        }

        if matches!(scope, None | Some(Scope::Ps)) {
            for (key, ps) in &self.config.ps {
                if target_token.is_empty()
                    || key.to_lowercase().contains(&target_token.to_lowercase())
                {
                    let clean_script = ps.script.trim().replace('\n', " ");
                    let detail = if clean_script.len() > 60 {
                        format!("{}...", &clean_script[..57])
                    } else {
                        clean_script
                    };
                    candidates.push(CandidateAction {
                        key: key.clone(),
                        kind: ActionKind::Ps,
                        detail,
                    });
                }
            }
        }

        if scope == Some(Scope::Internal) {
            for spec in commands::matching(target_token) {
                candidates.push(CandidateAction {
                    key: spec.name.to_string(),
                    kind: ActionKind::Internal,
                    detail: spec.description.to_string(),
                });
            }
        }

        candidates.sort_by(|a, b| {
            let a_exact = a.key.eq_ignore_ascii_case(target_token);
            let b_exact = b.key.eq_ignore_ascii_case(target_token);
            let a_prefix = a
                .key
                .to_lowercase()
                .starts_with(&target_token.to_lowercase());
            let b_prefix = b
                .key
                .to_lowercase()
                .starts_with(&target_token.to_lowercase());

            b_exact
                .cmp(&a_exact)
                .then_with(|| b_prefix.cmp(&a_prefix))
                .then_with(|| a.key.cmp(&b.key))
        });

        self.filtered_actions = candidates;
        if self.selected_index >= self.filtered_actions.len() {
            self.selected_index = 0;
        }

        let visible_items = self.filtered_actions.len().min(MAX_VISIBLE_ITEMS);
        let list_height = if visible_items > 0 {
            f32::from(u8::try_from(visible_items).unwrap_or(u8::MAX)) * ITEM_HEIGHT + 1.0
        } else {
            0.0
        };
        Self::resize_to_height(window, BAR_HEIGHT + list_height);
    }

    /// Resizes only when the change is visible, so typing within the same
    /// candidate count does not trigger a window resize on every keystroke.
    fn resize_to_height(window: &mut Window, height: f32) {
        let target = px(height);
        let current = window.bounds().size;
        let diff: f32 = (current.height - target).into();
        if diff.abs() > 0.5 {
            window.resize(size(current.width, target));
        }
    }

    fn set_error(&mut self, error: ErrorState, window: &mut Window, cx: &mut Context<Self>) {
        self.error = Some(error);
        self.query.clear();
        self.filtered_actions.clear();
        self.selected_index = 0;
        Self::resize_to_height(window, BAR_HEIGHT);
        self.focus_handle.focus(window);
        cx.notify();
    }

    fn show_error(&mut self, message: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_error(ErrorState::Runtime(message), window, cx);
    }

    /// Config errors are fatal: [Esc] must close the launcher instead of
    /// returning to an empty search bar.
    fn show_config_error(&mut self, message: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_error(ErrorState::Config(message), window, cx);
    }

    fn dismiss_error(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.query.clear();
        self.update_filtered_actions(window);
        self.focus_handle.focus(window);
        cx.notify();
    }

    /// A successful manual reload replaces the cached config and clears a config error.
    fn apply_config(
        &mut self,
        config: Arc<config::Config>,
        theme: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config = config;
        self.theme = theme;
        if self.error.as_ref().is_some_and(ErrorState::is_config) {
            self.error = None;
            self.query.clear();
        }
        self.update_filtered_actions(window);
        self.focus_handle.focus(window);
        cx.notify();
    }

    /// Dismiss the launcher and schedule an idle working-set trim. All internal
    /// close paths share this so the trim cannot be missed or applied twice.
    fn close(&self, window: &mut Window, cx: &mut Context<Self>) {
        *self.active_window.borrow_mut() = None;
        window.remove_window();
        schedule_trim(&*cx, &self.active_window, Duration::from_millis(200));
    }

    fn handle_key(
        this: &mut Self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(error) = &this.error {
            if event.keystroke.key == "escape" {
                if error.is_config() {
                    this.close(window, cx);
                } else {
                    this.dismiss_error(window, cx);
                }
            }
            return;
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                this.close(window, cx);
            }
            "down" => {
                if !this.filtered_actions.is_empty() {
                    this.selected_index = (this.selected_index + 1) % this.filtered_actions.len();
                    cx.notify();
                }
            }
            "up" => {
                if !this.filtered_actions.is_empty() {
                    this.selected_index = if this.selected_index == 0 {
                        this.filtered_actions.len() - 1
                    } else {
                        this.selected_index - 1
                    };
                    cx.notify();
                }
            }
            "tab" => {
                if let Some(candidate) = this.filtered_actions.get(this.selected_index) {
                    // Keep the active @/#/: scope so dynamic args still resolve
                    // to the same action when Enter is pressed.
                    let scope = split_scope(&this.query)
                        .map(|(scope, _)| scope.prefix().to_string())
                        .unwrap_or_default();
                    this.query = format!("{}{} ", scope, candidate.key);
                    this.update_filtered_actions(window);
                    cx.notify();
                }
            }
            "enter" => {
                this.execute(window, cx);
            }
            "backspace" => {
                this.query.pop();
                this.update_filtered_actions(window);
                cx.notify();
            }
            _ => {
                if let Some(ch) = &event.keystroke.key_char {
                    this.query.push_str(ch);
                } else if event.keystroke.key == "space" {
                    // GPUI reports Space as the immutable key "space" with no key_char.
                    this.query.push(' ');
                } else if event.keystroke.key.chars().count() == 1 {
                    this.query.push_str(&event.keystroke.key);
                } else {
                    return;
                }
                this.update_filtered_actions(window);
                cx.notify();
            }
        }
    }

    fn execute_action(
        &self,
        action: &CandidateAction,
        dynamic_args: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action.kind {
            ActionKind::App => {
                if let Some(app) = self.config.apps.get(&action.key) {
                    let mut cmd = std::process::Command::new(&app.path);
                    cmd.args(&app.args);
                    cmd.args(dynamic_args);
                    if let Err(err) = cmd.spawn() {
                        eprintln!("Failed to spawn {}: {}", app.path.display(), err);
                    }
                }
            }
            ActionKind::Ps => {
                if let Some(ps) = self.config.ps.get(&action.key) {
                    let name = action.key.clone();
                    let body = ps.body.clone();
                    let args = dynamic_args.to_vec();
                    let error_tx = self.ps_error_tx.clone();
                    if let Err(err) = thread::Builder::new()
                        .name("shyena-ps-task".into())
                        .spawn(move || run_ps_task(&name, &body, &args, &error_tx))
                    {
                        let _ = self.ps_error_tx.send_blocking(PsError {
                            message: format!("{}: failed to start task: {err}", action.key),
                        });
                    }
                }
            }
            // Internal commands are dispatched through `activate`, which needs
            // `&mut self` and the GPUI context.
            ActionKind::Internal => return,
        }

        self.close(window, cx);
    }

    /// Shared entry point for Enter and suggestion clicks so both paths handle
    /// app launches, PowerShell dispatch, and in-process commands identically.
    fn activate(
        &mut self,
        action: &CandidateAction,
        dynamic_args: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action.kind {
            ActionKind::Internal => {
                let spec = commands::find(&action.key).expect("internal candidate exists");
                self.run_internal(spec.command, dynamic_args, window, cx);
            }
            ActionKind::App | ActionKind::Ps => {
                self.execute_action(action, dynamic_args, window, cx);
            }
        }
    }

    fn run_internal(
        &mut self,
        command: InternalCommand,
        args: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            InternalCommand::Reload => {
                if !args.is_empty() {
                    self.show_error("reload takes no arguments".to_string(), window, cx);
                    return;
                }

                let outcome = self.store.borrow_mut().reload();
                match outcome {
                    ReloadOutcome::Updated(config) => {
                        let theme = config.theme;
                        self.apply_config(config, theme, window, cx);
                        self.close(window, cx);
                    }
                    ReloadOutcome::Error(message) => {
                        self.show_config_error(message, window, cx);
                    }
                }
            }
        }
    }

    fn execute(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.query.trim().is_empty() && self.filtered_actions.is_empty() {
            self.close(window, cx);
            return;
        }

        let target = match resolve_command(
            &self.config,
            &self.query,
            self.filtered_actions.get(self.selected_index),
        ) {
            Ok(target) => target,
            Err(message) => {
                self.show_error(message, window, cx);
                return;
            }
        };

        match target {
            CommandTarget::App { key, args } => {
                let app = self.config.apps.get(&key).expect("resolved app exists");
                let candidate = CandidateAction {
                    key,
                    kind: ActionKind::App,
                    detail: app.path.display().to_string(),
                };
                self.execute_action(&candidate, &args, window, cx);
            }
            CommandTarget::Ps { key, args } => {
                let ps = self.config.ps.get(&key).expect("resolved function exists");
                let candidate = CandidateAction {
                    key,
                    kind: ActionKind::Ps,
                    detail: ps.script.clone(),
                };
                self.execute_action(&candidate, &args, window, cx);
            }
            CommandTarget::Internal { command, args } => {
                self.run_internal(command, &args, window, cx);
            }
        }
    }
}

impl ActionKind {
    /// Short badge and colour used in the candidate list.
    fn badge(self, palette: Palette) -> (&'static str, Rgba) {
        match self {
            Self::App => ("APP", palette.accent),
            Self::Ps => ("PS", palette.ps),
            Self::Internal => ("SYS", palette.internal),
        }
    }

    /// Longer tag used in the input row when no candidate is selected.
    fn mode_tag(self, palette: Palette) -> (&'static str, Rgba) {
        match self {
            Self::App => ("LAUNCH", palette.accent),
            Self::Ps => ("EXEC", palette.ps),
            Self::Internal => ("SYSTEM", palette.internal),
        }
    }
}

impl From<Scope> for ActionKind {
    fn from(scope: Scope) -> Self {
        match scope {
            Scope::App => Self::App,
            Scope::Ps => Self::Ps,
            Scope::Internal => Self::Internal,
        }
    }
}

impl Render for SearchBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.theme.palette();
        let root = self.root_element(palette, cx);

        if let Some(error) = &self.error {
            return Self::render_error(root, error, palette);
        }

        let mode_tag = self.mode_tag(palette);
        root.child(Self::render_header(palette))
            .child(self.render_input(palette, mode_tag))
            .children(self.render_suggestions(palette, cx))
    }
}

impl SearchBar {
    fn root_element(&self, palette: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .track_focus(&self.focus_handle)
            .key_context("terminal_bar")
            .on_key_down(cx.listener(|this, event, window, cx| {
                Self::handle_key(this, event, window, cx);
            }))
            .size_full()
            .flex()
            .flex_col()
            .font_family("GeistMono Nerd Font Mono")
            .bg(palette.bg)
            .rounded(px(0.0))
            .border_1()
            .border_color(if self.error.is_some() {
                palette.error
            } else {
                palette.frame
            })
            .overflow_hidden()
    }

    fn mode_tag(&self, palette: Palette) -> Option<(&'static str, Rgba)> {
        let trimmed = self.query.trim();
        if trimmed.is_empty() {
            None
        } else if let Some((scope, _)) = split_scope(trimmed) {
            Some(ActionKind::from(scope).mode_tag(palette))
        } else if let Some(selected) = self.filtered_actions.get(self.selected_index) {
            Some(selected.kind.mode_tag(palette))
        } else if self
            .config
            .apps
            .contains_key(trimmed.split_whitespace().next().unwrap_or(""))
        {
            Some(ActionKind::App.mode_tag(palette))
        } else {
            Some(ActionKind::Ps.mode_tag(palette))
        }
    }

    /// Error state replaces the search bar entirely; [Esc] dismisses a runtime
    /// error or closes the window for a fatal config error.
    fn render_error(root: Div, error: &ErrorState, palette: Palette) -> Div {
        let label = if error.is_config() {
            "shyena::config_error"
        } else {
            "shyena::error"
        };
        let esc_hint = if error.is_config() {
            "[ ESC: close ]"
        } else {
            "[ ESC: continue ]"
        };

        root.child(
            div()
                .h(px(20.0))
                .px(px(10.0))
                .bg(palette.header)
                .border_b_1()
                .border_color(palette.error)
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(10.5))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(div().text_color(palette.error).child("┌─["))
                        .child(div().text_color(palette.error).child(label))
                        .child(div().text_color(palette.error).child("]")),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(div().text_color(palette.hint).child(esc_hint))
                        .child(div().text_color(palette.error).child("─┐")),
                ),
        )
        .child(
            div()
                .h(px(BAR_HEIGHT - 20.0))
                .flex()
                .items_center()
                .px(px(12.0))
                .gap(px(10.0))
                .child(
                    div()
                        .text_color(palette.error)
                        .text_size(px(14.0))
                        .child("✗"),
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(palette.text)
                        .text_size(px(12.5))
                        .child(error.message().to_string()),
                ),
        )
    }

    fn render_header(palette: Palette) -> Div {
        div()
            .h(px(20.0))
            .px(px(10.0))
            .bg(palette.header)
            .border_b_1()
            .border_color(palette.border_subtle)
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(10.5))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(div().text_color(palette.bracket).child("┌─["))
                    .child(div().text_color(palette.accent).child("shyena::cli"))
                    .child(div().text_color(palette.bracket).child("]")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(div().text_color(palette.hint).child("[ ESC: exit ]"))
                    .child(div().text_color(palette.bracket).child("─┐")),
            )
    }

    fn render_input(&self, palette: Palette, mode_tag: Option<(&'static str, Rgba)>) -> Div {
        div()
            .h(px(BAR_HEIGHT - 20.0))
            .flex()
            .items_center()
            .px(px(12.0))
            .child(
                div()
                    .mr(px(10.0))
                    .flex()
                    .items_center()
                    .text_color(palette.accent)
                    .text_size(px(14.0))
                    .child("❯"),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .text_size(px(13.5))
                    .child(if self.query.is_empty() {
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .text_color(palette.placeholder)
                                    .child("@ apps, # commands, : system..."),
                            )
                            .child(div().ml(px(2.0)).text_color(palette.accent).child("█"))
                    } else {
                        div()
                            .flex()
                            .items_center()
                            .child(div().text_color(palette.text).child(self.query.clone()))
                            .child(div().ml(px(2.0)).text_color(palette.accent).child("█"))
                    }),
            )
            .children(mode_tag.map(|(label, color)| {
                div()
                    .px(px(6.0))
                    .py(px(1.5))
                    .rounded(px(0.0))
                    .bg(palette.surface0)
                    .border_1()
                    .border_color(palette.surface1)
                    .text_color(color)
                    .text_size(px(10.5))
                    .child(format!("[ {label} ]"))
            }))
    }

    fn render_suggestions(&self, palette: Palette, cx: &mut Context<Self>) -> Option<Div> {
        if !self.config.suggestions.enabled() || self.filtered_actions.is_empty() {
            return None;
        }

        let selected_idx = self.selected_index;
        let items = self
            .filtered_actions
            .iter()
            .take(MAX_VISIBLE_ITEMS)
            .enumerate();

        Some(
            div()
                .w_full()
                .border_t_1()
                .border_color(palette.border_subtle)
                .flex()
                .flex_col()
                .children(items.map(|(idx, item)| {
                    let is_selected = idx == selected_idx;
                    let (badge, badge_color) = item.kind.badge(palette);
                    let action = item.clone();

                    div()
                        .h(px(ITEM_HEIGHT))
                        .px(px(12.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(12.0))
                        .bg(if is_selected {
                            palette.surface0
                        } else {
                            palette.bg
                        })
                        .hover(|style| style.bg(palette.surface0))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.activate(&action, &[], window, cx);
                            }),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .px(px(4.0))
                                        .py(px(1.0))
                                        .bg(palette.surface1)
                                        .border_1()
                                        .border_color(palette.border_subtle)
                                        .text_color(badge_color)
                                        .text_size(px(9.5))
                                        .child(badge),
                                )
                                .child(
                                    div()
                                        .text_color(if is_selected {
                                            palette.accent
                                        } else {
                                            palette.text
                                        })
                                        .child(item.key.clone()),
                                ),
                        )
                        .child(
                            div()
                                .text_color(palette.detail_text)
                                .text_size(px(11.0))
                                .child(item.detail.clone()),
                        )
                })),
        )
    }
}

/// Opens the launcher, retrying a previously failed config load on the way in.
fn open_search_window(
    cx: &mut App,
    active_window: &Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
    ps_error_tx: Sender<PsError>,
    store: &Rc<RefCell<ConfigStore>>,
) -> WindowHandle<SearchBar> {
    // Healthy opens are pure memory. Retry the load only when the cached one
    // failed, so a repaired config recovers on the next hotkey press.
    let needs_reload = store.borrow().has_error();
    if needs_reload {
        let _ = store.borrow_mut().reload();
    }

    let display = cx.displays().into_iter().next();
    let win_height = px(BAR_HEIGHT);
    let win_width = px(WIN_WIDTH);

    let (display_id, bounds) = if let Some(disp) = display {
        let screen_bounds = disp.bounds();
        let x = screen_bounds.origin.x + (screen_bounds.size.width - win_width) / 2.0;
        let y = screen_bounds.origin.y + (screen_bounds.size.height - win_height) * 0.22;
        let b = Bounds {
            origin: point(x, y),
            size: size(win_width, win_height),
        };
        (Some(disp.id()), b)
    } else {
        let b = Bounds {
            origin: point(px(100.0), px(100.0)),
            size: size(win_width, win_height),
        };
        (None, b)
    };

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id,
        titlebar: None,
        window_background: WindowBackgroundAppearance::Transparent,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        ..Default::default()
    };

    cx.activate(true);
    let active_ref = active_window.clone();
    let store_ref = store.clone();
    let handle = cx
        .open_window(options, move |window, cx| {
            cx.new(|cx| SearchBar::new(window, cx, active_ref, ps_error_tx, store_ref))
        })
        .unwrap();

    let diagnostic = store.borrow().diagnostic().map(str::to_string);
    if let Some(message) = diagnostic {
        let _ = handle.update(cx, |bar, window, cx| {
            bar.show_config_error(message, window, cx);
        });
    }

    handle
}

fn toggle_search_window(
    cx: &mut App,
    active_window: &Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
    ps_error_tx: &Sender<PsError>,
    store: &Rc<RefCell<ConfigStore>>,
) {
    let opened = active_window.borrow_mut().take();
    let was_open = opened.is_some_and(|handle| {
        handle
            .update(cx, |_, window, _| window.remove_window())
            .is_ok()
    });

    if was_open {
        schedule_trim(cx, active_window, Duration::from_millis(200));
    } else {
        let handle = open_search_window(cx, active_window, ps_error_tx.clone(), store);
        *active_window.borrow_mut() = Some(handle);
    }
}

/// Registers Alt+Backquote and forwards presses to the UI channel from a
/// dedicated thread. The manager must be kept alive for the process lifetime.
fn setup_global_hotkey(tx: Sender<()>) -> GlobalHotKeyManager {
    let manager = GlobalHotKeyManager::new().expect("Failed to initialize GlobalHotKeyManager");
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Backquote);
    let hotkey_id = hotkey.id();
    manager
        .register(hotkey)
        .expect("Failed to register Alt+Backquote global hotkey");

    let event_receiver = GlobalHotKeyEvent::receiver().clone();
    thread::spawn(move || {
        while let Ok(event) = event_receiver.recv() {
            if event.id == hotkey_id && event.state == HotKeyState::Pressed {
                let _ = tx.send_blocking(());
            }
        }
    });

    manager
}

fn main() {
    let app = Application::new();

    app.run(|cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                "../assets/fonts/GeistMonoNerdFontMono-Regular.otf"
            ))])
            .expect("Failed to register GeistMono Nerd Font");

        // The launcher is long-running (started at login), so the config is
        // read once here and every launcher open is served from memory.
        let config_store = Rc::new(RefCell::new(ConfigStore::load(
            config::Config::config_path(),
        )));

        // GPUI quits when its last window closes. Keep an invisible 1x1 window
        // open so the hotkey listener and async error delivery survive dismissal.
        let _anchor = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.0), px(0.0)),
                        size: size(px(1.0), px(1.0)),
                    })),
                    titlebar: None,
                    window_background: WindowBackgroundAppearance::Transparent,
                    show: false,
                    kind: WindowKind::PopUp,
                    ..Default::default()
                },
                |_, cx| cx.new(|_| AnchorWindow),
            )
            .expect("Failed to create anchor window");

        let (tx, rx): (Sender<()>, Receiver<()>) = async_channel::unbounded();
        let (ps_err_tx, ps_err_rx): (Sender<PsError>, Receiver<PsError>) =
            async_channel::unbounded();

        // Hotkey manager must stay alive for the lifetime of the application
        let hotkey_manager = setup_global_hotkey(tx);
        let active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>> =
            Rc::new(RefCell::new(None));

        // Reclaim startup pages once fonts, renderers and the anchor have settled.
        schedule_trim(&*cx, &active_window, Duration::from_secs(2));

        // Surface asynchronous PowerShell failures in the launcher itself. If
        // the bar was already dismissed, open a fresh window in the error state.
        let error_window = active_window.clone();
        let error_ps_tx = ps_err_tx.clone();
        let error_config_store = config_store.clone();
        cx.spawn(async move |cx| {
            while let Ok(err) = ps_err_rx.recv().await {
                let active = error_window.clone();
                let ps_error_tx = error_ps_tx.clone();
                let store = error_config_store.clone();
                let message = err.message;
                let _ = cx.update(move |cx| {
                    let handle = active.borrow().as_ref().copied();
                    let handle = if let Some(handle) = handle {
                        handle
                    } else {
                        let handle = open_search_window(cx, &active, ps_error_tx.clone(), &store);
                        *active.borrow_mut() = Some(handle);
                        handle
                    };

                    if handle
                        .update(cx, |bar, window, cx| {
                            bar.show_error(message.clone(), window, cx);
                        })
                        .is_err()
                    {
                        let handle = open_search_window(cx, &active, ps_error_tx, &store);
                        *active.borrow_mut() = Some(handle);
                        let _ = handle.update(cx, |bar, window, cx| {
                            bar.show_error(message, window, cx);
                        });
                    }
                });
            }
        })
        .detach();

        let active_window_clone = active_window.clone();
        let ps_err_tx_clone = ps_err_tx.clone();
        let hotkey_config_store = config_store.clone();
        cx.spawn(async move |cx| {
            // Keep hotkey_manager in the async task scope so it is never dropped
            let _manager = hotkey_manager;
            while let Ok(()) = rx.recv().await {
                let active = active_window_clone.clone();
                let ps_error_tx = ps_err_tx_clone.clone();
                let store = hotkey_config_store.clone();
                let _ = cx.update(move |cx| {
                    toggle_search_window(cx, &active, &ps_error_tx, &store);
                });
            }
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::{
        ActionKind, CandidateAction, CommandTarget, PsError, Scope, resolve_command, run_ps_task,
        split_scope,
    };
    use crate::commands::InternalCommand;
    use crate::config::{AppAction, Config, PsAction};
    use std::path::PathBuf;

    fn test_config() -> Config {
        let mut config = Config::default();
        config.apps.insert(
            "app".to_string(),
            AppAction {
                path: PathBuf::from("app.exe"),
                args: Vec::new(),
            },
        );
        config.ps.insert(
            "ps".to_string(),
            PsAction {
                script: "ps".to_string(),
                body: "{}".to_string(),
            },
        );
        config
    }

    #[test]
    fn test_resolve_command_accepts_known_targets_and_reload() {
        let config = test_config();
        assert_eq!(
            resolve_command(&config, "@app arg", None),
            Ok(CommandTarget::App {
                key: "app".to_string(),
                args: vec!["arg".to_string()],
            })
        );
        assert_eq!(
            resolve_command(&config, "#ps 1", None),
            Ok(CommandTarget::Ps {
                key: "ps".to_string(),
                args: vec!["1".to_string()],
            })
        );
        assert_eq!(
            resolve_command(&config, ":reload", None),
            Ok(CommandTarget::Internal {
                command: InternalCommand::Reload,
                args: Vec::new(),
            })
        );
        assert_eq!(
            resolve_command(&config, ": reload", None),
            Ok(CommandTarget::Internal {
                command: InternalCommand::Reload,
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn test_resolve_command_scopes_internal_commands() {
        let config = test_config();
        assert_eq!(
            resolve_command(&config, ":app", None),
            Err("unknown command 'app'".to_string())
        );
        assert_eq!(
            resolve_command(&config, ":ps", None),
            Err("unknown command 'ps'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "@reload", None),
            Err("unknown command 'reload'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "#reload", None),
            Err("unknown command 'reload'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "reload", None),
            Err("unknown command 'reload'".to_string())
        );
    }

    #[test]
    fn test_resolve_command_forwards_internal_arguments() {
        let config = test_config();
        assert_eq!(
            resolve_command(&config, ":reload extra", None),
            Ok(CommandTarget::Internal {
                command: InternalCommand::Reload,
                args: vec!["extra".to_string()],
            })
        );
    }

    #[test]
    fn test_resolve_command_rejects_unknown_and_misscoped_tokens() {
        let config = test_config();
        assert_eq!(
            resolve_command(&config, "unknown", None),
            Err("unknown command 'unknown'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "@ps", None),
            Err("unknown command 'ps'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "#app", None),
            Err("unknown command 'app'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "@", None),
            Err("expected command after prefix".to_string())
        );
    }

    #[test]
    fn test_resolve_command_falls_back_to_selected_candidate() {
        let config = test_config();
        let candidate = CandidateAction {
            key: "ps".to_string(),
            kind: ActionKind::Ps,
            detail: String::new(),
        };
        assert_eq!(
            resolve_command(&config, "p", Some(&candidate)),
            Ok(CommandTarget::Ps {
                key: "ps".to_string(),
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn test_resolve_command_falls_back_to_selected_internal_candidate() {
        let config = test_config();
        let candidate = CandidateAction {
            key: "reload".to_string(),
            kind: ActionKind::Internal,
            detail: String::new(),
        };
        assert_eq!(
            resolve_command(&config, ":re", Some(&candidate)),
            Ok(CommandTarget::Internal {
                command: InternalCommand::Reload,
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn test_split_scope_parses_combined_and_spaced_prefixes() {
        assert_eq!(split_scope("@app"), Some((Scope::App, "app")));
        assert_eq!(split_scope("@ app"), Some((Scope::App, " app")));
        assert_eq!(split_scope("#ps"), Some((Scope::Ps, "ps")));
        assert_eq!(split_scope("# ps"), Some((Scope::Ps, " ps")));
        assert_eq!(split_scope(":reload"), Some((Scope::Internal, "reload")));
        assert_eq!(split_scope(": reload"), Some((Scope::Internal, " reload")));
        assert_eq!(split_scope("  :reload"), Some((Scope::Internal, "reload")));
        assert_eq!(split_scope("plain"), None);
        assert_eq!(split_scope(""), None);
    }

    #[test]
    fn test_run_ps_task_reports_missing_argument_to_ui_channel() {
        let (error_tx, error_rx) = async_channel::unbounded::<PsError>();

        run_ps_task("brightness", "{ [int]$args[0] }", &[], &error_tx);

        let error = error_rx.try_recv().expect("error reported to UI");
        assert_eq!(error.message, "brightness: requires at least one argument");
    }
}
