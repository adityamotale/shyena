use std::{cell::RefCell, rc::Rc, thread};
use async_channel::{Receiver, Sender};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use gpui::*;

// Catppuccin Latte palette helpers
struct Latte;
impl Latte {
    // Acrylic translucent Base background
    fn base_acrylic() -> Rgba {
        rgba(0xeff1f5c8)
    }
    // Acrylic translucent Mantle for header
    fn mantle_acrylic() -> Rgba {
        rgba(0xe6e9efe0)
    }
    // Acrylic translucent Crust for footer
    fn crust_acrylic() -> Rgba {
        rgba(0xdce0e8d0)
    }
    // Surface layers for borders and active items
    fn surface0() -> Rgba {
        rgb(0xccd0da)
    }
    fn surface1() -> Rgba {
        rgba(0xbcc0cce8)
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

    fn filtered_items(&self) -> Vec<SearchItem> {
        if self.query.trim().is_empty() {
            return self.items.clone();
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
        let selected = self.selected_index.min(results.len().saturating_sub(1));

        div()
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
                        cx.notify();
                    }
                    _ => {
                        if let Some(ch) = &event.keystroke.key_char {
                            this.query.push_str(ch);
                            this.selected_index = 0;
                            cx.notify();
                        } else if event.keystroke.key.chars().count() == 1 {
                            this.query.push_str(&event.keystroke.key);
                            this.selected_index = 0;
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
            // Header search input (sharp rectangular aesthetic)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(56.0))
                    .px(px(18.0))
                    .bg(Latte::mantle_acrylic())
                    .border_b_1()
                    .border_color(Latte::surface0())
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
            )
            // Results list
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
            // Status bar footer
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
            )
    }
}

fn open_search_window(
    cx: &mut App,
    active_window: Rc<RefCell<Option<WindowHandle<SearchBar>>>>,
) -> WindowHandle<SearchBar> {
    let display = cx.displays().into_iter().next();
    let win_width = px(620.0);
    let win_height = px(320.0);

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
        // Acrylic blur behind effect on Windows
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
