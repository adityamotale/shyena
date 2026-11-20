//! Launcher window, input handling, rendering, and window lifecycle.
//!
//! GPUI drives the event loop: keyboard input updates the query and candidate
//! list, and Enter or a click produces a [`CommandIntent`] for the UI to run.
//! Failures surface in-place through [`ErrorState`].

use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use async_channel::Sender;
#[cfg(test)]
use gpui::TestAppContext;
use gpui::{
    App, AppContext, Bounds, Context, Div, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Render, Rgba, Styled, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, point,
    px, size,
};

use crate::commands::{self, InternalCommand};
use crate::config::{self, ConfigStore, ReloadOutcome};
use crate::dispatch::{
    ActionKind, CandidateAction, CommandIntent, PsOutcome, Scope, resolve_command, split_scope,
};
use crate::effects::Effects;
use crate::theme::{Palette, Theme};

const WIN_WIDTH: f32 = 660.0;
const BAR_HEIGHT: f32 = 62.0;
const ITEM_HEIGHT: f32 = 32.0;
const MAX_VISIBLE_ITEMS: usize = 6;
const MAX_OUTPUT_LINES: usize = 8;
const OUTPUT_LINE_HEIGHT: f32 = 18.0;
const OUTPUT_HEADER_HEIGHT: f32 = 24.0;
const OUTPUT_PADDING: f32 = 12.0;

/// Output of a completed PowerShell function shown below the bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PsOutput {
    pub(crate) name: String,
    pub(crate) text: String,
}

/// Shared handle to the single launcher window, or `None` while dismissed.
pub(crate) type ActiveWindow = Rc<RefCell<Option<WindowHandle<SearchBar>>>>;

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

/// Trim only while no launcher window is open. The delay keeps the syscall off
/// the teardown/next-frame path, and the guard cancels a pending trim when the
/// user reopens the bar within that window.
pub(crate) fn schedule_trim(
    cx: &App,
    active_window: &ActiveWindow,
    effects: &Effects,
    delay: Duration,
) {
    let active = active_window.clone();
    let effects = effects.clone();
    cx.spawn(async move |cx| {
        // The GPUI timer honours the test dispatcher's virtual clock.
        cx.background_executor().timer(delay).await;
        if active.borrow().is_none() {
            effects.trim_working_set();
        }
    })
    .detach();
}

/// Hidden window that keeps GPUI's run loop (and the hotkey/error channels)
/// alive after the launcher window is dismissed.
pub(crate) struct AnchorWindow;

impl Render for AnchorWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

