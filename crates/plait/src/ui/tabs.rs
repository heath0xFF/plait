//! Window tab controls. The core owns every transition and presentation fact.
use super::{Plait, theme::*, thread_controls::TextTooltip};
use gpui::{prelude::*, *};
use plait_core::tabs::{
    CheckoutKey, HostKey, TabError, TabId, ThreadTarget, WindowId, WindowTabs,
    row::{self as model, RowContext, RowWidths, TabLayout, TabMetadata, TabState},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static WINDOW_IDS: AtomicU64 = AtomicU64::new(1);

actions!(
    plait,
    [
        NewTab,
        CloseTab,
        ReopenTab,
        NextTab,
        PreviousTab,
        MoveTabLeft,
        MoveTabRight,
        ListTabs,
        TabJump1,
        TabJump2,
        TabJump3,
        TabJump4,
        TabJump5,
        TabJump6,
        TabJump7,
        TabJump8,
        TabJump9
    ]
);

pub(super) struct Tabs {
    pub model: WindowTabs,
    pub lifecycle: super::tab_lifecycle::Lifecycle,
    strip: Option<Entity<TabStrip>>,
    strip_theme: u32,
    pub host: HostKey,
    pub rows: Vec<TabMetadata>,
    layout: Option<TabLayout>,
    measured: Option<(f32, bool)>,
    first_visible: usize,
    pub list_open: bool,
    drag: Option<Drag>,
}
struct TabStrip {
    owner: WeakEntity<Plait>,
}
impl Render for TabStrip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |p, cx| p.tab_strip(cx).into_any_element())
            .unwrap_or_else(|_| div().into_any_element())
    }
}

struct Drag {
    id: TabId,
    start: Pixels,
    destination: Option<usize>,
}
impl Default for Tabs {
    fn default() -> Self {
        let window = WINDOW_IDS.fetch_add(1, Ordering::Relaxed);
        // Unverified hosts can use session tabs; this identity is never durable.
        Self {
            model: WindowTabs::new(WindowId(window)),
            lifecycle: Default::default(),
            strip: None,
            strip_theme: 0,
            host: HostKey {
                id: format!("session-{window}"),
                store: PathBuf::from("/"),
            },
            rows: Vec::new(),
            layout: None,
            measured: None,
            first_visible: 0,
            list_open: false,
            drag: None,
        }
    }
}

fn tint(state: TabState, active: bool) -> u32 {
    match state {
        TabState::Approve | TabState::Interrupted => colors().amber,
        TabState::Failed => colors().red,
        TabState::Working => colors().blue,
        TabState::Finished => colors().mint,
        TabState::Idle if active => colors().mint,
        _ => colors().subtle,
    }
}
fn tooltip(value: String, cx: &mut App) -> AnyView {
    cx.new(|_| TextTooltip(value)).into()
}
fn measure(text: &str, window: &Window) -> f32 {
    let run = TextRun {
        len: text.len(),
        font: font(".SystemUIFont"),
        color: rgb(colors().muted).into(),
        ..Default::default()
    };
    window
        .text_system()
        .shape_line(text.to_owned().into(), px(11.), &[run], None)
        .width
        .into()
}

impl Plait {
    pub(super) fn refresh_tab_rows(&mut self) {
        let rows = model::metadata(
            &self.tabs.model,
            &RowContext {
                host: &self.tabs.host,
                threads: &self.host.connection.threads,
                navigation: &self.preferences.value.navigation,
                machine: &self.active_project().machines[0].name,
            },
        );
        if rows != self.tabs.rows {
            self.tabs.rows = rows;
            self.tabs.measured = None;
        }
    }

