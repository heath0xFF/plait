mod changes;
mod composer;
mod conversation;
mod host;
mod jump;
mod live_conversation;
mod navigation;
mod settings;
#[cfg(all(test, target_os = "linux"))]
mod snapshot;
pub mod theme;

use gpui::{prelude::*, *};
use plait_core::{
    host_state::HostState,
    model::DiffMode,
    state::{AppState, Load, project_name},
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
        PreviousFile
    ]
);

pub struct Plait {
    state: AppState,
    host: HostState,
    host_watch: Option<host::WatchLease>,
    composers: HashMap<String, Entity<composer::Composer>>,
    live_scroll: ListState,
    live_count: usize,
    settings_open: bool,
    new_thread_open: bool,
    focus: FocusHandle,
    expanded_live_tools: HashSet<String>,
    file_scroll: UniformListScrollHandle,
    diff_scroll: UniformListScrollHandle,
    show_changes: bool,
    sidebar: bool,
    diagnostics: bool,
    preferences: jump::PreferenceState,
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
            composers: HashMap::new(),
            live_scroll: ListState::new(0, ListAlignment::Bottom, px(300.)),
            live_count: 0,
            settings_open: false,
            new_thread_open: false,
            focus,
            expanded_live_tools: HashSet::new(),
            file_scroll: UniformListScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
            show_changes: false,
            sidebar: true,
            diagnostics: false,
            preferences: jump::PreferenceState::default(),
            picker_open: false,
            notice: None,
            last_render_us: 0,
            max_render_us: 0,
            startup,
            first_render_ms: None,
            interaction: None,
            switch_us: 0,
        };
        this.pump(cx);
        this.load_preferences(cx);
        this.reconnect_host(window, cx);
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
                if let Some(snapshot) = this.state.repository.snapshot.ready()
                    && let Some(name) = snapshot.root.file_name()
                {
                    this.state.projects[0].name = name.to_string_lossy().into_owned();
                }
                this.pump(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Move to the next or previous host thread, wrapping. With no selection,
    /// forward starts at the first thread and backward at the last.
    fn step_thread(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.host.threads.len();
        if count == 0 {
            return;
        }
        let current = self
            .host
            .selected
            .as_ref()
            .and_then(|id| self.host.threads.iter().position(|thread| &thread.id == id));
        let next = match (current, forward) {
            (Some(index), true) => (index + 1) % count,
            (Some(index), false) => (index + count - 1) % count,
            (None, true) => 0,
            (None, false) => count - 1,
        };
        self.interaction = Some(std::time::Instant::now());
        let id = self.host.threads[next].id.clone();
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
                            this.state.projects[0].name = project_name(&path);
                            let epoch = this.host.epoch + 1;
                            let submissions = std::mem::take(&mut this.host.submissions);
                            this.host = HostState::new(path.clone());
                            this.host.epoch = epoch;
                            this.host.submissions = submissions;
                            this.reconnect_host(window, cx);
                            this.state.repository.open(path);
                            // The new checkout's threads come from the reconnect.
                            this.host_watch = None;
                            this.settings_open = false;
                            this.new_thread_open = false;
                            this.file_scroll = UniformListScrollHandle::new();
                            this.notice = None;
                            this.pump(cx);
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
        window.focus(&self.focus, cx);
        self.settings_open = open;
        self.new_thread_open = false;
        cx.notify();
    }

    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Keep the shortcut context alive when its focused child is removed.
        window.focus(&self.focus, cx);
        self.sidebar = !self.sidebar;
        cx.notify();
    }

    fn show_tab(&mut self, changes: bool, cx: &mut Context<Self>) {
        self.show_changes = changes;
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
    }

    /// Where the selected thread runs: Project, Machine, and its checkout branch.
    pub(super) fn environment(
        &self,
        model: Option<String>,
        note: Option<(&'static str, u32)>,
    ) -> impl IntoElement {
        // Host threads belong to this machine's checkout.
        let project = &self.state.projects[0];
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
        let wide = window.viewport_size().width > px(1150.);
        let project = &self.state.projects[0];
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
        } else if self.new_thread_open {
            "New local thread".to_owned()
        } else if real {
            self.host.selected.clone().unwrap_or_default()
        } else {
            project.name.clone()
        };
        let status = if real && !self.settings_open && !self.new_thread_open {
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
        let location = if real || self.settings_open || self.new_thread_open {
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
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)),
            )
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
            .on_action(cx.listener(|_, _: &Close, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &Minimize, window, _| window.minimize_window()))
            .map(|root| self.on_jump_actions(root, cx))
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
                            .child(if self.settings_open {
                                self.settings(cx).into_any_element()
                            } else if self.new_thread_open {
                                self.new_thread(cx).into_any_element()
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
            .when(self.diagnostics, |el| el.child(self.diagnostics_bar()));
        self.last_render_us = start.elapsed().as_micros();
        self.max_render_us = self.max_render_us.max(self.last_render_us);
        root
    }
}

pub fn menus(cx: &mut App) {
    composer::bind_keys(cx);
    jump::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-w", Close, Some("Plait")),
        KeyBinding::new("cmd-m", Minimize, Some("Plait")),
        KeyBinding::new("cmd-o", OpenRepository, Some("Plait")),
        KeyBinding::new("cmd-,", Settings, Some("Plait")),
        KeyBinding::new("cmd-r", Refresh, Some("Plait")),
        KeyBinding::new("cmd-b", ToggleSidebar, Some("Plait")),
        KeyBinding::new("cmd-shift-d", ToggleDiagnostics, Some("Plait")),
        // Cmd-1 to Cmd-9 jump to sidebar items; see jump.rs.
        KeyBinding::new("cmd-shift-1", Conversation, Some("Plait")),
        KeyBinding::new("cmd-shift-2", Changes, Some("Plait")),
        KeyBinding::new("cmd-alt-down", NextThread, Some("Plait")),
        KeyBinding::new("cmd-alt-up", PreviousThread, Some("Plait")),
        KeyBinding::new("alt-down", NextFile, Some("Plait")),
        KeyBinding::new("alt-up", PreviousFile, Some("Plait")),
        KeyBinding::new("tab", Tab, Some("Plait")),
        KeyBinding::new("shift-tab", BackTab, Some("Plait")),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus([
        Menu::new("Plait").items([
            MenuItem::action("Settings…", Settings),
            MenuItem::separator(),
            MenuItem::action("Quit Plait", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Open Repository…", OpenRepository),
            MenuItem::action("Refresh Git Status", Refresh),
            MenuItem::separator(),
            MenuItem::action("Close Window", Close),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Diagnostics", ToggleDiagnostics),
            MenuItem::action("Conversation", Conversation),
            MenuItem::action("Changes", Changes),
            MenuItem::separator(),
            MenuItem::action("Next Thread", NextThread),
            MenuItem::action("Previous Thread", PreviousThread),
            MenuItem::action("Next Changed File", NextFile),
            MenuItem::action("Previous Changed File", PreviousFile),
        ]),
        Menu::new("Window").items([MenuItem::action("Minimize", Minimize)]),
    ]);
}