/// Keeps the process alive after the launcher window closes. The handle is
/// intentionally returned to (and held by) the composition root.
pub(crate) fn open_anchor_window(cx: &mut App) -> WindowHandle<AnchorWindow> {
    cx.open_window(
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
    .expect("Failed to create anchor window")
}

pub(crate) struct SearchBar {
    focus_handle: FocusHandle,
    query: String,
    was_active: bool,
    _subscription: Subscription,
    active_window: ActiveWindow,
    config: Arc<config::Config>,
    ps_outcome_tx: Sender<PsOutcome>,
    store: Rc<RefCell<ConfigStore>>,
    effects: Effects,
    theme: Theme,
    selected_index: usize,
    filtered_actions: Vec<CandidateAction>,
    error: Option<ErrorState>,
    output: Option<PsOutput>,
}

impl SearchBar {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        active_window: ActiveWindow,
        ps_outcome_tx: Sender<PsOutcome>,
        store: Rc<RefCell<ConfigStore>>,
        effects: Effects,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window);

        let config = store.borrow().current();
        let theme = config.theme;

        effects.configure_window(window);

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
            ps_outcome_tx,
            store,
            effects,
            theme,
            selected_index: 0,
            filtered_actions: Vec::new(),
            error: None,
            output: None,
        };
        instance.update_filtered_actions(window);
        instance
    }

    fn output_panel_height(&self) -> f32 {
        if let Some(out) = &self.output {
            let line_count = f32::from(
                u8::try_from(out.text.lines().count().clamp(1, MAX_OUTPUT_LINES))
                    .unwrap_or(u8::MAX),
            );
            OUTPUT_HEADER_HEIGHT + line_count * OUTPUT_LINE_HEIGHT + OUTPUT_PADDING
        } else {
            0.0
        }
    }

    fn update_filtered_actions(&mut self, window: &mut Window) {
        let output_h = self.output_panel_height();
        let trimmed = self.query.trim();
        if trimmed.is_empty() {
            self.filtered_actions.clear();
            self.selected_index = 0;
            Self::resize_to_height(window, BAR_HEIGHT + output_h);
            return;
        }

        // Suggestions off: keep the bar at base height and skip candidate
        // evaluation. Exact-key resolution on Enter still works via
        // `resolve_command`.
        if !self.config.suggestions.enabled() {
            self.filtered_actions.clear();
            self.selected_index = 0;
            Self::resize_to_height(window, BAR_HEIGHT + output_h);
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
        Self::resize_to_height(window, BAR_HEIGHT + list_height + output_h);
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
        self.output = None;
        self.query.clear();
        self.filtered_actions.clear();
        self.selected_index = 0;
        Self::resize_to_height(window, BAR_HEIGHT);
        self.focus_handle.focus(window);
        cx.notify();
    }

    pub(crate) fn show_error(
        &mut self,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_error(ErrorState::Runtime(message), window, cx);
    }

    /// Displays stdout output from a completed PowerShell function.
    pub(crate) fn show_ps_output(
        &mut self,
        name: String,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        self.output = Some(PsOutput { name, text });
        self.update_filtered_actions(window);
        self.focus_handle.focus(window);
        cx.notify();
    }

    fn dismiss_output(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.output = None;
        self.update_filtered_actions(window);
        self.focus_handle.focus(window);
        cx.notify();
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
        schedule_trim(
            cx,
            &self.active_window,
            &self.effects,
            Duration::from_millis(200),
        );
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

        if this.output.is_some() && event.keystroke.key == "escape" {
            this.dismiss_output(window, cx);
            return;
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                this.close(window, cx);
            }
            "down" => {
                let visible = this.filtered_actions.len().min(MAX_VISIBLE_ITEMS);
                if visible > 0 {
                    this.selected_index = (this.selected_index + 1) % visible;
                    cx.notify();
                }
            }
            "up" => {
                let visible = this.filtered_actions.len().min(MAX_VISIBLE_ITEMS);
                if visible > 0 {
                    this.selected_index = if this.selected_index == 0 {
                        visible - 1
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
                        .map_or_default(|(scope, _)| scope.prefix().to_string());

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

    /// Spawns the configured application with static then dynamic arguments.
    fn launch_app(&self, key: &str, dynamic_args: &[String]) {
        if let Some(app) = self.config.apps.get(key) {
            let mut args = app.args.clone();
            args.extend_from_slice(dynamic_args);
            self.effects.spawn_app(&app.path, &args);
        }
    }

    /// Dispatches a PowerShell function through the injected effects boundary.
    fn launch_ps(&self, key: &str, dynamic_args: &[String]) {
        if let Some(ps) = self.config.ps.get(key) {
            self.effects
                .spawn_powershell(key, &ps.body, dynamic_args, self.ps_outcome_tx.clone());
        }
    }

    /// Single execution entry point for Enter and suggestion clicks.
    fn activate(&mut self, intent: &CommandIntent, window: &mut Window, cx: &mut Context<Self>) {
        match intent {
            CommandIntent::Internal { key, args } => {
                let spec = commands::find(key).expect("internal command was resolved");
                self.run_internal(spec.command, args, window, cx);
            }
            CommandIntent::App { key, args } => {
                self.launch_app(key, args);
                self.close(window, cx);
            }
            CommandIntent::Ps { key, args } => {
                self.launch_ps(key, args);
                self.close(window, cx);
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
            InternalCommand::Update => {
                if !args.is_empty() {
                    self.show_error("update takes no arguments".to_string(), window, cx);
                    return;
                }

                self.effects.spawn_updater();
                self.close(window, cx);
            }
            InternalCommand::Quit => {
                if !args.is_empty() {
                    self.show_error("quit takes no arguments".to_string(), window, cx);
                    return;
                }

                self.close(window, cx);
                cx.quit();
            }
        }
    }

    fn execute(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.query.trim().is_empty() && self.filtered_actions.is_empty() {
            self.close(window, cx);
            return;
        }

        let intent = match resolve_command(
            &self.config,
            &self.query,
            self.filtered_actions.get(self.selected_index),
        ) {
            Ok(intent) => intent,
            Err(message) => {
                self.show_error(message, window, cx);
                return;
            }
        };

        self.activate(&intent, window, cx);
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
            .children(self.render_output(palette))
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
                    let intent = item.to_intent(Vec::new());

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
                                this.activate(&intent, window, cx);
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

    fn render_output(&self, palette: Palette) -> Option<Div> {
        let output = self.output.as_ref()?;

        Some(
            div()
                .w_full()
                .border_t_1()
                .border_color(palette.border_subtle)
                .flex()
                .flex_col()
                .bg(palette.surface0)
                .child(
                    div()
                        .h(px(OUTPUT_HEADER_HEIGHT))
                        .px(px(12.0))
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
                                .child(
                                    div()
                                        .text_color(palette.ps)
                                        .child(format!("stdout:{}", output.name)),
                                )
                                .child(div().text_color(palette.bracket).child("]")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .child(div().text_color(palette.hint).child("[ ESC: clear ]"))
                                .child(div().text_color(palette.bracket).child("─┐")),
                        ),
                )
                .child(
                    div()
                        .max_h(px(f32::from(
                            u8::try_from(MAX_OUTPUT_LINES).unwrap_or(u8::MAX),
                        ) * OUTPUT_LINE_HEIGHT
                            + OUTPUT_PADDING))
                        .overflow_hidden()
                        .px(px(12.0))
                        .py(px(6.0))
                        .flex()
                        .flex_col()
                        .children(output.text.lines().take(MAX_OUTPUT_LINES).map(|line| {
                            div()
                                .h(px(OUTPUT_LINE_HEIGHT))
                                .flex()
                                .items_center()
                                .text_size(px(12.0))
                                .text_color(palette.text)
                                .child(line.to_string())
                        })),
                ),
        )
    }
}

/// Opens the launcher, retrying a previously failed config load on the way in.
pub(crate) fn open_search_window(
    cx: &mut App,
    active_window: &ActiveWindow,
    ps_outcome_tx: Sender<PsOutcome>,
    store: &Rc<RefCell<ConfigStore>>,
    effects: &Effects,
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
    let effects = effects.clone();
    let handle = cx
        .open_window(options, move |window, cx| {
            cx.new(|cx| SearchBar::new(window, cx, active_ref, ps_outcome_tx, store_ref, effects))
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

/// Toggles the launcher: dismissing an open bar or opening a fresh one.
pub(crate) fn toggle_search_window(
    cx: &mut App,
    active_window: &ActiveWindow,
    ps_outcome_tx: &Sender<PsOutcome>,
    store: &Rc<RefCell<ConfigStore>>,
    effects: &Effects,
) {
    let opened = active_window.borrow_mut().take();
    let was_open = opened.is_some_and(|handle| {
        handle
            .update(cx, |_, window, _| window.remove_window())
            .is_ok()
    });

    if was_open {
        schedule_trim(cx, active_window, effects, Duration::from_millis(200));
    } else {
        let handle = open_search_window(cx, active_window, ps_outcome_tx.clone(), store, effects);
        *active_window.borrow_mut() = Some(handle);
    }
}

/// Returns the error text currently shown, if any. Used by the async pump tests.
#[cfg(test)]
pub(crate) fn error_message<C: AppContext>(
    window: &WindowHandle<SearchBar>,
    cx: &mut C,
) -> Option<String> {
    window
        .update(cx, |bar, _, _| {
            bar.error.as_ref().map(|e| e.message().to_string())
        })
        .ok()
        .flatten()
}

/// Returns the output (name, text) currently shown, if any. Used by the async pump tests.
#[cfg(test)]
pub(crate) fn output_message<C: AppContext>(
    window: &WindowHandle<SearchBar>,
    cx: &mut C,
) -> Option<(String, String)> {
    window
        .update(cx, |bar, _, _| {
            bar.output
                .as_ref()
                .map(|o| (o.name.clone(), o.text.clone()))
        })
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::{Keystroke, Modifiers, VisualTestContext};

    use super::*;
    use crate::test_support::{AppLaunch, RecordingEffects, TestConfig, UI_CONFIG};

    struct Harness {
        cx: VisualTestContext,
        window: WindowHandle<SearchBar>,
        active: ActiveWindow,
        effects: Arc<RecordingEffects>,
        config: TestConfig,
    }

    fn open(cx: &TestAppContext, source: &str) -> Harness {
        let mut cx = cx.new_app();
        let effects = RecordingEffects::new();
        let config = TestConfig::new("harness", source);
        let active: ActiveWindow = Rc::new(RefCell::new(None));
        let (ps_tx, _ps_rx) = async_channel::unbounded();
        let store = config.store.clone();
        let recorder = effects.clone();
        let window = cx.add_window(|window, cx| {
            SearchBar::new(window, cx, active.clone(), ps_tx, store, recorder)
        });
        *active.borrow_mut() = Some(window);
        let cx = VisualTestContext::from_window(*window, &cx);
        cx.run_until_parked();
        Harness {
            cx,
            window,
            active,
            effects,
            config,
        }
    }

    impl Harness {
        fn state<R>(&mut self, read: impl FnOnce(&SearchBar) -> R) -> R {
            self.window
                .update(&mut self.cx, |bar, _, _| read(bar))
                .expect("launcher window is open")
        }

        fn query(&mut self) -> String {
            self.state(|bar| bar.query.clone())
        }

        fn candidates(&mut self) -> Vec<String> {
            self.state(|bar| bar.filtered_actions.iter().map(|c| c.key.clone()).collect())
        }

        fn selected(&mut self) -> usize {
            self.state(|bar| bar.selected_index)
        }

        fn error(&mut self) -> Option<String> {
            self.state(|bar| bar.error.as_ref().map(|e| e.message().to_string()))
        }

        fn output(&mut self) -> Option<(String, String)> {
            self.state(|bar| {
                bar.output
                    .as_ref()
                    .map(|o| (o.name.clone(), o.text.clone()))
            })
        }

        /// Drives timers scheduled with `Timer::after` on the foreground executor.
        fn advance(&mut self, duration: Duration) {
            self.cx.cx.executor().advance_clock(duration);
            self.cx.run_until_parked();
        }
    }

    #[gpui::test]
    fn typing_filters_candidates_and_tab_completes_scope(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.cx.simulate_input("@ c");
        assert_eq!(h.query(), "@ c");
        assert_eq!(h.candidates(), vec!["calc"]);

        // key_char path (real IME input) must behave like the key fallback above.
        h.cx.dispatch_keystroke(
            *h.window,
            Keystroke {
                modifiers: Modifiers::none(),
                key: "a".into(),
                key_char: Some("a".into()),
            },
        );
        assert_eq!(h.query(), "@ ca");

        h.cx.simulate_keystrokes("tab");
        // Tab completion keeps the scope prefix attached to the key.
        assert_eq!(h.query(), "@calc ");
        assert_eq!(h.selected(), 0);
    }

    #[gpui::test]
    fn enter_launches_app_with_static_and_dynamic_args(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.cx.simulate_input("@notepad");
        h.cx.simulate_keystrokes("space");
        h.cx.simulate_input("extra");
        h.cx.simulate_keystrokes("enter");

        assert_eq!(
            h.effects.app_launches(),
            vec![AppLaunch {
                path: PathBuf::from(r"C:\Windows\System32\notepad.exe"),
                args: vec!["--flag".to_string(), "extra".to_string()],
            }]
        );
        assert!(h.active.borrow().is_none(), "window closes after launch");
        h.advance(Duration::from_millis(300));
        assert_eq!(h.effects.trims(), 1);
    }

    #[gpui::test]
    fn enter_dispatches_powershell_with_dynamic_args(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.cx.simulate_input("#brightness");
        h.cx.simulate_keystrokes("space");
        h.cx.simulate_input("42");
        h.cx.simulate_keystrokes("enter");

        let launches = h.effects.ps_launches();
        assert_eq!(launches.len(), 1);
        assert_eq!(launches[0].name, "brightness");
        assert_eq!(launches[0].args, vec!["42".to_string()]);
        assert!(launches[0].body.contains("param([int]$Level)"));
        assert!(h.active.borrow().is_none());
    }

    #[gpui::test]
    fn arrow_keys_change_selection(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.cx.simulate_keystrokes("@");
        assert_eq!(h.candidates(), vec!["calc", "notepad"]);
        assert_eq!(h.selected(), 0);

        h.cx.simulate_keystrokes("down");
        assert_eq!(h.selected(), 1);

        h.cx.simulate_keystrokes("enter");
        assert_eq!(
            h.effects.app_launches()[0].path,
            PathBuf::from(r"C:\Windows\System32\notepad.exe")
        );
    }

    #[gpui::test]
    fn arrow_keys_clamp_to_max_visible_items(cx: &TestAppContext) {
        let config_many_apps = r"
Register-App 'app1' @{ Path = '1.exe' }
Register-App 'app2' @{ Path = '2.exe' }
Register-App 'app3' @{ Path = '3.exe' }
Register-App 'app4' @{ Path = '4.exe' }
Register-App 'app5' @{ Path = '5.exe' }
Register-App 'app6' @{ Path = '6.exe' }
Register-App 'app7' @{ Path = '7.exe' }
Register-App 'app8' @{ Path = '8.exe' }
";
        let mut h = open(cx, config_many_apps);
        h.cx.simulate_keystrokes("@");
        assert!(h.candidates().len() > MAX_VISIBLE_ITEMS);
        assert_eq!(h.selected(), 0);

        // Pressing up from 0 wraps to the last visible item (MAX_VISIBLE_ITEMS - 1)
        h.cx.simulate_keystrokes("up");
        assert_eq!(h.selected(), MAX_VISIBLE_ITEMS - 1);

        // Pressing down from last visible item wraps back to 0
        h.cx.simulate_keystrokes("down");
        assert_eq!(h.selected(), 0);
    }

    #[gpui::test]
    fn clicking_a_candidate_activates_it(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.cx.simulate_keystrokes("@");
        // Header is 20px, input 42px, rows are 32px: first row centre is y=78.
        // Our handler acts on mouse-down; mouse-up cannot be delivered because
        // activation closes the window immediately.
        h.cx.simulate_mouse_down(
            point(px(100.0), px(78.0)),
            MouseButton::Left,
            Modifiers::none(),
        );

        assert_eq!(h.effects.app_launches()[0].path, PathBuf::from("calc.exe"));
    }

    #[gpui::test]
    fn reload_swaps_config_and_closes_window(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);
        h.config
            .rewrite("Set-Theme 'light'\nRegister-App 'term' @{ Path = 'wt.exe' }\n");

        h.cx.simulate_input(":reload");
        let query = h.query();
        assert_eq!(query, ":reload", "query before enter");
        h.cx.simulate_keystrokes("enter");

        assert_eq!(h.config.store.borrow().current().theme, Theme::Light);
        assert!(h.config.store.borrow().current().apps.contains_key("term"));
        assert!(h.active.borrow().is_none());
    }

    #[gpui::test]
    fn internal_update_dispatches_updater(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.cx.simulate_input(":update");
        let query = h.query();
        assert_eq!(query, ":update", "query before enter");
        h.cx.simulate_keystrokes("enter");

        assert_eq!(h.effects.updaters(), 1);
        assert!(h.active.borrow().is_none());
    }

    #[gpui::test]
    fn internal_quit_closes_window(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.cx.simulate_input(":quit");
        assert_eq!(h.query(), ":quit", "query before enter");
        h.cx.simulate_keystrokes("enter");

        // The launcher window is removed before quit is signalled.
        assert!(h.active.borrow().is_none(), "window closed after :quit");
    }

    #[gpui::test]
    fn quit_rejects_arguments(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.cx.simulate_input(":quit extra");
        h.cx.simulate_keystrokes("enter");

        assert_eq!(
            h.error().as_deref(),
            Some("quit takes no arguments"),
            "spurious args produce an inline error"
        );
        assert!(h.active.borrow().is_some(), "window stays open on error");
    }

    #[gpui::test]
    fn runtime_error_is_dismissible_without_closing(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.cx.simulate_input("bogus");
        let query = h.query();
        assert_eq!(query, "bogus", "query before enter");
        h.cx.simulate_keystrokes("enter");
        assert_eq!(h.error().as_deref(), Some("unknown command 'bogus'"));
        assert!(h.active.borrow().is_some());

        h.cx.simulate_keystrokes("escape");
        assert_eq!(h.error(), None);
        assert_eq!(h.query(), "");
        assert!(h.active.borrow().is_some());
    }

    #[gpui::test]
    fn fatal_config_error_closes_on_escape(cx: &TestAppContext) {
        let config = TestConfig::new("fatal", "Set-Theme 'mocha'");
        let effects = RecordingEffects::new();
        let active: ActiveWindow = Rc::new(RefCell::new(None));
        let (ps_tx, _ps_rx) = async_channel::unbounded();
        let recorder: Effects = effects.clone();
        let store = config.store.clone();
        let window = cx.update(|app| open_search_window(app, &active, ps_tx, &store, &recorder));
        let mut cx = VisualTestContext::from_window(*window, cx);
        cx.run_until_parked();

        let message = error_message(&window, &mut cx.cx).expect("config error shown");
        assert!(message.contains("invalid theme"), "{message}");

        cx.simulate_keystrokes("escape");
        assert!(active.borrow().is_none());
    }

    #[gpui::test]
    fn empty_query_enter_only_closes(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.cx.simulate_keystrokes("enter");

        assert_eq!(h.effects.app_launches(), Vec::new());
        assert_eq!(h.effects.ps_launches(), Vec::new());
        h.advance(Duration::from_millis(300));
        assert!(h.active.borrow().is_none());
        assert_eq!(h.effects.trims(), 1);
        assert_eq!(h.effects.window_configures(), 1);
    }

    #[gpui::test]
    fn suggestions_off_still_resolves_exact_keys(cx: &TestAppContext) {
        let mut h = open(
            cx,
            "Set-Suggestions 'off'\nRegister-App 'calc' @{ Path = 'calc.exe' }",
        );

        h.cx.simulate_input("@calc");
        assert_eq!(h.candidates(), Vec::<String>::new());
        h.cx.simulate_keystrokes("enter");

        assert_eq!(h.effects.app_launches()[0].path, PathBuf::from("calc.exe"));
    }

    #[gpui::test]
    fn ps_output_displayed_and_cleared_on_escape(cx: &TestAppContext) {
        let mut h = open(cx, UI_CONFIG);

        h.window
            .update(&mut h.cx, |bar, window, cx| {
                bar.show_ps_output("uuid".into(), "1234-5678".into(), window, cx);
            })
            .unwrap();

        assert_eq!(h.output(), Some(("uuid".into(), "1234-5678".into())));
        assert!(h.active.borrow().is_some());

        h.cx.simulate_keystrokes("escape");
        assert_eq!(h.output(), None);
        assert!(h.active.borrow().is_some());
    }
}
