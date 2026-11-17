use std::{cell::RefCell, rc::Rc, sync::Arc, thread};
use async_channel::{Receiver, Sender};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use gpui::*;

mod config;
mod theme;
use theme::Mocha as Theme;

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
    pub script: String,
    pub args: Vec<String>,
}

fn spawn_powershell_worker(rx: Receiver<PsTask>) {
    thread::spawn(move || {
        while let Ok(task) = rx.recv_blocking() {
            execute_powershell_task(task);
        }
    });
}

fn execute_powershell_task(task: PsTask) {
    let mut script_buffer = String::from("$args = @(");
    for (idx, arg) in task.args.iter().enumerate() {
        if idx > 0x00 {
            script_buffer.push_str(", ");
        }
        script_buffer.push('\'');
        script_buffer.push_str(&arg.replace('\'', "''"));
        script_buffer.push('\'');
    }
    script_buffer.push_str(");\n");
    script_buffer.push_str(&task.script);

    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;

    let mut cmd = std::process::Command::new("pwsh");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script_buffer]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000);

    if cmd.status().is_err() {
        let mut fallback = std::process::Command::new("powershell");
        fallback.args(["-NoProfile", "-NonInteractive", "-Command", &script_buffer]);
        #[cfg(target_os = "windows")]
        fallback.creation_flags(0x0800_0000);
        let _ = fallback.status();
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
    selected_index: usize,
    filtered_actions: Vec<CandidateAction>,
}

impl SearchBar {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
        config: Arc<config::Config>,
        ps_sender: Sender<PsTask>,
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
            selected_index: 0,
            filtered_actions: Vec::new(),
        };
        instance.update_filtered_actions(window);
        instance
    }

    fn update_filtered_actions(&mut self, window: &mut Window) {
        let trimmed = self.query.trim();
        let raw_token = trimmed.split_whitespace().next().unwrap_or("");

        let (filter_scope, target_token) = if let Some(stripped) = raw_token.strip_prefix('@') {
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
                        script: ps.script.clone(),
                        args: dynamic_args.to_vec(),
                    };
                    let _ = self.ps_sender.send_blocking(task);
                }
            }
        }

        *self.active_window.borrow_mut() = None;
        window.remove_window();
    }

    fn execute(&self, window: &mut Window) {
        let trimmed = self.query.trim();
        if !trimmed.is_empty() {
            let tokens: Vec<&str> = trimmed.split_whitespace().collect();
            let raw_key = tokens[0x00];
            let clean_key = raw_key
                .strip_prefix('@')
                .or_else(|| raw_key.strip_prefix('#'))
                .unwrap_or(raw_key);

            let dynamic_args: Vec<String> = tokens[0x01..]
                .iter()
                .map(|s| s.to_string())
                .collect();

            if let Some(app) = self.config.apps.get(clean_key) {
                let candidate = CandidateAction {
                    key: clean_key.to_string(),
                    kind: ActionKind::App,
                    detail: app.path.display().to_string(),
                };
                self.execute_action(&candidate, &dynamic_args, window);
                return;
            } else if let Some(ps) = self.config.ps.get(clean_key) {
                let candidate = CandidateAction {
                    key: clean_key.to_string(),
                    kind: ActionKind::Ps,
                    detail: ps.script.clone(),
                };
                self.execute_action(&candidate, &dynamic_args, window);
                return;
            } else if let Some(selected) = self.filtered_actions.get(self.selected_index) {
                self.execute_action(selected, &dynamic_args, window);
                return;
            } else {
                println!("Terminal command executed: {}", trimmed);
            }
        } else if let Some(selected) = self.filtered_actions.get(self.selected_index) {
            self.execute_action(selected, &[], window);
            return;
        }

        *self.active_window.borrow_mut() = None;
        window.remove_window();
    }
}

