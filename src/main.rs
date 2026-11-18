use std::{cell::RefCell, rc::Rc, sync::Arc, thread};
use async_channel::{Receiver, Sender};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use gpui::*;

mod config;
mod exec;
mod theme;
use config::{ReloadTrigger, RESERVED_RELOAD_CMD};
use theme::Theme;

const WIN_WIDTH: f32 = 660.0;
const BAR_HEIGHT: f32 = 62.0;
const ITEM_HEIGHT: f32 = 32.0;
const MAX_VISIBLE_ITEMS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    App,
    Ps,
}

#[derive(Clone, Debug)]
pub struct CandidateAction {
    pub key: String,
    pub kind: ActionKind,
    pub detail: String,
}

#[cfg(target_os = "windows")]
mod platform {
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_DONOTROUND,
    };
    use windows::Win32::UI::Controls::MARGINS;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetForegroundWindow, SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    pub fn setup_window(window: &Window) {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };

        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };

        let hwnd = HWND(win32.hwnd.get() as *mut std::ffi::c_void);

        // Extend 1px frame into client area to activate Windows DWM hardware drop shadow
        let margins = MARGINS {
            cxLeftWidth: 1,
            cxRightWidth: 1,
            cyTopHeight: 1,
            cyBottomHeight: 1,
        };
        let _ = unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) };

        // Authentic sharp TUI window geometry (0px corner radius)
        let corner_pref = DWMWCP_DONOTROUND;
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_pref as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&corner_pref) as u32,
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
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use gpui::Window;

    pub fn setup_window(_window: &Window) {}

    pub fn make_always_on_top(_window: &Window) -> bool {
        false
    }
}

#[derive(Clone, Debug)]
pub struct PsTask {
    pub function_name: String,
    pub function_body: String,
    pub dynamic_tokens: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PsError {
    pub message: String,
}

/// Runtime failures are dismissible with [Esc]; config failures are fatal for
/// the current launcher session and close the window on [Esc] instead.
#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum CommandTarget {
    App { key: String, args: Vec<String> },
    Ps { key: String, args: Vec<String> },
    Reload,
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

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let first = tokens[0];
    let (scope, key, rest): (Option<&str>, &str, &[&str]) = if first == "@" || first == "#" {
        if tokens.len() < 0x02 {
            return selected
                .map(|candidate| candidate_target(candidate, Vec::new()))
                .ok_or_else(|| "expected command after prefix".to_string());
        }
        (Some(first), tokens[0x01], &tokens[0x02..])
    } else if let Some(stripped) = first.strip_prefix('@') {
        (Some("@"), stripped, &tokens[0x01..])
    } else if let Some(stripped) = first.strip_prefix('#') {
        (Some("#"), stripped, &tokens[0x01..])
    } else {
        (None, first, &tokens[0x01..])
    };

    if key.is_empty() {
        return Err("expected command name".to_string());
    }

    if key.eq_ignore_ascii_case(RESERVED_RELOAD_CMD) && scope != Some("@") {
        return Ok(CommandTarget::Reload);
    }

    let args = rest.iter().map(|token| token.to_string()).collect::<Vec<_>>();

    if scope != Some("#") && config.apps.contains_key(key) {
        return Ok(CommandTarget::App {
            key: key.to_string(),
            args,
        });
    }

    if scope != Some("@") {
        if config.ps.contains_key(key) {
            return Ok(CommandTarget::Ps {
                key: key.to_string(),
                args,
            });
        }
    }

    if let Some(candidate) = selected {
        return Ok(candidate_target(candidate, args));
    }

    Err(format!("unknown command '{key}'"))
}

fn spawn_powershell_worker(rx: Receiver<PsTask>, error_tx: Sender<PsError>) {
    thread::spawn(move || {
        while let Ok(task) = rx.recv_blocking() {
            execute_powershell_task(task, &error_tx);
        }
    });
}

