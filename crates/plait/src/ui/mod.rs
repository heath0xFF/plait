mod actions;
mod changes;
mod commands;
mod composer;
mod conversation;
mod host;
mod jump;
mod live_conversation;
mod markdown;
mod model_picker;
mod navigation;
mod navigation_host;
mod palette;
mod selection;
mod session;
mod settings;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod snapshot;
mod text_field;
pub mod theme;
mod tool_card;
mod update;

use gpui::{prelude::*, *};
use plait_core::{
    host_state::HostState,
    model::DiffMode,
    state::{AppState, Load},
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Instant,
};
use theme::*;

actions!(
    plait,
    [
        Quit,
        Close,
        Minimize,
        OpenRepository,
        Settings,
        CheckForUpdates,
        Refresh,
        ToggleSidebar,
        ToggleDiagnostics,
        Conversation,
        Changes,
        NextThread,
        PreviousThread,
        Tab,
        BackTab,
        NextFile,
        PreviousFile,
        NewThread,
        NewThreadWithModel,
        CommandPalette,
        ToggleInspector,
        RenameThread,
        ArchiveThread,
        ToggleArchived,
        FilterThreads,
        ToggleNotifications
    ]
);

/// Below this width the Git inspector is not shown; Git stays in Changes.
const WIDE_WINDOW: f32 = 1150.;

pub struct Plait {
    state: AppState,
    host: HostState,
    host_watch: Option<host::WatchLease>,
    navigation: navigation::Navigation,
    composers: HashMap<String, Entity<composer::Composer>>,
    live_scroll: ListState,
    live_count: usize,
    live_layout: Vec<plait_core::transcript::Slot>,
    formatted: std::sync::Arc<plait_core::markdown::Transcript>,
    settings_open: bool,
    /// A new thread was requested: open it when the host confirms creation,
    /// unless the user has gone elsewhere in the meantime.
    new_thread_open: bool,
    /// The command palette or model picker, above the workspace.
    overlay: Option<commands::Overlay>,
    /// The inspector toggle was used on a window too narrow to show it.
    inspector_note: bool,
    focus: FocusHandle,
    expanded_live_tools: HashSet<String>,
    reading: selection::Reading,
    file_scroll: UniformListScrollHandle,
    diff_scroll: UniformListScrollHandle,
    show_changes: bool,
    sidebar: bool,
    diagnostics: bool,
    preferences: jump::PreferenceState,
    updates: update::UpdateState,
    session: session::SessionState,
    picker_open: bool,
    notice: Option<String>,
    last_render_us: u128,
    max_render_us: u128,
    startup: Instant,
    first_render_ms: Option<u128>,
    interaction: Option<Instant>,
    switch_us: u128,
}

