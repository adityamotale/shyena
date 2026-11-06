use std::{cell::RefCell, rc::Rc, thread};
use async_channel::{Receiver, Sender};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use gpui::*;

const WIN_WIDTH: f32 = 600.0;
const BAR_HEIGHT: f32 = 52.0;
const EXPANDED_HEIGHT: f32 = 320.0;

#[cfg(target_os = "windows")]
mod platform {
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW,
        DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };
    use windows::Win32::UI::Controls::MARGINS;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetForegroundWindow, SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    #[repr(C)]
    struct AccentPolicy {
        accent_state: u32,
        accent_flags: u32,
        gradient_color: u32,
        animation_id: u32,
    }

    #[repr(C)]
    struct WindowCompositionAttribData {
        attrib: u32,
        pv_data: *mut std::ffi::c_void,
        cb_data: usize,
    }

    pub fn apply_acrylic(window: &Window) -> bool {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return false;
        };

        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return false;
        };

        let hwnd = HWND(win32.hwnd.get() as *mut std::ffi::c_void);

        // Extend DWM frame into client area
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) };

        // Windows 11 rounded window corners
        let corner_pref = DWMWCP_ROUND;
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_pref as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&corner_pref) as u32,
            )
        };

        // Windows 11 Acrylic: DWMWA_SYSTEMBACKDROP_TYPE (0x26 / 38) = DWMSBT_TRANSIENTWINDOW (0x03)
        let backdrop_type = DWMSBT_TRANSIENTWINDOW;
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &backdrop_type as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&backdrop_type) as u32,
            )
        };

        // Always apply composition attribute acrylic blur (same method used by Windows Terminal)
        apply_win10_acrylic_fallback(hwnd)
    }

    fn apply_win10_acrylic_fallback(hwnd: HWND) -> bool {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetModuleHandleA(lp_module_name: *const u8) -> *mut std::ffi::c_void;
            fn GetProcAddress(
                h_module: *mut std::ffi::c_void,
                lp_proc_name: *const u8,
            ) -> *mut std::ffi::c_void;
        }

        unsafe {
            let user32 = GetModuleHandleA(b"user32.dll\0".as_ptr());
            if user32.is_null() {
                return false;
            }

            let func = GetProcAddress(user32, b"SetWindowCompositionAttribute\0".as_ptr());
            if func.is_null() {
                return false;
            }

            type SetAttrFn = unsafe extern "system" fn(
                HWND,
                *mut WindowCompositionAttribData,
            ) -> i32;
            let set_attr: SetAttrFn = std::mem::transmute(func);
            let accent = AccentPolicy {
                accent_state: 0x04,
                accent_flags: 0x02,
                gradient_color: 0x18f5f1ef,
                animation_id: 0x00,
            };
            let mut data = WindowCompositionAttribData {
                attrib: 0x13,
                pv_data: &accent as *const _ as *mut _,
                cb_data: std::mem::size_of::<AccentPolicy>(),
            };
            set_attr(hwnd, &mut data) != 0
        }
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

    pub fn apply_acrylic(_window: &Window) -> bool {
        false
    }

    pub fn make_always_on_top(_window: &Window) -> bool {
        false
    }
}

// Catppuccin Latte palette helpers
struct Latte;
#[allow(dead_code)]
impl Latte {
    // Acrylic translucent Base background (~10% alpha to let blur and grain dominate)
    fn base_acrylic() -> Rgba {
        rgba(0xeff1f518)
    }
    // Acrylic translucent Mantle for header (~70% alpha)
    fn mantle_acrylic() -> Rgba {
        rgba(0xe6e9efb2)
    }
    // Acrylic translucent Crust for footer
    fn crust_acrylic() -> Rgba {
        rgba(0xdce0e8b2)
    }
    // Surface layers for borders and active items
    fn surface0() -> Rgba {
        rgb(0xccd0da)
    }
    fn border_glass() -> Rgba {
        rgba(0xccd0da80)
    }
    fn surface1() -> Rgba {
        rgba(0xbcc0ccd8)
    }
    // Typography
    fn text() -> Rgba {
        rgb(0x4c4f69)
    }
    fn subtext0() -> Rgba {
        rgb(0x6c6f85)
    }
    fn subtext1() -> Rgba {
        rgb(0x5c5f77)
    }
    fn overlay0() -> Rgba {
        rgb(0x9ca0b0)
    }
    fn overlay2() -> Rgba {
        rgb(0x7c7f93)
    }
    // Accent colors
    fn blue() -> Rgba {
        rgb(0x1e66f5)
    }
    fn base_solid() -> Rgba {
        rgb(0xeff1f5)
    }
    fn mantle_solid() -> Rgba {
        rgb(0xe6e9ef)
    }
    fn crust_solid() -> Rgba {
        rgb(0xdce0e8)
    }
}

