use std::{cell::RefCell, rc::Rc, thread};
use async_channel::{Receiver, Sender};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use gpui::*;

mod theme;
use theme::Mocha as Theme;

const WIN_WIDTH: f32 = 660.0;
const BAR_HEIGHT: f32 = 62.0;

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

struct SearchBar {
    focus_handle: FocusHandle,
    query: String,
    was_active: bool,
    _subscription: Subscription,
    active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
}

impl SearchBar {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
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

        Self {
            focus_handle,
            query: String::new(),
            was_active: false,
            _subscription: subscription,
            active_window,
        }
    }

    fn execute(&self, window: &mut Window) {
        println!("Terminal command executed: {}", self.query.trim());
        *self.active_window.borrow_mut() = None;
        window.remove_window();
    }
}

impl Render for SearchBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (mode_label, mode_color) = if self.query.is_empty() {
            ("READY", Theme::subtext0())
        } else if self.query.starts_with('>') || self.query.starts_with('$') {
            ("↵ RUN", Theme::yellow())
        } else if self.query.starts_with('@') {
            ("↵ FILE", Theme::blue())
        } else if self.query.starts_with('?') {
            ("↵ WEB", Theme::sky())
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
                    "enter" => {
                        this.execute(window);
                    }
                    "backspace" => {
                        this.query.pop();
                        cx.notify();
                    }
                    _ => {
                        if let Some(ch) = &event.keystroke.key_char {
                            this.query.push_str(ch);
                            cx.notify();
                        } else if event.keystroke.key.chars().count() == 1 {
                            this.query.push_str(&event.keystroke.key);
                            cx.notify();
                        }
                    }
                }
            }))
            .size_full()
            .flex()
            .flex_col()
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
                    .flex_1()
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
                                            .child("type command, @file, or ?search..."),
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
    }
}



fn open_search_window(
    cx: &mut App,
    active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
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
    cx.open_window(options, |window, cx| {
        cx.new(|cx| SearchBar::new(window, cx, active_ref))
    })
    .unwrap()
}

fn toggle_search_window(
    cx: &mut App,
    active_window: &Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
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
        let handle = open_search_window(cx, active_window.clone());
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
        let (tx, rx): (Sender<()>, Receiver<()>) = async_channel::unbounded();

        // Hotkey manager must stay alive for the lifetime of the application
        let hotkey_manager = setup_global_hotkey(tx);
        let active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>> =
            Rc::new(RefCell::new(None));

        let active_window_clone = active_window.clone();
        cx.spawn(async move |cx| {
            // Keep hotkey_manager in the async task scope so it is never dropped
            let _manager = hotkey_manager;
            while let Ok(()) = rx.recv().await {
                let active = active_window_clone.clone();
                let _ = cx.update(move |cx| {
                    toggle_search_window(cx, &active);
                });
            }
        })
        .detach();
    });
}