fn execute_powershell_task(task: PsTask, error_tx: &Sender<PsError>) {
    let args: Vec<&str> = task.dynamic_tokens.iter().map(|s| s.as_str()).collect();
    if let Err(err) = exec::dispatch_ps_function(&task.function_name, &task.function_body, &args) {
        let _ = error_tx.send_blocking(PsError {
            message: exec::format_ps_error(&task.function_name, &err),
        });
    }
}

/// Hidden window that keeps GPUI's run loop (and the PowerShell worker) alive
/// after the launcher window is dismissed.
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
    ps_sender: Sender<PsTask>,
    reload_trigger: ReloadTrigger,
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
        config: Arc<config::Config>,
        ps_sender: Sender<PsTask>,
        reload_trigger: ReloadTrigger,
        theme: Theme,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window);

        platform::setup_window(window);
        let _ = platform::make_always_on_top(window);

        let subscription = cx.observe_window_activation(window, |this, window, cx| {
            let active = window.is_window_active();
            if active {
                this.was_active = true;
            } else if this.was_active {
                *this.active_window.borrow_mut() = None;
                window.remove_window();
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
            ps_sender,
            reload_trigger,
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
            let target_height = px(BAR_HEIGHT);
            let current_size = window.bounds().size;
            let diff: f32 = (current_size.height - target_height).into();
            if diff.abs() > 0.5 {
                window.resize(size(current_size.width, target_height));
            }
            return;
        }

        let mut tokens_iter = trimmed.split_whitespace();
        let raw_token = tokens_iter.next().unwrap_or("");

        let (filter_scope, target_token) = if raw_token == "@" {
            (Some(ActionKind::App), tokens_iter.next().unwrap_or(""))
        } else if raw_token == "#" {
            (Some(ActionKind::Ps), tokens_iter.next().unwrap_or(""))
        } else if let Some(stripped) = raw_token.strip_prefix('@') {
            (Some(ActionKind::App), stripped)
        } else if let Some(stripped) = raw_token.strip_prefix('#') {
            (Some(ActionKind::Ps), stripped)
        } else {
            (None, raw_token)
        };

        let mut candidates = Vec::new();
        if filter_scope.is_none() || filter_scope == Some(ActionKind::App) {
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

        if filter_scope.is_none() || filter_scope == Some(ActionKind::Ps) {
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

        candidates.sort_by(|a, b| {
            let a_exact = a.key.eq_ignore_ascii_case(target_token);
            let b_exact = b.key.eq_ignore_ascii_case(target_token);
            let a_prefix = a.key.to_lowercase().starts_with(&target_token.to_lowercase());
            let b_prefix = b.key.to_lowercase().starts_with(&target_token.to_lowercase());

            b_exact.cmp(&a_exact)
                .then_with(|| b_prefix.cmp(&a_prefix))
                .then_with(|| a.key.cmp(&b.key))
        });

        self.filtered_actions = candidates;
        if self.selected_index >= self.filtered_actions.len() {
            self.selected_index = 0;
        }

        let visible_items = self.filtered_actions.len().min(MAX_VISIBLE_ITEMS);
        let list_height = if visible_items > 0 {
            (visible_items as f32) * ITEM_HEIGHT + 1.0
        } else {
            0.0
        };
        let target_height = px(BAR_HEIGHT + list_height);
        let current_size = window.bounds().size;
        let diff: f32 = (current_size.height - target_height).into();
        if diff.abs() > 0.5 {
            window.resize(size(current_size.width, target_height));
        }
    }

    fn resize_to_bar(&self, window: &mut Window) {
        let current_size = window.bounds().size;
        let target_height = px(BAR_HEIGHT);
        let diff: f32 = (current_size.height - target_height).into();
        if diff.abs() > 0.5 {
            window.resize(size(current_size.width, target_height));
        }
    }

    fn set_error(
        &mut self,
        error: ErrorState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = Some(error);
        self.query.clear();
        self.filtered_actions.clear();
        self.selected_index = 0;
        self.resize_to_bar(window);
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

    /// A successful hot reload replaces the snapshot and clears a config error.
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

    fn handle_key(
        this: &mut Self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(error) = &this.error {
            if event.keystroke.key == "escape" {
                if error.is_config() {
                    *this.active_window.borrow_mut() = None;
                    window.remove_window();
                } else {
                    this.dismiss_error(window, cx);
                }
            }
            return;
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                *this.active_window.borrow_mut() = None;
                window.remove_window();
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
                    // Keep the active @/# scope so dynamic args still resolve
                    // to the same action when Enter is pressed.
                    let scope = this
                        .query
                        .trim_start()
                        .chars()
                        .next()
                        .filter(|c| *c == '@' || *c == '#')
                        .map(|c| c.to_string())
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

    fn execute_action(&self, action: &CandidateAction, dynamic_args: &[String], window: &mut Window) {
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
                    let task = PsTask {
                        function_name: action.key.clone(),
                        function_body: ps.body.clone(),
                        dynamic_tokens: dynamic_args.to_vec(),
                    };
                    let _ = self.ps_sender.send_blocking(task);
                }
            }
        }

        *self.active_window.borrow_mut() = None;
        window.remove_window();
    }

    fn execute(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.query.trim().is_empty() && self.filtered_actions.is_empty() {
            *self.active_window.borrow_mut() = None;
            window.remove_window();
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
                self.execute_action(&candidate, &args, window);
            }
            CommandTarget::Ps { key, args } => {
                let ps = self.config.ps.get(&key).expect("resolved function exists");
                let candidate = CandidateAction {
                    key,
                    kind: ActionKind::Ps,
                    detail: ps.script.clone(),
                };
                self.execute_action(&candidate, &args, window);
            }
            CommandTarget::Reload => {
                self.reload_trigger.trigger_now();
                *self.active_window.borrow_mut() = None;
                window.remove_window();
            }
        }
    }
}

impl Render for SearchBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let trimmed = self.query.trim();
        let first_token = trimmed.split_whitespace().next().unwrap_or("");

        let mode_tag = if trimmed.is_empty() {
            None
        } else if trimmed.starts_with('@') {
            Some(("LAUNCH", theme.accent()))
        } else if trimmed.starts_with('#') {
            Some(("EXEC", theme.peach()))
        } else if let Some(selected) = self.filtered_actions.get(self.selected_index) {
            match selected.kind {
                ActionKind::App => Some(("LAUNCH", theme.accent())),
                ActionKind::Ps => Some(("EXEC", theme.peach())),
            }
        } else if self.config.apps.contains_key(first_token) {
            Some(("LAUNCH", theme.accent()))
        } else {
            Some(("EXEC", theme.peach()))
        };

        let root = div()
            .track_focus(&self.focus_handle)
            .key_context("terminal_bar")
            .on_key_down(cx.listener(|this, event, window, cx| {
                Self::handle_key(this, event, window, cx)
            }))
            .size_full()
            .flex()
            .flex_col()
            .font_family("GeistMono Nerd Font Mono")
            .bg(theme.terminal_bg())
            .rounded(px(0.0))
            .border_1()
            .border_color(if self.error.is_some() {
                theme.red()
            } else {
                theme.terminal_frame()
            })
            .overflow_hidden();

        // Error state replaces the search bar entirely; [Esc] returns to an
        // empty bar in the same window.
        if let Some(error) = &self.error {
            let message = error.message().to_string();
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

            return root
                .child(
                    div()
                        .h(px(20.0))
                        .px(px(10.0))
                        .bg(theme.terminal_header())
                        .border_b_1()
                        .border_color(theme.red())
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(10.5))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .child(div().text_color(theme.red()).child("┌─["))
                                .child(div().text_color(theme.red()).child(label.to_string()))
                                .child(div().text_color(theme.red()).child("]")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .text_color(theme.subtext0())
                                        .child(esc_hint.to_string()),
                                )
                                .child(div().text_color(theme.red()).child("─┐")),
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
                                .text_color(theme.red())
                                .text_size(px(14.0))
                                .child("✗"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_color(theme.text())
                                .text_size(px(12.5))
                                .child(message),
                        ),
                );
        }

        root
            // Top ASCII TUI header bracket
            .child(
                div()
                    .h(px(20.0))
                    .px(px(10.0))
                    .bg(theme.terminal_header())
                    .border_b_1()
                    .border_color(theme.border_subtle())
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(10.5))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .text_color(theme.terminal_bracket())
                                    .child("┌─["),
                            )
                            .child(
                                div()
                                    .text_color(theme.accent())
                                    .child("shyena::cli"),
                            )
                            .child(
                                div()
                                    .text_color(theme.terminal_bracket())
                                    .child("]"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .text_color(theme.subtext0())
                                    .child("[ ESC: exit ]"),
                            )
                            .child(
                                div()
                                    .text_color(theme.terminal_bracket())
                                    .child("─┐"),
                            ),
                    ),
            )
            // Input stream with leading prompt and solid terminal block cursor
            .child(
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
                            .text_color(theme.accent())
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
                                            .text_color(theme.overlay0())
                                            .child("@ for apps, # for commands..."),
                                    )
                                    .child(
                                        div()
                                            .ml(px(2.0))
                                            .text_color(theme.accent())
                                            .child("█"),
                                    )
                            } else {
                                div()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_color(theme.text())
                                            .child(self.query.clone()),
                                    )
                                    .child(
                                        div()
                                            .ml(px(2.0))
                                            .text_color(theme.accent())
                                            .child("█"),
                                    )
                            }),
                    )
                    // Sharp bracketed action tag
                    .children(mode_tag.map(|(label, color)| {
                        div()
                            .px(px(6.0))
                            .py(px(1.5))
                            .rounded(px(0.0))
                            .bg(theme.surface0())
                            .border_1()
                            .border_color(theme.surface1())
                            .text_color(color)
                            .text_size(px(10.5))
                            .child(format!("[ {} ]", label))
                    })),
            )
            .children(if !self.filtered_actions.is_empty() {
                let selected_idx = self.selected_index;
                let visible_items = self.filtered_actions.iter().take(MAX_VISIBLE_ITEMS).enumerate();

                Some(
                    div()
                        .w_full()
                        .border_t_1()
                        .border_color(theme.border_subtle())
                        .flex()
                        .flex_col()
                        .children(visible_items.map(|(idx, item)| {
                            let is_selected = idx == selected_idx;
                            let (badge_text, badge_color) = match item.kind {
                                ActionKind::App => ("APP", theme.accent()),
                                ActionKind::Ps => ("PS", theme.peach()),
                            };
                            let action_item = item.clone();

                            div()
                                .h(px(ITEM_HEIGHT))
                                .px(px(12.0))
                                .flex()
                                .items_center()
                                .justify_between()
                                .text_size(px(12.0))
                                .bg(if is_selected {
                                    theme.surface0()
                                } else {
                                    theme.terminal_bg()
                                })
                                .hover(|style| style.bg(theme.surface0()))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, window, _cx| {
                                        this.execute_action(&action_item, &[], window);
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
                                                .bg(theme.surface1())
                                                .border_1()
                                                .border_color(theme.border_subtle())
                                                .text_color(badge_color)
                                                .text_size(px(9.5))
                                                .child(badge_text),
                                        )
                                        .child(
                                            div()
                                                .text_color(if is_selected {
                                                    theme.accent()
                                                } else {
                                                    theme.text()
                                                })
                                                .child(item.key.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_color(theme.detail_text())
                                        .text_size(px(11.0))
                                        .child(item.detail.clone()),
                                )
                        })),
                )
            } else {
                None
            })
    }
}