#[derive(Clone)]
struct SearchItem {
    title: String,
    subtitle: String,
}

struct SearchBar {
    focus_handle: FocusHandle,
    query: String,
    selected_index: usize,
    items: Vec<SearchItem>,
    was_active: bool,
    acrylic_applied: bool,
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

        let acrylic_applied = platform::apply_acrylic(window);
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
            selected_index: 0,
            items: Self::default_items(),
            was_active: false,
            acrylic_applied,
            _subscription: subscription,
            active_window,
        }
    }

    fn default_items() -> Vec<SearchItem> {
        vec![
            SearchItem {
                title: "System Settings".to_string(),
                subtitle: "Preferences and system configuration".to_string(),
            },
            SearchItem {
                title: "Terminal".to_string(),
                subtitle: "Open command prompt or PowerShell".to_string(),
            },
            SearchItem {
                title: "File Explorer".to_string(),
                subtitle: "Browse files and directories".to_string(),
            },
            SearchItem {
                title: "Web Browser".to_string(),
                subtitle: "Launch default internet browser".to_string(),
            },
        ]
    }

    // When query is empty, return no items so initial search bar is completely empty
    fn filtered_items(&self) -> Vec<SearchItem> {
        if self.query.trim().is_empty() {
            return Vec::new();
        }
        let q = self.query.to_lowercase();
        let mut matches: Vec<SearchItem> = self
            .items
            .iter()
            .filter(|it| {
                it.title.to_lowercase().contains(&q)
                    || it.subtitle.to_lowercase().contains(&q)
            })
            .cloned()
            .collect();

        if matches.is_empty() {
            matches.push(SearchItem {
                title: format!("Run: \"{}\"", self.query),
                subtitle: "Execute custom command or search".to_string(),
            });
        }
        matches
    }

    fn execute(&self, window: &mut Window) {
        let results = self.filtered_items();
        if let Some(item) = results.get(self.selected_index) {
            println!("Selected: {} ({})", item.title, item.subtitle);
        }
        *self.active_window.borrow_mut() = None;
        window.remove_window();
    }
}

