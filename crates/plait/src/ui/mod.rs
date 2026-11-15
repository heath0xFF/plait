mod changes;
mod conversation;
mod navigation;
pub mod theme;

use gpui::{prelude::*, *};
use plait::{
    model::{DiffMode, Selection},
    state::{AppState, Load},
};
use std::{collections::HashSet, path::PathBuf, time::Instant};
use theme::*;

actions!(
    plait,
    [
        Quit,
        Close,
        Minimize,
        OpenRepository,
        Refresh,
        ToggleSidebar,
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
    focus: FocusHandle,
    conversation_scroll: ListState,
    expanded_tools: HashSet<usize>,
    file_scroll: UniformListScrollHandle,
    diff_scroll: UniformListScrollHandle,
    show_changes: bool,
    sidebar: bool,
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
        let state = AppState::new(path);
        let count = state.projects[0].machines[0].threads[0].events.len();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            state,
            focus,
            conversation_scroll: ListState::new(count, ListAlignment::Top, px(300.)),
            expanded_tools: HashSet::new(),
            file_scroll: UniformListScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
            show_changes: false,
            sidebar: true,
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

    fn select_thread(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.interaction = Some(Instant::now());
        self.state.select(selection);
        let s = self.state.selection;
        let count = self.state.projects[s.project].machines[s.machine].threads[s.thread]
            .events
            .len();
        self.expanded_tools.clear();
        self.conversation_scroll.reset(count);
        cx.notify();
    }

    fn step_thread(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.state.step_thread(forward);
        self.select_thread(self.state.selection, cx);
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
        if self.state.local() && !matches!(self.state.repository.snapshot, Load::Loading) {
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
        self.picker_open = true;
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open repository (read-only)".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = prompt.await;
            let _ = this.update(cx, |this, cx| {
                this.picker_open = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            this.state.projects[0].name = path
                                .file_name()
                                .unwrap_or(path.as_os_str())
                                .to_string_lossy()
                                .into_owned();
                            this.state.repository.open(path);
                            this.select_thread(
                                Selection {
                                    project: 0,
                                    machine: 0,
                                    thread: 0,
                                },
                                cx,
                            );
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

impl Render for Plait {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let start = Instant::now();
        self.first_render_ms
            .get_or_insert_with(|| self.startup.elapsed().as_millis());
        if let Some(interaction) = self.interaction.take() {
            self.switch_us = interaction.elapsed().as_micros();
        }
        let wide = window.viewport_size().width > px(1150.);
        let selection = self.state.selection;
        let project = &self.state.projects[selection.project];
        let machine = &project.machines[selection.machine];
        let thread = &machine.threads[selection.thread];
        let title = thread.title;
        let status = thread.status;
        let location = format!("{}  /  {}", project.name, machine.name);
        let local = machine.local;
        let root = column()
            .id("plait")
            .role(Role::Application)
            .aria_label("Plait")
            .key_context("Plait")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open_repository))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| {
                // Keep the shortcut context alive when its focused child is removed.
                window.focus(&this.focus, cx);
                this.sidebar = !this.sidebar;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Conversation, _, cx| {
                this.show_changes = false;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Changes, _, cx| {
                this.show_changes = true;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextThread, _, cx| this.step_thread(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousThread, _, cx| this.step_thread(false, cx)))
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
                            .child(
                                row()
                                    .h(px(64.))
                                    .flex_shrink_0()
                                    .px_6()
                                    .justify_between()
                                    .border_b_1()
                                    .border_color(rgb(BORDER))
                                    .child(
                                        column()
                                            .min_w_0()
                                            .gap_1()
                                            .child(tag(location, MUTED))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(19.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(text!(title)),
                                            ),
                                    )
                                    .child(
                                        row()
                                            .flex_shrink_0()
                                            .child(dot(status_color(status)))
                                            .child(tag(
                                                format!("{} · mock", status.label()),
                                                status_color(status),
                                            )),
                                    ),
                            )
                            .child(
                                row()
                                    .px_5()
                                    .h(px(44.))
                                    .flex_shrink_0()
                                    .border_b_1()
                                    .border_color(rgb(BORDER))
                                    .justify_between()
                                    .child(
                                        row()
                                            .child(
                                                button("conversation-tab", "Conversation")
                                                    .role(Role::Tab)
                                                    .aria_selected(!self.show_changes)
                                                    .when(!self.show_changes, |el| {
                                                        el.bg(rgb(PANEL)).text_color(rgb(TEXT))
                                                    })
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.show_changes = false;
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                button("changes-tab", "Changes")
                                                    .role(Role::Tab)
                                                    .aria_selected(self.show_changes)
                                                    .when(self.show_changes, |el| {
                                                        el.bg(rgb(PANEL)).text_color(rgb(MINT))
                                                    })
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.show_changes = true;
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                    .child(if local {
                                        tag("READ-ONLY", MINT)
                                    } else {
                                        tag("REMOTE FIXTURE · NOT CONNECTED", VIOLET)
                                    }),
                            )
                            .when_some(self.notice.clone(), |el, notice| {
                                el.child(div().px_5().py_2().text_color(rgb(RED)).child(notice))
                            })
                            .child(if self.show_changes {
                                self.changes(cx).into_any_element()
                            } else {
                                self.conversation(wide, cx).into_any_element()
                            }),
                    ),
            )
            .child(
                row()
                    .h(px(28.))
                    .px_4()
                    .flex_shrink_0()
                    .justify_between()
                    .bg(rgb(RAIL))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(if local {
                        "Zaphod · local repository"
                    } else {
                        "Remote sample · no connection or execution"
                    })
                    .child(text!(format!(
                        "UI {} µs · peak {} µs · switch {} µs · first {} ms",
                        self.last_render_us,
                        self.max_render_us,
                        self.switch_us,
                        self.first_render_ms.unwrap_or(0)
                    ))),
            );
        self.last_render_us = start.elapsed().as_micros();
        self.max_render_us = self.max_render_us.max(self.last_render_us);
        root
    }
}

pub fn menus(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-w", Close, Some("Plait")),
        KeyBinding::new("cmd-m", Minimize, Some("Plait")),
        KeyBinding::new("cmd-o", OpenRepository, Some("Plait")),
        KeyBinding::new("cmd-r", Refresh, Some("Plait")),
        KeyBinding::new("cmd-b", ToggleSidebar, Some("Plait")),
        KeyBinding::new("cmd-1", Conversation, Some("Plait")),
        KeyBinding::new("cmd-2", Changes, Some("Plait")),
        KeyBinding::new("cmd-alt-down", NextThread, Some("Plait")),
        KeyBinding::new("cmd-alt-up", PreviousThread, Some("Plait")),
        KeyBinding::new("alt-down", NextFile, Some("Plait")),
        KeyBinding::new("alt-up", PreviousFile, Some("Plait")),
        KeyBinding::new("tab", Tab, Some("Plait")),
        KeyBinding::new("shift-tab", BackTab, Some("Plait")),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus([
        Menu::new("Plait").items([MenuItem::action("Quit Plait", Quit)]),
        Menu::new("File").items([
            MenuItem::action("Open Repository…", OpenRepository),
            MenuItem::action("Refresh Git Status", Refresh),
            MenuItem::separator(),
            MenuItem::action("Close Window", Close),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
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