fn open_search_window(
    cx: &mut App,
    active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
    ps_sender: Sender<PsTask>,
    reload_trigger: ReloadTrigger,
) -> WindowHandle<SearchBar> {
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
    let reload_ref = reload_trigger.clone();
    let (cfg, diagnostic) = config::Config::load_with_diagnostics();
    let config = Arc::new(cfg);
    let theme = config.theme;
    let handle = cx
        .open_window(options, |window, cx| {
            cx.new(|cx| {
                SearchBar::new(
                    window,
                    cx,
                    active_ref,
                    config,
                    ps_sender,
                    reload_ref,
                    theme,
                )
            })
        })
        .unwrap();

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
    ps_sender: &Sender<PsTask>,
    reload_trigger: ReloadTrigger,
) {
    let mut win_ref = active_window.borrow_mut();
    let mut was_open = false;

    if let Some(handle) = win_ref.as_ref() {
        if handle.update(cx, |_, window, _| {
            window.remove_window();
        }).is_ok() {
            was_open = true;
        }
    }

    *win_ref = None;

    if !was_open {
        drop(win_ref);
        let handle = open_search_window(
            cx,
            active_window.clone(),
            ps_sender.clone(),
            reload_trigger,
        );
        *active_window.borrow_mut() = Some(handle);
    }
}