impl Plait {
    pub fn new(
        path: PathBuf,
        startup: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let host = HostState::new(path.clone());
        let state = AppState::new(path);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            state,
            host,
            host_watch: None,
            navigation: navigation::Navigation::new(cx),
            composers: HashMap::new(),
            live_scroll: ListState::new(0, ListAlignment::Bottom, px(300.)),
            live_count: 0,
            live_layout: Vec::new(),
            formatted: Default::default(),
            settings_open: false,
            new_thread_open: false,
            overlay: None,
            inspector_note: false,
            focus,
            expanded_live_tools: HashSet::new(),
            reading: Default::default(),
            file_scroll: UniformListScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
            show_changes: false,
            sidebar: true,
            diagnostics: false,
            preferences: jump::PreferenceState::default(),
            updates: update::UpdateState::default(),
            session: session::SessionState::default(),
            picker_open: false,
            notice: None,
            last_render_us: 0,
            max_render_us: 0,
            startup,
            first_render_ms: None,
            interaction: None,
            switch_us: 0,
        };
        this.initialize_navigation(window, cx);
        this.pump(cx);
        this.load_preferences(cx);
        // An open picker shows host state; keep it current as replies land.
        cx.observe_self(|this, cx| this.sync_picker(cx)).detach();
        this.reconnect_host(window, cx);
        // Test scenes stay offline.
        if !cfg!(test) {
            this.check_for_updates(true, cx);
        }
        this
    }

    fn pump(&mut self, cx: &mut Context<Self>) {
        let Some(job) = self.state.repository.next_job() else {
            return;
        };
        let task = cx.background_executor().spawn(async move {
            let reply = job.run();
            (job, reply)
        });
        cx.spawn(async move |this, cx| {
            let (job, reply) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.state.repository.complete(job, reply);
                this.normalize_checkout(cx);
                this.deliver_files(cx);
                this.pump(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Move to the next or previous host thread, wrapping. With no selection,
    /// forward starts at the first thread and backward at the last.
    fn step_thread(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let indices = self.filtered_threads(cx);
        let count = indices.len();
        if count == 0 {
            return;
        }
        let current = self.host.selected.as_ref().and_then(|id| {
            indices
                .iter()
                .position(|&index| &self.host.threads[index].id == id)
        });
        let next = match (current, forward) {
            (Some(index), true) => (index + 1) % count,
            (Some(index), false) => (index + count - 1) % count,
            (None, true) => 0,
            (None, false) => count - 1,
        };
        self.interaction = Some(std::time::Instant::now());
        let id = self.host.threads[indices[next]].id.clone();
        self.select_host_thread(id, window, cx);
    }

    fn select_file(
        &mut self,
        file: usize,
        mode: DiffMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.interaction = Some(Instant::now());
        self.state.repository.select(file, mode);
        self.diff_scroll = UniformListScrollHandle::new();
        self.show_changes = true;
        self.pump(cx);
        cx.notify();
    }

    fn step_file(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let repo = &self.state.repository;
        let Some(snapshot) = repo.snapshot.ready() else {
            return;
        };
        let count = snapshot.changes.len();
        if count == 0 {
            return;
        }
        let current = repo.selected.as_ref().map_or(0, |s| s.file);
        let next = if forward {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        let mode = if snapshot.changes[next].unstaged() {
            DiffMode::Unstaged
        } else {
            DiffMode::Staged
        };
        self.file_scroll.scroll_to_item(next, ScrollStrategy::Top);
        self.select_file(next, mode, window, cx);
    }

    fn refresh(&mut self, _: &Refresh, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if !matches!(self.state.repository.snapshot, Load::Loading) {
            self.state.repository.refresh();
            self.pump(cx);
            cx.notify();
        }
    }

    fn open_repository(&mut self, _: &OpenRepository, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if self.picker_open {
            return;
        }
        if self.host.pending_create.is_some() {
            self.notice =
                Some("Resolve the pending thread creation before changing checkout.".into());
            cx.notify();
            return;
        }
        self.picker_open = true;
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open repository (read-only)".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = prompt.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.picker_open = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            this.change_checkout(path, window, cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        this.notice = Some(format!("Cannot open folder picker: {error}"))
                    }
                    Err(error) => {
                        this.notice = Some(format!("Folder picker closed unexpectedly: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Plait {
    fn show_settings(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.close_overlay(false, window, cx);
        window.focus(&self.focus, cx);
        self.settings_open = open;
        if !open
            && !self.show_changes
            && let Some(id) = self.host.selected.clone()
        {
            self.mark_thread_read(&id, cx);
        }
        self.new_thread_open = false;
        cx.notify();
    }

    fn wide(window: &Window) -> bool {
        window.viewport_size().width > px(WIDE_WINDOW)
    }

    /// The Git inspector beside the conversation: wide windows, unless hidden.
    pub(super) fn shows_inspector(&self, wide: bool) -> bool {
        wide && !self.preferences.value.hide_inspector
    }

    /// Show or hide the Git inspector and remember the choice. A narrow window
    /// has no room for it, so the toggle says where Git is instead.
    fn toggle_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !Self::wide(window) {
            self.inspector_note = !self.inspector_note;
            cx.notify();
            return;
        }
        self.inspector_note = false;
        let mut value = self.preferences.value.clone();
        value.hide_inspector = !value.hide_inspector;
        self.set_preferences(value, jump::PreferenceGroup::View, cx);
    }

    fn inspector_note(&self, cx: &mut Context<Self>) -> impl IntoElement {
        row()
            .id("inspector-note")
            .role(Role::Group)
            .aria_label("Git inspector unavailable at this window width")
            .flex_shrink_0()
            .px_4()
            .py_2()
            .gap_3()
            .border_b_1()
            .border_color(rgb(BORDER))
            .text_size(px(12.))
            .child(div().flex_1().min_w_0().text_color(rgb(MUTED)).child(text!(
                format!(
                    "The Git inspector needs a window wider than {WIDE_WINDOW} points. Git status and diffs are in Changes."
                )
            )))
            .child(
                toned_button("inspector-note-changes", "Show Changes", Tone::Filled).on_activate(
                    cx,
                    |this, _, cx| {
                        this.inspector_note = false;
                        this.show_tab(true, cx);
                    },
                ),
            )
            .child(
                button("inspector-note-dismiss", "Dismiss").on_activate(cx, |this, _, cx| {
                    this.inspector_note = false;
                    cx.notify();
                }),
            )
    }

    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Keep the shortcut context alive when its focused child is removed.
        window.focus(&self.focus, cx);
        self.sidebar = !self.sidebar;
        cx.notify();
    }

    fn show_tab(&mut self, changes: bool, cx: &mut Context<Self>) {
        self.show_changes = changes;
        if !changes
            && !self.settings_open
            && let Some(id) = self.host.selected.clone()
        {
            self.mark_thread_read(&id, cx);
        }
        cx.notify();
    }

    /// One compact chrome row: where you are, its state, and the two workspaces.
    fn header(
        &self,
        title: String,
        location: String,
        status: (String, u32),
        wide: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let changed = self
            .state
            .repository
            .snapshot
            .ready()
            .map(|snapshot| snapshot.changes.len())
            .filter(|count| *count > 0);
        row()
            .h(px(CHROME_HEIGHT))
            .flex_shrink_0()
            .pl_2()
            .pr_3()
            .gap_3()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                icon_button("toggle-sidebar", "panel", "Toggle sidebar")
                    .aria_keyshortcuts("Meta+B")
                    .on_activate(cx, |this, window, cx| this.toggle_sidebar(window, cx)),
            )
            .child(
                row()
                    .flex_1()
                    .min_w_0()
                    .gap_3()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(text!(title)),
                    )
                    // The composer repeats this context; the header keeps it for Changes.
                    .when(wide, |el| {
                        el.child(tag(location, SUBTLE).flex_shrink_0().whitespace_nowrap())
                    }),
            )
            .child(
                row()
                    .flex_shrink_0()
                    .gap(px(6.))
                    .child(dot(status.1))
                    .child(tag(status.0, MUTED)),
            )
            .child(
                segmented()
                    .child(
                        segment("conversation-tab", "Conversation", !self.show_changes)
                            .aria_keyshortcuts("Meta+Shift+1")
                            .on_click(cx.listener(|this, _, _, cx| this.show_tab(false, cx))),
                    )
                    .child(
                        segment("changes-tab", "Changes", self.show_changes)
                            .aria_keyshortcuts("Meta+Shift+2")
                            // The accessible name stays "Changes"; the count is visual.
                            .when_some(changed, |el, count| {
                                el.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(SUBTLE))
                                        .child(count.to_string()),
                                )
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.show_tab(true, cx))),
                    ),
            )
            .child(
                // Mirrors the sidebar toggle at the other end of the header.
                icon_button(
                    "toggle-inspector",
                    "panel-right",
                    if wide {
                        "Toggle Git inspector"
                    } else {
                        "Git inspector (needs a wider window)"
                    },
                )
                .aria_keyshortcuts("Meta+Alt+B")
                .on_activate(cx, |this, window, cx| this.toggle_inspector(window, cx)),
            )
    }

    /// Where the selected thread runs: Project, Machine, and its checkout branch.
    pub(super) fn environment(
        &self,
        model: Option<String>,
        note: Option<(&'static str, u32)>,
    ) -> impl IntoElement {
        // Host threads belong to this machine's checkout.
        let project = self.active_project();
        let machine = &project.machines[0];
        let branch = self
            .state
            .repository
            .snapshot
            .ready()
            .map(|snapshot| snapshot.branch.clone());
        let chip = |id: &'static str, glyph: &'static str, label: String| {
            row()
                .gap(px(5.))
                .min_w_0()
                .child(icon(glyph).size(px(12.)).text_color(rgb(SUBTLE)))
                .child(div().truncate().child(text!(id = id, label)))
        };
        row()
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .mx_auto()
            .px_1()
            .gap_4()
            .text_size(px(11.))
            .text_color(rgb(MUTED))
            .child(chip("environment-project", "folder", project.name.clone()))
            .child(chip("environment-machine", "machine", machine.name.clone()))
            .when_some(branch, |el, branch| {
                el.child(chip("environment-branch", "branch", branch))
            })
            .when_some(model, |el, model| {
                el.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(rgb(SUBTLE))
                        .child(text!(id = "environment-model", model)),
                )
            })
            .child(div().flex_1())
            .when_some(note, |el, (note, tint)| {
                el.child(tag(note, tint).flex_shrink_0().whitespace_nowrap())
            })
    }

    fn diagnostics_bar(&self) -> impl IntoElement {
        row()
            .h(px(24.))
            .px_3()
            .flex_shrink_0()
            .justify_end()
            .bg(rgb(RAIL))
            .border_t_1()
            .border_color(rgb(BORDER))
            .text_size(px(11.))
            .text_color(rgb(SUBTLE))
            .child(text!(format!(
                "UI {} µs · peak {} µs · switch {} µs · first {} ms",
                self.last_render_us,
                self.max_render_us,
                self.switch_us,
                self.first_render_ms.unwrap_or(0)
            )))
    }
}

impl Render for Plait {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let start = Instant::now();
        self.first_render_ms
            .get_or_insert_with(|| self.startup.elapsed().as_millis());
        if let Some(interaction) = self.interaction.take() {
            self.switch_us = interaction.elapsed().as_micros();
        }
        let wide = Self::wide(window);
        let project = self.active_project();
        let machine = &project.machines[0];
        let real = self.host.selected.is_some();
        let connection = if self.host.connecting {
            ("Connecting…".to_owned(), MUTED)
        } else if self.host.connected {
            ("Host connected".to_owned(), MINT)
        } else {
            ("Host disconnected".to_owned(), AMBER)
        };
        let title = if self.settings_open {
            "Settings".to_owned()
        } else if real {
            self.thread_title(self.host.selected.as_deref().unwrap_or_default())
        } else {
            project.name.clone()
        };
        let status = if real && !self.settings_open {
            self.host.snapshot.as_ref().map_or_else(
                || ("Loading".to_owned(), MUTED),
                |snapshot| {
                    (
                        snapshot.status.label().to_owned(),
                        host_status_color(snapshot.status),
                    )
                },
            )
        } else {
            connection
        };
        // With no thread open the title is already the project; don't repeat it.
        let location = if real || self.settings_open {
            format!("{} / {}", project.name, machine.name)
        } else {
            machine.name.clone()
        };
        let root = column()
            .id("plait")
            .role(Role::Application)
            .aria_label("Plait")
            .key_context("Plait")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open_repository))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(|this, _: &Settings, window, cx| {
                this.show_settings(!this.settings_open, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CheckForUpdates, window, cx| {
                this.show_settings(true, window, cx);
                this.check_for_updates(false, cx);
            }))
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleInspector, window, cx| {
                this.toggle_inspector(window, cx)
            }))
            .on_action(cx.listener(|this, _: &RenameThread, window, cx| {
                if let Some(id) = this.host.selected.clone() {
                    this.begin_rename(id, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ArchiveThread, window, cx| {
                if let Some(thread) = this
                    .host
                    .threads
                    .iter()
                    .find(|thread| Some(&thread.id) == this.host.selected.as_ref())
                {
                    this.archive_thread(
                        thread.id.clone(),
                        thread.archived != Some(true),
                        window,
                        cx,
                    );
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleArchived, _, cx| {
                this.navigation.archived = !this.navigation.archived;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FilterThreads, window, cx| {
                this.sidebar = true;
                window.focus(&this.navigation.filter.focus_handle(cx), cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleNotifications, _, cx| {
                let mut value = this.preferences.value.clone();
                value.navigation.mute_notifications = !value.navigation.mute_notifications;
                this.set_preferences(value, jump::PreferenceGroup::View, cx);
            }))
            .on_action(cx.listener(|this, _: &CommandPalette, window, cx| {
                this.toggle_command_palette(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &NewThread, window, cx| this.new_thread(None, window, cx)),
            )
            .on_action(cx.listener(|this, _: &NewThreadWithModel, window, cx| {
                this.open_model_picker(model_picker::ModelPurpose::NewThread, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleDiagnostics, _, cx| {
                this.diagnostics = !this.diagnostics;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Conversation, _, cx| this.show_tab(false, cx)))
            .on_action(cx.listener(|this, _: &Changes, _, cx| this.show_tab(true, cx)))
            .on_action(
                cx.listener(|this, _: &NextThread, window, cx| this.step_thread(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &PreviousThread, window, cx| {
                this.step_thread(false, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &NextFile, window, cx| this.step_file(true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PreviousFile, window, cx| this.step_file(false, window, cx)),
            )
            .on_action(cx.listener(|_, _: &Tab, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &BackTab, window, cx| window.focus_prev(cx)))
            .on_action(
                cx.listener(|this, _: &Close, window, cx| this.close_desktop(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Quit, window, cx| this.close_desktop(true, window, cx)),
            )
            .on_action(cx.listener(|_, _: &Minimize, window, _| window.minimize_window()))
            .map(|root| self.on_jump_actions(root, cx))
            .map(|root| self.on_selection_actions(root, cx))
            .size_full()
            .overflow_hidden()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .text_size(px(13.))
            .font_family(".SystemUIFont")
            .child(
                row()
                    .gap_0()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .when(self.sidebar, |el| el.child(self.navigation(wide, cx)))
                    .child(
                        column()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(self.header(title, location, status, wide, cx))
                            .when_some(self.session.note.clone(), |el, note| {
                                el.child(
                                    div()
                                        .px_4()
                                        .py_2()
                                        .text_size(px(12.))
                                        .text_color(rgb(AMBER))
                                        .child(note),
                                )
                            })
                            .when_some(self.notice.clone(), |el, notice| {
                                el.child(
                                    div()
                                        .px_4()
                                        .py_2()
                                        .text_size(px(12.))
                                        .text_color(rgb(RED))
                                        .child(notice),
                                )
                            })
                            .when_some(self.creation_bar(cx), |el, bar| el.child(bar))
                            .when(self.inspector_note && !wide, |el| {
                                el.child(self.inspector_note(cx))
                            })
                            .child(if self.settings_open {
                                self.settings(cx).into_any_element()
                            } else if self.show_changes {
                                self.changes(wide, cx).into_any_element()
                            } else if real {
                                self.live_conversation(wide, cx).into_any_element()
                            } else {
                                self.idle(wide, cx).into_any_element()
                            }),
                    ),
            )
            // Timings stay available without spending a permanent chrome row on them.
            .when(self.diagnostics, |el| el.child(self.diagnostics_bar()))
            // Last, so the overlay paints above and blocks the workspace.
            .when_some(self.overlay.as_ref(), |el, overlay| {
                el.child(overlay.palette.clone())
            });
        self.last_render_us = start.elapsed().as_micros();
        self.max_render_us = self.max_render_us.max(self.last_render_us);
        root
    }
}

/// Bind the default shortcuts and build the menu bar from the action table.
pub fn menus(cx: &mut App) {
    actions::install(cx, &actions::KeyOverrides::new());
    cx.on_action(|_: &Quit, cx| cx.quit());
}