impl Render for SearchBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let results = self.filtered_items();
        let is_empty = self.query.trim().is_empty();
        let selected = self.selected_index.min(results.len().saturating_sub(1));

        let base_bg = if self.acrylic_applied {
            Latte::base_acrylic()
        } else {
            Latte::base_solid()
        };
        let mantle_bg = if self.acrylic_applied {
            Latte::mantle_acrylic()
        } else {
            Latte::mantle_solid()
        };
        let crust_bg = if self.acrylic_applied {
            Latte::crust_acrylic()
        } else {
            Latte::crust_solid()
        };

        let mut root = div()
            .track_focus(&self.focus_handle)
            .key_context("search_bar")
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => {
                        *this.active_window.borrow_mut() = None;
                        window.remove_window();
                    }
                    "enter" => {
                        this.execute(window);
                    }
                    "up" => {
                        let total = this.filtered_items().len();
                        if total > 0 {
                            this.selected_index = if this.selected_index == 0 {
                                total - 1
                            } else {
                                this.selected_index - 1
                            };
                            cx.notify();
                        }
                    }
                    "down" => {
                        let total = this.filtered_items().len();
                        if total > 0 {
                            this.selected_index = (this.selected_index + 1) % total;
                            cx.notify();
                        }
                    }
                    "backspace" => {
                        this.query.pop();
                        this.selected_index = 0;
                        let target_height = if this.query.trim().is_empty() {
                            BAR_HEIGHT
                        } else {
                            EXPANDED_HEIGHT
                        };
                        window.resize(size(px(WIN_WIDTH), px(target_height)));
                        cx.notify();
                    }
                    _ => {
                        let mut changed = false;
                        if let Some(ch) = &event.keystroke.key_char {
                            this.query.push_str(ch);
                            changed = true;
                        } else if event.keystroke.key.chars().count() == 1 {
                            this.query.push_str(&event.keystroke.key);
                            changed = true;
                        }
                        if changed {
                            this.selected_index = 0;
                            let target_height = if this.query.trim().is_empty() {
                                BAR_HEIGHT
                            } else {
                                EXPANDED_HEIGHT
                            };
                            window.resize(size(px(WIN_WIDTH), px(target_height)));
                            cx.notify();
                        }
                    }
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(base_bg)
            .rounded(px(12.0))
            .border_1()
            .border_color(Latte::border_glass())
            .overflow_hidden()
            // Header search input (clean minimal acrylic bar)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(BAR_HEIGHT))
                    .px(px(20.0))
                    .bg(if is_empty {
                        rgba(0x00000000)
                    } else {
                        mantle_bg
                    })
                    .border_b_1()
                    .border_color(if is_empty {
                        rgba(0x00000000)
                    } else {
                        Latte::surface0()
                    })
                    .child(
                        div()
                            .mr(px(14.0))
                            .text_color(Latte::blue())
                            .text_size(px(18.0))
                            .child("›"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.0))
                            .text_color(Latte::text())
                            .child(if self.query.is_empty() {
                                div()
                                    .text_color(Latte::overlay0())
                                    .child("Type a command or search...")
                            } else {
                                div()
                                    .flex()
                                    .items_center()
                                    .child(self.query.clone())
                                    .child(
                                        div()
                                            .ml(px(2.0))
                                            .w(px(2.0))
                                            .h(px(18.0))
                                            .bg(Latte::blue()),
                                    )
                            }),
                    )
                    .child(
                        div()
                            .px(px(6.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(rgba(0xccd0da66))
                            .text_color(Latte::subtext0())
                            .text_size(px(11.0))
                            .child("esc"),
                    ),
            );

        // Only render results list and status footer if query is not empty
        if !is_empty {
            root = root
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .py(px(6.0))
                        .px(px(6.0))
                        .children(results.into_iter().enumerate().map(|(idx, item)| {
                            let is_active = idx == selected;
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .px(px(12.0))
                                .py(px(8.0))
                                .bg(if is_active {
                                    Latte::surface1()
                                } else {
                                    rgba(0x00000000)
                                })
                                .border_l_2()
                                .border_color(if is_active {
                                    Latte::blue()
                                } else {
                                    rgba(0x00000000)
                                })
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(14.0))
                                                .text_color(if is_active {
                                                    Latte::blue()
                                                } else {
                                                    Latte::text()
                                                })
                                                .child(item.title),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(Latte::subtext0())
                                                .child(item.subtitle),
                                        ),
                                )
                                .child(if is_active {
                                    div()
                                        .px(px(7.0))
                                        .py(px(2.0))
                                        .bg(Latte::blue())
                                        .text_color(Latte::base_solid())
                                        .text_size(px(10.0))
                                        .child("↵ Enter")
                                } else {
                                    div()
                                })
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .h(px(28.0))
                        .px(px(14.0))
                        .bg(crust_bg)
                        .border_t_1()
                        .border_color(Latte::surface0())
                        .text_size(px(11.0))
                        .text_color(Latte::overlay2())
                        .child("Catppuccin Latte Acrylic")
                        .child("Navigate: ↑↓ • Open: ↵ • Close: ESC"),
                );
        }

        root
    }
}

fn open_search_window(
    cx: &mut App,
    active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
) -> WindowHandle<SearchBar> {
    let display = cx.displays().into_iter().next();
    // Opens initially as just the search bar
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
        // Native acrylic blur backdrop via GPUI compositor
        window_background: WindowBackgroundAppearance::Blurred,
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