    pub(super) fn on_tab_actions<E: InteractiveElement>(
        &self,
        root: E,
        cx: &mut Context<Self>,
    ) -> E {
        macro_rules! jumps { ($root:expr, $( $action:ty => $n:expr ),*) => {
            $root$(.on_action(cx.listener(|this, _: &$action, window, cx| {
                this.navigate_tab(TabNavigation::Jump($n), window, cx);
            })))*
        }; }
        let root = root
            .on_action(cx.listener(|this, _: &NewTab, window, cx| this.open_draft(window, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                if let Some(id) = this.tabs.model.active() {
                    this.close_tab(id, window, cx);
                } else {
                    this.close_desktop(false, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ReopenTab, window, cx| {
                this.navigate_tab(TabNavigation::Reopen, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                this.navigate_tab(TabNavigation::Next, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| {
                this.navigate_tab(TabNavigation::Previous, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveTabLeft, _, cx| this.move_active_tab(false, cx)))
            .on_action(cx.listener(|this, _: &MoveTabRight, _, cx| this.move_active_tab(true, cx)))
            .on_action(cx.listener(|this, _: &ListTabs, window, cx| {
                this.tabs.list_open = !this.tabs.list_open;
                window.focus(&this.focus, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.tabs.list_open {
                    this.tabs.list_open = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }));
        jumps!(root, TabJump1 => 1, TabJump2 => 2, TabJump3 => 3, TabJump4 => 4,
            TabJump5 => 5, TabJump6 => 6, TabJump7 => 7, TabJump8 => 8, TabJump9 => 9)
    }

    pub(super) fn tab_error(&mut self, result: Result<(), TabError>) -> bool {
        match result {
            Ok(()) => true,
            Err(TabError::IdentityMismatch) => {
                self.notice = Some("This checkout has changed identity. Close its existing tab, then open the thread again.".into());
                false
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                false
            }
        }
    }

    #[cfg(test)]
    pub(super) fn open_thread_tab(
        &mut self,
        id: String,
        checkout: PathBuf,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.save_tab_view(cx) {
            return false;
        }
        let target = ThreadTarget::Existing {
            id,
            checkout: self.tab_checkout(checkout),
        };
        let result = self.tabs.model.open(
            target,
            self.host
                .connection
                .connected
                .then_some(self.host.connection.epoch),
        );
        self.tab_error(result.map(|_| ()))
    }

    pub(super) fn tab_checkout(&self, root: PathBuf) -> CheckoutKey {
        CheckoutKey {
            host: self.tabs.host.clone(),
            root,
            directory: None,
        }
    }

    #[cfg(test)]
    fn save_tab_view(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.tabs.model.active() else {
            return true;
        };
        self.capture_session(cx);
        let bookmark = self.capture_tab_bookmark(None, cx);
        match plait_core::tabs::bookmark::BookmarkRecord::capture(&bookmark) {
            Ok(bookmark) => {
                let result = self.tabs.model.save_bookmark(id, bookmark);
                self.tab_error(result)
            }
            Err(_) => {
                self.notice =
                    Some("The tab could not save its view. Try again before closing it.".into());
                false
            }
        }
    }

    fn activate_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.request_tab(super::tab_lifecycle::Intent::Activate(id), window, cx);
    }

    pub(super) fn present_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.list_open = false;
        let target = self
            .tabs
            .model
            .active()
            .and_then(|id| self.tabs.model.get(id))
            .map(|tab| {
                (
                    tab.thread().target().clone(),
                    tab.thread().bookmark().view(),
                )
            });
        match target {
            Some((ThreadTarget::Existing { id, .. }, view)) => {
                self.select_host_thread_content(id, window, cx);
                self.show_tab(view, window, cx);
            }
            Some((ThreadTarget::Draft { checkout, .. }, view)) => {
                if checkout.root != self.host.creation.checkout {
                    self.change_checkout(checkout.root, window, cx);
                }
                self.open_draft_content(window, cx);
                self.show_tab(view, window, cx);
            }
            None => {
                self.workspace.reset_for_thread_selection();
                self.host.select(None);
                self.draft.open = false;
                self.refresh_live_list(None);
                window.focus(&self.focus, cx);
            }
        }
        self.hydrate_tab(window, cx);
        self.refresh_tab_rows();
        cx.notify();
    }

    fn close_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.request_tab(super::tab_lifecycle::Intent::Close(id), window, cx);
    }

    fn navigate_tab(&mut self, action: TabNavigation, window: &mut Window, cx: &mut Context<Self>) {
        self.request_tab(super::tab_lifecycle::Intent::Navigate(action), window, cx);
    }

    fn move_active_tab(&mut self, right: bool, cx: &mut Context<Self>) {
        let Some(id) = self.tabs.model.active() else {
            return;
        };
        let Some(index) = self.tabs.model.tabs().iter().position(|t| t.id() == id) else {
            return;
        };
        let to = if right {
            index + 1
        } else {
            index.saturating_sub(1)
        };
        let result = self.tabs.model.move_tab(id, to);
        self.tab_error(result);
        self.refresh_tab_rows();
        cx.notify();
    }

    /// Only width/metadata changes run layout; steady frames reuse its result.
    fn measure_tabs(&mut self, window: &Window) {
        let width = f32::from(window.viewport_size().width);
        let fullscreen = window.is_fullscreen();
        if self.tabs.measured == Some((width, fullscreen)) {
            return;
        }
        let available = width - if fullscreen { 8. } else { 84. } - 32. - 32. - 48.;
        let widths = |overflow| RowWidths {
            available,
            minimum_tab: 128.,
            maximum_tab: 240.,
            overflow,
        };
        let limit = if width > super::WIDE_WINDOW { 2 } else { 1 };
        let initial = model::layout(&self.tabs.rows, widths(44.), self.tabs.first_visible, limit);
        let overflow = (measure(&initial.overflow_label, window) + 16.)
            .clamp(44., if limit == 2 { 164. } else { 140. });
        let layout = model::layout(
            &self.tabs.rows,
            widths(overflow),
            self.tabs.first_visible,
            limit,
        );
        self.tabs.first_visible = layout.first_visible;
        self.tabs.layout = Some(layout);
        self.tabs.measured = Some((width, fullscreen));
    }

    pub(super) fn tab_row(&mut self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let redraw = self.tabs.strip_theme != colors().bg
            || self.tabs.measured
                != Some((
                    f32::from(window.viewport_size().width),
                    window.is_fullscreen(),
                ))
            || self.tabs.drag.is_some();
        self.measure_tabs(window);
        self.tabs.strip_theme = colors().bg;
        let layout = self.tabs.layout.as_ref().expect("measured tab row");
        let strip = self
            .tabs
            .strip
            .get_or_insert_with(|| {
                let owner = cx.weak_entity();
                cx.new(|_| TabStrip { owner })
            })
            .clone();
        if redraw {
            strip.update(cx, |_, cx| cx.notify());
        }
        let strip = strip.cached(
            StyleRefinement::default()
                .h_full()
                .w(px(layout.visible.len() as f32 * layout.tab_width)),
        );
        let bar = self
            .window_drag(row(), cx)
            .id("window-tab-bar")
            .h(px(CHROME_HEIGHT))
            .flex_shrink_0()
            .gap_0()
            .bg(rgb(colors().rail))
            .border_b_1()
            .border_color(rgb(colors().border))
            .pl(px(if window.is_fullscreen() { 8. } else { 84. }))
            .child(
                icon_button("toggle-sidebar", "panel", "Toggle sidebar")
                    .mr_1()
                    .aria_keyshortcuts(self.shortcut_aria("view.toggle_sidebar"))
                    .tooltip(|_, cx| tooltip("Toggle sidebar (⌘B)".into(), cx))
                    .on_activate(cx, |this, window, cx| this.toggle_sidebar(window, cx)),
            )
            .child(strip)
            .when(!layout.hidden.is_empty(), |bar| {
                let hover = layout.overflow_hover.clone();
                bar.child(
                    button("tab-overflow", layout.overflow_label.clone())
                        .max_w(px(if self.panels.viewport > super::WIDE_WINDOW {
                            164.
                        } else {
                            140.
                        }))
                        .overflow_hidden()
                        .aria_label("Show all tabs")
                        .tooltip(move |_, cx| tooltip(hover.clone(), cx))
                        .on_activate(cx, |this, window, cx| {
                            this.tabs.list_open = !this.tabs.list_open;
                            window.focus(&this.focus, cx);
                            cx.notify();
                        }),
                )
            })
            .child(
                icon_button("new-tab", "plus", "New thread tab")
                    .ml_1()
                    .tooltip(|_, cx| tooltip("New thread tab (⌘T)".into(), cx))
                    .on_activate(cx, |this, window, cx| this.open_draft(window, cx)),
            )
            .child(div().flex_1().min_w(px(48.)).h_full());
        bar.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
            let Some(drag) = this.tabs.drag.as_mut() else {
                return;
            };
            if event.pressed_button != Some(MouseButton::Left) {
                this.tabs.drag = None;
                return;
            }
            if (event.position.x - drag.start).abs() < px(4.) && drag.destination.is_none() {
                return;
            }
            this.window_drag = false;
            if let Some(layout) = &this.tabs.layout {
                let left = if window.is_fullscreen() { 40. } else { 116. };
                let index =
                    ((f32::from(event.position.x) - left) / layout.tab_width).max(0.) as usize;
                drag.destination = Some(
                    (layout.first_visible + index).min(this.tabs.rows.len().saturating_sub(1)),
                );
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if let Some(Drag {
                    id,
                    destination: Some(to),
                    ..
                }) = this.tabs.drag.take()
                {
                    this.tabs.measured = None;
                    let result = this.tabs.model.move_tab(id, to);
                    this.tab_error(result);
                    this.refresh_tab_rows();
                    cx.notify();
                }
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.tabs.drag = None;
                this.tabs.measured = None;
                cx.notify();
            }),
        )
    }

    fn tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = self.tabs.layout.as_ref().expect("measured strip");
        let mut strip = row()
            .id("tab-row")
            .role(Role::TabList)
            .aria_label("Open tabs")
            .gap_0()
            .min_w_0()
            .h_full()
            .items_end();
        for id in &layout.visible {
            if let Some(meta) = self.tabs.rows.iter().find(|t| t.id == *id) {
                strip = strip.child(self.tab_control(meta, layout.tab_width, cx));
            }
        }
        strip
    }

    fn tab_control(
        &self,
        meta: &TabMetadata,
        width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = meta.id;
        let group = format!("thread-tab-{}", id.get());
        let hover = meta.hover.clone();
        let close_hover = meta.close_hover;
        let close = meta.active;
        let attention = matches!(
            meta.state,
            TabState::Approve | TabState::Interrupted | TabState::Failed
        );
        let at = self.tabs.model.tabs().iter().position(|t| t.id() == id);
        let insertion = self
            .tabs
            .drag
            .as_ref()
            .is_some_and(|drag| drag.destination == at);
        row()
            .id(SharedString::from(group.clone()))
            .group(SharedString::from(group.clone()))
            .role(Role::Tab)
            .aria_label(format!("{} — {}", meta.title, meta.state.word(true)))
            .aria_selected(meta.active)
            .focusable()
            .tab_stop(true)
            .cursor_pointer()
            .w(px(width))
            .h(px(34.))
            .relative()
            .flex_shrink_0()
            .min_w_0()
            .gap(px(6.))
            .pl(px(8.))
            .pr(px(6.))
            .rounded_t(px(6.))
            .border_1()
            .border_color(if meta.active {
                rgb(colors().border).into()
            } else {
                transparent_black()
            })
            .bg(rgb(if meta.active {
                colors().bg
            } else {
                colors().rail
            }))
            .hover(|s| s.bg(rgba(colors().hover)))
            .focus_visible(|s| s.border_color(rgb(colors().blue)))
            .text_size(px(12.5))
            .font_weight(if meta.active || meta.unread || attention {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            })
            .tooltip(move |_, cx| tooltip(hover.clone(), cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, _| {
                    window.prevent_default();
                    this.window_drag = false;
                    this.tabs.drag = Some(Drag {
                        id,
                        start: event.position.x,
                        destination: None,
                    });
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.close_tab(id, window, cx);
                }),
            )
            .on_activate(cx, move |this, window, cx| {
                this.activate_tab(id, window, cx)
            })
            .when(attention, |el| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(6.))
                        .right(px(6.))
                        .h(px(1.))
                        .bg(rgb(tint(meta.state, meta.active))),
                )
            })
            .when(insertion, |el| {
                el.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(rgb(colors().blue)),
                )
            })
            .child(
                div()
                    .w(px(12.))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .child(dot(tint(meta.state, meta.active))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .child(meta.title.clone()),
            )
            .when(!meta.word.is_empty(), |el| {
                el.child(
                    tag(meta.word, tint(meta.state, meta.active))
                        .text_size(px(11.))
                        .flex_shrink_0(),
                )
            })
            .child(self.tab_close_control(id, close_hover, close, group, cx))
    }

    fn tab_close_control(
        &self,
        id: TabId,
        hover: &'static str,
        visible: bool,
        group: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        icon_button(("close-tab", id.get()), "close", "Close tab")
            .size(px(18.))
            .when(!visible, |el| {
                el.w_0()
                    .opacity(0.)
                    .group_hover(SharedString::from(group), |s| s.w(px(18.)).opacity(1.))
                    .focus_visible(|s| s.w(px(18.)).opacity(1.).border_color(rgb(colors().blue)))
            })
            .tooltip(move |_, cx| tooltip(hover.into(), cx))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_activate(cx, move |this, window, cx| {
                cx.stop_propagation();
                this.close_tab(id, window, cx);
            })
    }

    pub(super) fn tab_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.tabs.list_open {
            return None;
        }
        let layout = self.tabs.layout.as_ref()?;
        let mut list = column()
            .id("all-tabs-list")
            .role(Role::Menu)
            .aria_label("All open tabs")
            .absolute()
            .top(px(CHROME_HEIGHT))
            .right(px(48.))
            .w(px(320.))
            .max_h(px(336.))
            .overflow_y_scroll()
            .p_1()
            .bg(rgb(colors().panel))
            .border_1()
            .border_color(rgb(colors().border))
            .rounded_md()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.tabs.list_open = false;
                cx.notify();
            }));
        for group in &layout.groups {
            list = list.child(
                div()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .text_size(px(11.))
                    .text_color(rgb(colors().subtle))
                    .child(group.group.title()),
            );
            for id in &group.tabs {
                let Some(meta) = self.tabs.rows.iter().find(|t| t.id == *id) else {
                    continue;
                };
                let id = *id;
                let hover = meta.hover.clone();
                let group = format!("list-tab-{}", id.get());
                list = list.child(
                    nav_row(("tab-list", id.get()), meta.title.clone(), meta.active)
                        .role(Role::MenuItem)
                        .group(SharedString::from(group.clone()))
                        .text_size(px(12.))
                        .tooltip(move |_, cx| tooltip(hover.clone(), cx))
                        .child(dot(tint(meta.state, meta.active)))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_ellipsis()
                                .child(meta.title.clone()),
                        )
                        .child(
                            tag(meta.project.clone(), colors().subtle)
                                .max_w(px(70.))
                                .truncate(),
                        )
                        .child(
                            tag(meta.state.word(true), tint(meta.state, meta.active))
                                .text_size(px(11.)),
                        )
                        .when_some(meta.shortcut, |el, n| {
                            el.child(tag(format!("⌘{n}"), colors().subtle))
                        })
                        .child(self.tab_close_control(id, meta.close_hover, false, group, cx))
                        .on_activate(cx, move |this, window, cx| {
                            this.activate_tab(id, window, cx)
                        }),
                );
            }
        }
        Some(list.into_any_element())
    }
}

#[derive(Clone, Copy)]
pub(super) enum TabNavigation {
    Next,
    Previous,
    Jump(usize),
    Reopen,
}