fn setup_global_hotkey(tx: Sender<()>) -> GlobalHotKeyManager {
    let manager = GlobalHotKeyManager::new().expect("Failed to initialize GlobalHotKeyManager");
    // Register Alt + Backquote (tilde key)
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Backquote);
    let hotkey_id = hotkey.id();
    manager.register(hotkey).expect("Failed to register Alt+Backquote global hotkey");

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
            .add_fonts(vec![
                std::borrow::Cow::Borrowed(include_bytes!(
                    "../assets/fonts/GeistMonoNerdFontMono-Regular.otf"
                )),
            ])
            .expect("Failed to register GeistMono Nerd Font");

        // GPUI quits when its last window closes. Keep an invisible 1x1 window
        // open so the background PowerShell worker survives launcher dismissal.
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
        let (ps_tx, ps_rx): (Sender<PsTask>, Receiver<PsTask>) = async_channel::unbounded();
        let (ps_err_tx, ps_err_rx): (Sender<PsError>, Receiver<PsError>) =
            async_channel::unbounded();
        spawn_powershell_worker(ps_rx, ps_err_tx);

        // The watcher monitors the parent directory, so make sure it exists
        // before the Win32 listener starts (a first launch may not have it yet).
        let config_path = config::Config::config_path();
        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let (reload_trigger, reload_rx) =
            config::spawn_config_watcher(config_path).expect("Failed to start config watcher");

        // Hotkey manager must stay alive for the lifetime of the application
        let hotkey_manager = setup_global_hotkey(tx);
        let active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>> =
            Rc::new(RefCell::new(None));

        // Surface asynchronous PowerShell failures in the launcher itself. If
        // the bar was already dismissed, open a fresh window in the error state.
        let error_window = active_window.clone();
        let error_ps_sender = ps_tx.clone();
        let error_reload_trigger = reload_trigger.clone();
        cx.spawn(async move |cx| {
            while let Ok(err) = ps_err_rx.recv().await {
                let active = error_window.clone();
                let ps_sender = error_ps_sender.clone();
                let trigger = error_reload_trigger.clone();
                let message = err.message;
                let _ = cx.update(move |cx| {
                    let handle = active.borrow().as_ref().cloned();
                    let handle = match handle {
                        Some(handle) => handle,
                        None => {
                            let handle = open_search_window(
                                cx,
                                active.clone(),
                                ps_sender.clone(),
                                trigger.clone(),
                            );
                            *active.borrow_mut() = Some(handle.clone());
                            handle
                        }
                    };

                    if handle
                        .update(cx, |bar, window, cx| {
                            bar.show_error(message.clone(), window, cx);
                        })
                        .is_err()
                    {
                        let handle =
                            open_search_window(cx, active.clone(), ps_sender, trigger);
                        *active.borrow_mut() = Some(handle.clone());
                        let _ = handle.update(cx, |bar, window, cx| {
                            bar.show_error(message, window, cx);
                        });
                    }
                });
            }
        })
        .detach();

        // Live config changes: valid snapshots refresh the open bar, while any
        // I/O or parse failure becomes a fatal config error panel.
        let reload_event_window = active_window.clone();
        let reload_event_ps_sender = ps_tx.clone();
        let reload_event_trigger = reload_trigger.clone();
        let reload_keepalive = reload_trigger.clone();
        cx.spawn(async move |cx| {
            let _trigger_keepalive = reload_keepalive;
            while let Ok(event) = reload_rx.recv().await {
                let active = reload_event_window.clone();
                let ps_sender = reload_event_ps_sender.clone();
                let trigger = reload_event_trigger.clone();
                let _ = cx.update(move |cx| match event {
                    config::ReloadEvent::Updated(snapshot) => {
                        let cfg = Arc::new(config::Config::from(snapshot));
                        let theme = cfg.theme;
                        if let Some(handle) = active.borrow().as_ref().cloned() {
                            if handle
                                .update(cx, |bar, window, cx| {
                                    bar.apply_config(cfg, theme, window, cx);
                                })
                                .is_err()
                            {
                                *active.borrow_mut() = None;
                            }
                        }
                    }
                    config::ReloadEvent::Error(err) => {
                        let message = match err {
                            config::ReloadError::Parse(parse_err) => parse_err.to_diagnostic(),
                            config::ReloadError::Io(io_err) => format!(
                                "~/.config/.shyena:1:1: error: failed to read config: {io_err}"
                            ),
                        };
                        let handle = active.borrow().as_ref().cloned();
                        let handle = match handle {
                            Some(handle) => handle,
                            None => {
                                let handle = open_search_window(
                                    cx,
                                    active.clone(),
                                    ps_sender.clone(),
                                    trigger.clone(),
                                );
                                *active.borrow_mut() = Some(handle.clone());
                                handle
                            }
                        };

                        if handle
                            .update(cx, |bar, window, cx| {
                                bar.show_config_error(message.clone(), window, cx);
                            })
                            .is_err()
                        {
                            let handle =
                                open_search_window(cx, active.clone(), ps_sender, trigger);
                            *active.borrow_mut() = Some(handle.clone());
                            let _ = handle.update(cx, |bar, window, cx| {
                                bar.show_config_error(message, window, cx);
                            });
                        }
                    }
                });
            }
        })
        .detach();

        let active_window_clone = active_window.clone();
        let ps_tx_clone = ps_tx.clone();
        let hotkey_reload_trigger = reload_trigger.clone();
        cx.spawn(async move |cx| {
            // Keep hotkey_manager in the async task scope so it is never dropped
            let _manager = hotkey_manager;
            while let Ok(()) = rx.recv().await {
                let active = active_window_clone.clone();
                let ps_sender = ps_tx_clone.clone();
                let trigger = hotkey_reload_trigger.clone();
                let _ = cx.update(move |cx| {
                    toggle_search_window(cx, &active, &ps_sender, trigger);
                });
            }
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::{
        ActionKind, CandidateAction, CommandTarget, PsError, PsTask, resolve_command,
        spawn_powershell_worker,
    };
    use crate::config::{AppAction, Config, PsAction};
    use async_channel::{Receiver, Sender};
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
            resolve_command(&config, "#config-reload", None),
            Ok(CommandTarget::Reload)
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
    fn test_worker_reports_missing_argument_to_ui_channel() {
        let (task_tx, task_rx): (Sender<PsTask>, Receiver<PsTask>) = async_channel::unbounded();
        let (error_tx, error_rx): (Sender<PsError>, Receiver<PsError>) = async_channel::unbounded();
        spawn_powershell_worker(task_rx, error_tx);

        task_tx
            .send_blocking(PsTask {
                function_name: "brightness".to_string(),
                function_body: "{ [int]$args[0] }".to_string(),
                dynamic_tokens: Vec::new(),
            })
            .expect("task accepted");

        let error = error_rx.recv_blocking().expect("error reported to UI");
        assert_eq!(error.message, "brightness: requires at least one argument");
    }
}