impl Render for SearchBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let trimmed = self.query.trim();
        let first_token = trimmed.split_whitespace().next().unwrap_or("");

        let (mode_label, mode_color) = if self.query.is_empty() {
            ("READY", Theme::subtext0())
        } else if self.query.starts_with('@') {
            ("@ APPS", Theme::green())
        } else if self.query.starts_with('#') {
            ("# PS", Theme::peach())
        } else if self.config.apps.contains_key(first_token) {
            ("↵ APP", Theme::green())
        } else if self.config.ps.contains_key(first_token) {
            ("↵ PS", Theme::peach())
        } else {
            ("↵ EXEC", Theme::green())
        };

        div()
            .track_focus(&self.focus_handle)
            .key_context("terminal_bar")
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
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
                            this.query = format!("{} ", candidate.key);
                            this.update_filtered_actions(window);
                            cx.notify();
                        }
                    }
                    "enter" => {
                        this.execute(window);
                    }
                    "backspace" => {
                        this.query.pop();
                        this.update_filtered_actions(window);
                        cx.notify();
                    }
                    _ => {
                        if let Some(ch) = &event.keystroke.key_char {
                            this.query.push_str(ch);
                            this.update_filtered_actions(window);
                            cx.notify();
                        } else if event.keystroke.key.chars().count() == 1 {
                            this.query.push_str(&event.keystroke.key);
                            this.update_filtered_actions(window);
                            cx.notify();
                        }
                    }
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .font_family("GeistMono Nerd Font Mono")
            .bg(Theme::terminal_bg())
            .rounded(px(0.0))
            .border_1()
            .border_color(Theme::terminal_frame())
            .overflow_hidden()
            // Top ASCII TUI header bracket
            .child(
                div()
                    .h(px(20.0))
                    .px(px(10.0))
                    .bg(Theme::terminal_header())
                    .border_b_1()
                    .border_color(Theme::border_subtle())
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
                                    .text_color(Theme::terminal_bracket())
                                    .child("┌─["),
                            )
                            .child(
                                div()
                                    .text_color(Theme::green())
                                    .child("shyena::cli"),
                            )
                            .child(
                                div()
                                    .text_color(Theme::terminal_bracket())
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
                                    .text_color(Theme::subtext0())
                                    .child("[ ESC: exit ]"),
                            )
                            .child(
                                div()
                                    .text_color(Theme::terminal_bracket())
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
                            .text_color(Theme::green())
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
                                            .text_color(Theme::overlay0())
                                            .child("type command, @apps, or #ps..."),
                                    )
                                    .child(
                                        div()
                                            .ml(px(2.0))
                                            .text_color(Theme::green())
                                            .child("█"),
                                    )
                            } else {
                                div()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_color(Theme::text())
                                            .child(self.query.clone()),
                                    )
                                    .child(
                                        div()
                                            .ml(px(2.0))
                                            .text_color(Theme::green())
                                            .child("█"),
                                    )
                            }),
                    )
                    // Sharp bracketed action tag
                    .child(
                        div()
                            .px(px(6.0))
                            .py(px(1.5))
                            .rounded(px(0.0))
                            .bg(Theme::surface0())
                            .border_1()
                            .border_color(Theme::surface1())
                            .text_color(mode_color)
                            .text_size(px(10.5))
                            .child(format!("[ {} ]", mode_label)),
                    ),
            )
            .children(if !self.filtered_actions.is_empty() {
                let selected_idx = self.selected_index;
                let visible_items = self.filtered_actions.iter().take(MAX_VISIBLE_ITEMS).enumerate();

                Some(
                    div()
                        .w_full()
                        .border_t_1()
                        .border_color(Theme::border_subtle())
                        .flex()
                        .flex_col()
                        .children(visible_items.map(|(idx, item)| {
                            let is_selected = idx == selected_idx;
                            let (badge_text, badge_color) = match item.kind {
                                ActionKind::App => ("APP", Theme::green()),
                                ActionKind::Ps => ("PS", Theme::peach()),
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
                                    Theme::surface0()
                                } else {
                                    Theme::terminal_bg()
                                })
                                .hover(|style| style.bg(Theme::surface0()))
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
                                                .bg(Theme::surface1())
                                                .border_1()
                                                .border_color(Theme::border_subtle())
                                                .text_color(badge_color)
                                                .text_size(px(9.5))
                                                .child(badge_text),
                                        )
                                        .child(
                                            div()
                                                .text_color(if is_selected {
                                                    Theme::green()
                                                } else {
                                                    Theme::text()
                                                })
                                                .child(item.key.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_color(Theme::subtext0())
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
    let config = Arc::new(config::Config::load());
    cx.open_window(options, |window, cx| {
        cx.new(|cx| SearchBar::new(window, cx, active_ref, config, ps_sender))
    })
    .unwrap()
}

fn toggle_search_window(
    cx: &mut App,
    active_window: &Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
    ps_sender: &Sender<PsTask>,
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
        let handle = open_search_window(cx, active_window.clone(), ps_sender.clone());
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
                std::borrow::Cow::Borrowed(include_bytes!(
                    "../assets/fonts/GeistMonoNerdFontMono-Italic.otf"
                )),
            ])
            .expect("Failed to register GeistMono Nerd Font");

        let (tx, rx): (Sender<()>, Receiver<()>) = async_channel::unbounded();
        let (ps_tx, ps_rx): (Sender<PsTask>, Receiver<PsTask>) = async_channel::unbounded();
        spawn_powershell_worker(ps_rx);

        // Hotkey manager must stay alive for the lifetime of the application
        let hotkey_manager = setup_global_hotkey(tx);
        let active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>> =
            Rc::new(RefCell::new(None));

        let active_window_clone = active_window.clone();
        let ps_tx_clone = ps_tx.clone();
        cx.spawn(async move |cx| {
            // Keep hotkey_manager in the async task scope so it is never dropped
            let _manager = hotkey_manager;
            while let Ok(()) = rx.recv().await {
                let active = active_window_clone.clone();
                let ps_sender = ps_tx_clone.clone();
                let _ = cx.update(move |cx| {
                    toggle_search_window(cx, &active, &ps_sender);
                });
            }
        })
        .detach();
    });
}
