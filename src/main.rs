use std::{cell::RefCell, rc::Rc, thread};
use async_channel::{Receiver, Sender};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use gpui::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const WIN_WIDTH: f32 = 620.0;
const BAR_HEIGHT: f32 = 56.0;
const EXPANDED_HEIGHT: f32 = 320.0;

// Win32 DWM & User32 composition definitions for Acrylic blur
#[repr(C)]
struct MARGINS {
    cx_left_width: i32,
    cx_right_width: i32,
    cy_top_height: i32,
    cy_bottom_height: i32,
}

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

unsafe fn apply_acrylic(hwnd: *mut std::ffi::c_void) {
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmExtendFrameIntoClientArea(
            h_wnd: *mut std::ffi::c_void,
            p_mar_inset: *const MARGINS,
        ) -> i32;
        fn DwmSetWindowAttribute(
            h_wnd: *mut std::ffi::c_void,
            dw_attribute: u32,
            pv_attribute: *const std::ffi::c_void,
            cb_attribute: u32,
        ) -> i32;
    }

    // Extend DWM frame into client area so backdrop blur is visible across entire window
    let margins = MARGINS {
        cx_left_width: -1,
        cx_right_width: -1,
        cy_top_height: -1,
        cy_bottom_height: -1,
    };
    let _ = unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) };

    // Windows 11 DWMWA_SYSTEMBACKDROP_TYPE = 38; DWMSBT_TRANSIENTWINDOW = 3 (Acrylic)
    let backdrop_type: u32 = 3;
    let hr = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            38,
            &backdrop_type as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };

    // Windows 10 Fallback: SetWindowCompositionAttribute with ACCENT_ENABLE_ACRYLICBLURBEHIND = 4
    if hr != 0 {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetModuleHandleA(lp_module_name: *const u8) -> *mut std::ffi::c_void;
            fn GetProcAddress(
                h_module: *mut std::ffi::c_void,
                lp_proc_name: *const u8,
            ) -> *mut std::ffi::c_void;
        }
        let user32 = unsafe { GetModuleHandleA(b"user32.dll\0".as_ptr()) };
        if !user32.is_null() {
            let func = unsafe {
                GetProcAddress(user32, b"SetWindowCompositionAttribute\0".as_ptr())
            };
            if !func.is_null() {
                type SetAttrFn = unsafe extern "system" fn(
                    *mut std::ffi::c_void,
                    *mut WindowCompositionAttribData,
                ) -> i32;
                let set_attr: SetAttrFn = unsafe { std::mem::transmute(func) };
                let accent = AccentPolicy {
                    accent_state: 4,
                    accent_flags: 2,
                    gradient_color: 0x66f5f1ef,
                    animation_id: 0,
                };
                let mut data = WindowCompositionAttribData {
                    attrib: 0x13,
                    pv_data: &accent as *const _ as *mut _,
                    cb_data: std::mem::size_of::<AccentPolicy>(),
                };
                let _ = unsafe { set_attr(hwnd, &mut data) };
            }
        }
    }
}

// Catppuccin Latte palette helpers
struct Latte;
impl Latte {
    // Acrylic translucent Base background (~55% alpha to let blur and grain show through)
    fn base_acrylic() -> Rgba {
        rgba(0xeff1f58c)
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

        // Apply true Win32 Acrylic blur behind the client area
        if let Ok(handle) = HasWindowHandle::window_handle(window) {
            if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                let hwnd = win32.hwnd.get() as *mut std::ffi::c_void;
                unsafe { apply_acrylic(hwnd) };
            }
        }

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
            .bg(Latte::base_acrylic())
            .border_1()
            .border_color(Latte::surface0())
            .overflow_hidden()
            // Header search input (sharp rectangular aesthetic with zero radius)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(BAR_HEIGHT))
                    .px(px(18.0))
                    .bg(Latte::mantle_acrylic())
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
                            .child("⌕"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.0))
                            .text_color(Latte::text())
                            .child(if self.query.is_empty() {
                                div()
                                    .text_color(Latte::overlay0())
                                    .child("Search or type a command... (Esc to dismiss)")
                            } else {
                                div()
                                    .flex()
                                    .child(self.query.clone())
                                    .child(
                                        div()
                                            .text_color(Latte::blue())
                                            .child("|")
                                    )
                            }),
                    )
                    .child(
                        div()
                            .px(px(8.0))
                            .py(px(2.0))
                            .bg(Latte::surface0())
                            .border_1()
                            .border_color(Latte::surface0())
                            .text_color(Latte::subtext1())
                            .text_size(px(11.0))
                            .child("ESC"),
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
                        .bg(Latte::crust_acrylic())
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
        let y = screen_bounds.origin.y + (screen_bounds.size.height - win_height) / 2.0;
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
        // Transparent DX swapchain allows DWM acrylic composition to render through
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
    // Register Alt + Space
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Space);
    let hotkey_id = hotkey.id();
    manager.register(hotkey).expect("Failed to register Alt+Space global hotkey");

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
