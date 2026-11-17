use super::{
    OpenRepository, Plait, host::WatchLease, jump::PreferenceGroup, model_picker::ModelPurpose,
    text_field::TextField, theme::*,
};
use gpui::{prelude::*, *};
use plait_core::{
    host::{Request, ThreadReply, ThreadSummary},
    navigation as nav,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) struct Navigation {
    pub filter: Entity<TextField>,
    pub scroll: UniformListScrollHandle,
    pub collapsed: HashSet<PathBuf>,
    pub available: HashMap<PathBuf, bool>,
    pub archived: bool,
    pub attention: bool,
    pub rename: Option<(String, Entity<TextField>)>,
    pub menu: Option<String>,
    pub managing: Option<ThreadManagement>,
    management_generation: u64,
    pub error: Option<String>,
    pub loading: bool,
    pub generation: u64,
    pub watch: Option<WatchLease>,
}

impl Navigation {
    pub fn new(cx: &mut Context<Plait>) -> Self {
        Self {
            scroll: UniformListScrollHandle::new(),
            filter: cx.new(|cx| {
                TextField::new("Filter projects and threads", "Find a thread…", cx).search()
            }),
            collapsed: HashSet::new(),
            available: HashMap::new(),
            archived: false,
            attention: false,
            rename: None,
            menu: None,
            managing: None,
            management_generation: 0,
            error: None,
            loading: false,
            generation: 0,
            watch: None,
        }
    }
}

#[derive(Clone)]
pub(super) struct ThreadManagement {
    epoch: u64,
    generation: u64,
    id: String,
    rename: Option<(Entity<TextField>, String)>,
}

#[derive(Clone, Copy)]
enum Row {
    Project(usize),
    Thread(usize),
}

/// One-line title that gives way to the trailing state.
fn title(text: impl Into<SharedString>) -> Div {
    div().flex_1().min_w_0().truncate().child(text.into())
}

impl Plait {
    pub(super) fn active_project(&self) -> &plait_core::model::Project {
        self.state
            .projects
            .iter()
            .find(|project| project.checkout == self.project_of(&self.host.checkout))
            .unwrap_or(&self.state.projects[0])
    }

    /// The project a checkout lists under; a Plait worktree belongs to its repository.
    pub(super) fn project_of<'a>(&'a self, checkout: &'a std::path::Path) -> &'a std::path::Path {
        self.preferences.value.navigation.project_of(checkout)
    }

    pub(super) fn filtered_threads(&self, cx: &App) -> Vec<usize> {
        let query = self.navigation.filter.read(cx).text();
        let projects = self
            .state
            .projects
            .iter()
            .enumerate()
            .map(|(index, project)| (&project.checkout, (index, project)))
            .collect::<HashMap<_, _>>();
        let mut matches = self
            .host
            .threads
            .iter()
            .enumerate()
            .filter_map(|(index, thread)| {
                let checkout = self.project_of(&thread.checkout).to_path_buf();
                let (project_index, project) = projects.get(&checkout)?;
                nav::matches(
                    thread,
                    &self.summary_title(thread),
                    project,
                    query,
                    self.navigation.archived,
                    self.navigation.attention,
                    &self.preferences.value.navigation.unread,
                )
                .then_some((*project_index, index))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(project, _)| *project);
        matches.into_iter().map(|(_, thread)| thread).collect()
    }

    fn navigation_rows(&self, cx: &App) -> Vec<Row> {
        let mut grouped = HashMap::<&std::path::Path, Vec<usize>>::new();
        for index in self.filtered_threads(cx) {
            grouped
                .entry(self.project_of(&self.host.threads[index].checkout))
                .or_default()
                .push(index);
        }
        let query = self.navigation.filter.read(cx).text().trim().to_lowercase();
        let mut rows = Vec::new();
        for (pi, project) in self.state.projects.iter().enumerate() {
            let matching = grouped.get(project.checkout.as_path());
            let project_matches = query.is_empty() || project.name.to_lowercase().contains(&query);
            if matching.is_none() && (!project_matches || self.navigation.attention) {
                continue;
            }
            rows.push(Row::Project(pi));
            if (!self.navigation.collapsed.contains(&project.checkout) || !query.is_empty())
                && let Some(indices) = matching
            {
                rows.extend(indices.iter().copied().map(Row::Thread));
            }
        }
        rows
    }

    fn project_row(&self, pi: usize, cx: &mut Context<Self>) -> AnyElement {
        let project = &self.state.projects[pi];
        let checkout = project.checkout.clone();
        let hide = checkout.clone();
        let expanded = !self.navigation.collapsed.contains(&checkout);
        let unavailable = self.navigation.available.get(&checkout) == Some(&false);
        row()
            .w_full()
            .h(px(ROW_HEIGHT))
            .gap_0()
            .child(
                nav_row(
                    ("project", pi),
                    format!(
                        "{} project, {}{}",
                        project.name,
                        checkout.display(),
                        if unavailable {
                            ", folder unavailable"
                        } else {
                            ""
                        }
                    ),
                    false,
                )
                .w_auto()
                .flex_1()
                .min_w_0()
                .role(Role::TreeItem)
                .aria_level(1)
                .aria_expanded(expanded)
                .child(icon(if expanded { "down" } else { "right" }))
                .child(icon("folder").text_color(rgb(if unavailable { AMBER } else { MINT })))
                .child(
                    title(project.name.clone())
                        .text_color(rgb(TEXT))
                        .font_weight(FontWeight::MEDIUM),
                )
                .when(unavailable, |el| el.child(tag("Missing", AMBER)))
                .on_activate(cx, move |this, _, cx| {
                    if !this.navigation.collapsed.remove(&checkout) {
                        this.navigation.collapsed.insert(checkout.clone());
                    }
                    cx.notify();
                }),
            )
            .child(
                icon_button(
                    ("project-actions", pi),
                    "more",
                    format!("Actions for {} project", project.name),
                )
                .on_activate(cx, move |this, _, cx| {
                    this.navigation.menu = Some(format!("project:{}", hide.display()));
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    fn host_thread_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let thread = &self.host.threads[index];
        let id = thread.id.clone();
        let selected = self.host.selected.as_ref() == Some(&id);
        if let Some((editing, field)) = &self.navigation.rename
            && editing == &id
        {
            return row()
                .w_full()
                .h(px(ROW_HEIGHT))
                .gap_1()
                .pl_2()
                .child(div().flex_1().min_w_0().child(field.clone()))
                .child(
                    icon_button("rename-save", "right", "Save thread name")
                        .on_activate(cx, |this, window, cx| this.finish_rename(window, cx)),
                )
                .child(
                    icon_button("rename-cancel", "close", "Cancel rename").on_activate(
                        cx,
                        |this, window, cx| {
                            this.navigation.rename = None;
                            window.focus(&this.focus, cx);
                            cx.notify();
                        },
                    ),
                )
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if this
                        .navigation
                        .rename
                        .as_ref()
                        .is_some_and(|(_, field)| field.read(cx).is_composing())
                    {
                        return;
                    }
                    if event.keystroke.key == "enter" {
                        this.finish_rename(window, cx);
                        cx.stop_propagation();
                    } else if event.keystroke.key == "escape" {
                        this.navigation.rename = None;
                        window.focus(&this.focus, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
                .into_any_element();
        }
        let menu_id = id.clone();
        let context_id = id.clone();
        let unread = self.preferences.value.navigation.unread.contains(&id);
        let status = nav::state_label(thread);
        let short = if status == "Needs approval" {
            "Approve"
        } else {
            status
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let age = nav::age(thread.updated_at, now);
        let label = self.thread_title(&id);
        let worktree = self
            .preferences
            .value
            .navigation
            .worktree(&thread.checkout)
            .is_some();
        row()
            .w_full()
            .h(px(ROW_HEIGHT))
            .gap_0()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    if this.host.protocol() >= 3 {
                        this.navigation.menu = Some(context_id.clone());
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
            .child(
                nav_row(
                    ("host-thread", index),
                    format!(
                        "Thread {} · {} · {} / {}. {}{}{}",
                        id,
                        status,
                        thread.model.provider,
                        thread.model.model_id,
                        label,
                        if worktree {
                            ". In its own worktree"
                        } else {
                            ""
                        },
                        if unread { ". Unread activity" } else { "" }
                    ),
                    selected,
                )
                .w_auto()
                .flex_1()
                .min_w_0()
                .role(Role::TreeItem)
                .aria_level(2)
                .aria_selected(selected)
                .pl(px(12.))
                .child(dot(if unread {
                    BLUE
                } else {
                    host_status_color(thread.status)
                }))
                .child(
                    title(label)
                        .text_color(rgb(TEXT))
                        .when(unread, |el| el.font_weight(FontWeight::SEMIBOLD)),
                )
                // A glyph, not a word: the title needs the width more.
                .when(worktree, |el| {
                    el.child(icon("branch").size(px(12.)).text_color(rgb(SUBTLE)))
                })
                .child(
                    tag(
                        if thread.archived == Some(true) {
                            "Archived"
                        } else {
                            short
                        },
                        if thread.pending_approval == Some(true) {
                            AMBER
                        } else {
                            MUTED
                        },
                    )
                    .flex_shrink_0(),
                )
                .when(!age.is_empty() && !thread.status.busy(), |el| {
                    el.child(tag(age, SUBTLE).flex_shrink_0())
                })
                .on_activate(cx, move |this, window, cx| {
                    this.select_host_thread(id.clone(), window, cx)
                }),
            )
            .when(self.host.protocol() >= 3, |el| {
                el.child(
                    icon_button(
                        ("thread-actions", index),
                        "more",
                        format!("Actions for {}", self.thread_title(&menu_id)),
                    )
                    .on_activate(cx, move |this, _, cx| {
                        this.navigation.menu = Some(menu_id.clone());
                        cx.notify();
                    }),
                )
            })
            .into_any_element()
    }

    fn tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.navigation_rows(cx);
        let empty = rows.is_empty();
        column()
            .id("project-tree")
            .role(Role::Tree)
            .aria_label("Projects, machines, and threads")
            .flex_1()
            .min_h_0()
            .px_2()
            .py_2()
            .when(empty, |el| {
                el.child(tag("No matching threads.", MUTED).p_2())
            })
            .when(!empty, |el| {
                el.child(
                    uniform_list(
                        "navigation-list",
                        rows.len(),
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|index| match rows[index] {
                                    Row::Project(pi) => this.project_row(pi, cx),
                                    Row::Thread(ti) => this.host_thread_row(ti, cx),
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.navigation.scroll)
                    .size_full(),
                )
            })
    }

    fn navigation_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        column()
            .px_2()
            .pt_2()
            .gap_2()
            .child(self.navigation.filter.clone())
            .child(
                row()
                    .gap_1()
                    .child(
                        segment("filter-attention", "Needs me", self.navigation.attention)
                            .aria_label("Filter threads needing attention")
                            .on_activate(cx, |this, _, cx| {
                                this.navigation.attention = !this.navigation.attention;
                                cx.notify();
                            }),
                    )
                    .child(
                        segment("filter-archived", "Archived", self.navigation.archived)
                            .aria_label("Include archived threads")
                            .on_activate(cx, |this, _, cx| {
                                this.navigation.archived = !this.navigation.archived;
                                cx.notify();
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        icon_button(
                            "thread-list-refresh",
                            "refresh",
                            "Refresh all threads and folders",
                        )
                        .on_activate(cx, |this, window, cx| {
                            this.navigation.available.clear();
                            this.load_threads(window, cx);
                        }),
                    ),
            )
            .child(
                row()
                    .gap_1()
                    .child(
                        button(
                            "thread-alerts",
                            if self.preferences.value.navigation.mute_notifications {
                                "Alerts off"
                            } else {
                                "Alerts on"
                            },
                        )
                        .aria_label("Toggle thread notifications and attention requests")
                        .on_activate(cx, |this, _, cx| {
                            let mut preferences = this.preferences.value.clone();
                            preferences.navigation.mute_notifications =
                                !preferences.navigation.mute_notifications;
                            this.set_preferences(preferences, PreferenceGroup::View, cx);
                        }),
                    )
                    .when(!self.preferences.value.navigation.hidden.is_empty(), |el| {
                        el.child(
                            button("show-hidden-projects", "Show hidden")
                                .aria_label("Show hidden projects")
                                .on_activate(cx, |this, _, cx| {
                                    let mut preferences = this.preferences.value.clone();
                                    preferences.navigation.hidden.clear();
                                    this.set_preferences(preferences, PreferenceGroup::View, cx);
                                    this.rebuild_projects(cx);
                                }),
                        )
                    }),
            )
            .when(self.navigation.managing.is_some(), |el| {
                el.child(tag("Saving thread…", SUBTLE))
            })
            .when(self.navigation.loading, |el| {
                el.child(tag("Loading threads…", SUBTLE))
            })
            .when(self.host.threads.len() > 200, |el| {
                el.child(tag("Newest 200 live · Refresh older threads", SUBTLE))
            })
            .when_some(self.navigation.error.clone(), |el, error| {
                el.child(
                    tag(error, AMBER)
                        .id("navigation-error")
                        .role(Role::Alert)
                        .line_height(px(16.)),
                )
            })
    }

    fn navigation_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(id) = &self.navigation.menu else {
            return div().into_any_element();
        };
        let mut panel = column()
            .mx_2()
            .mb_2()
            .p_2()
            .gap_2()
            .border_1()
            .border_color(rgb(EDGE))
            .rounded_md()
            .bg(rgb(PANEL));
        if let Some(path) = id.strip_prefix("project:") {
            let checkout = PathBuf::from(path);
            let select = checkout.clone();
            let hide = checkout.clone();
            panel = panel.child(tag("Project", MUTED))
                .child(button("project-open", "Open project").on_activate(cx, move |this, window, cx| { this.navigation.menu = None; this.change_checkout(select.clone(), window, cx); }))
                .child(button("project-hide", "Remove from sidebar").on_activate(cx, move |this, _, cx| {
                    if this.project_of(&this.host.checkout) == hide { this.navigation.error = Some("Open another project before removing this one. Host threads are kept.".into()); }
                    else { let mut value = this.preferences.value.clone(); value.navigation.hide(&hide); this.set_preferences(value, PreferenceGroup::View, cx); this.rebuild_projects(cx); }
                    this.navigation.menu = None; cx.notify();
                }))
                .child(tag("Removing a project keeps its host threads.", SUBTLE));
        } else if let Some(thread) = self.host.threads.iter().find(|thread| thread.id == *id) {
            let rename = id.clone();
            let archive = id.clone();
            let archived = thread.archived == Some(true);
            let tree = self.preferences.value.navigation.worktree(&thread.checkout);
            let branch = tree.map(|tree| tree.branch.clone());
            let remove = thread.checkout.clone();
            panel = panel
                .child(tag(self.thread_title(id), TEXT).truncate())
                .child(
                    button("thread-rename", "Rename thread…")
                        .on_activate(cx, move |this, window, cx| {
                            this.begin_rename(rename.clone(), window, cx)
                        }),
                )
                .child(
                    button(
                        "thread-archive",
                        if archived {
                            "Unarchive thread"
                        } else {
                            "Archive thread"
                        },
                    )
                    .on_activate(cx, move |this, window, cx| {
                        this.archive_thread(archive.clone(), !archived, window, cx)
                    }),
                )
                .when_some(branch, |el, branch| {
                    el.child(tag(format!("Worktree on {branch}"), SUBTLE).truncate())
                        .child(
                            button("thread-remove-worktree", "Remove worktree…")
                                .disabled(self.worktree_busy)
                                .on_activate(cx, move |this, window, cx| {
                                    this.remove_worktree(remove.clone(), window, cx)
                                }),
                        )
                });
        }
        panel
            .child(
                button("navigation-menu-close", "Close").on_activate(cx, |this, _, cx| {
                    this.navigation.menu = None;
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    pub(super) fn begin_rename(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.host.protocol() < 3 || !self.host.connected {
            return;
        }
        self.sidebar = true;
        self.navigation
            .filter
            .update(cx, |field, cx| field.set_text("", cx));
        self.navigation.attention = false;
        if let Some(thread) = self.host.threads.iter().find(|thread| thread.id == id) {
            let project = self.project_of(&thread.checkout).to_path_buf();
            self.navigation.collapsed.remove(&project);
            if thread.archived == Some(true) {
                self.navigation.archived = true;
            }
        }
        if let Some(index) = self
            .navigation_rows(cx)
            .iter()
            .position(|row| matches!(row, Row::Thread(index) if self.host.threads[*index].id == id))
        {
            self.navigation
                .scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }
        let title = self.thread_title(&id);
        let field = cx.new(|cx| {
            let mut field = TextField::new("Thread name", "Name this thread", cx);
            field.set_text(&title, cx);
            field
        });
        window.focus(&field.focus_handle(cx), cx);
        self.navigation.rename = Some((id, field));
        self.navigation.menu = None;
        self.navigation.error = None;
        cx.notify();
    }

    fn finish_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, field)) = &self.navigation.rename else {
            return;
        };
        let name = field.read(cx).text().trim().to_owned();
        if name.is_empty() || name.chars().count() > 200 {
            self.navigation.error = Some("Use a name between 1 and 200 characters.".into());
            cx.notify();
            return;
        }
        let id = id.clone();
        self.manage_thread(id.clone(), Request::rename_thread(id, name), window, cx);
    }

    pub(super) fn archive_thread(
        &mut self,
        id: String,
        archived: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.manage_thread(
            id.clone(),
            Request::archive_thread(id, archived),
            window,
            cx,
        );
    }

    fn manage_thread(
        &mut self,
        id: String,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(operation) = self.begin_thread_management(id, &request, cx) else {
            return;
        };
        self.host_request::<ThreadReply>(request, window, cx, move |this, reply, window, cx| {
            this.finish_thread_management(operation, reply, window, cx);
        });
    }

    pub(super) fn begin_thread_management(
        &mut self,
        id: String,
        request: &Request,
        cx: &App,
    ) -> Option<ThreadManagement> {
        if self.navigation.managing.is_some() || !self.host.connected || self.host.protocol() < 3 {
            return None;
        }
        self.navigation.management_generation += 1;
        let operation = ThreadManagement {
            epoch: self.host.epoch,
            generation: self.navigation.management_generation,
            rename: self.navigation.rename.as_ref().and_then(|(thread, field)| {
                (matches!(request, Request::RenameThread { .. }) && thread == &id)
                    .then(|| (field.clone(), field.read(cx).text().to_owned()))
            }),
            id,
        };
        if self.navigation.menu.as_deref() == Some(&operation.id) {
            self.navigation.menu = None;
        }
        self.navigation.managing = Some(operation.clone());
        self.navigation.error = None;
        Some(operation)
    }

    pub(super) fn finish_thread_management(
        &mut self,
        operation: ThreadManagement,
        reply: Result<ThreadReply, plait_core::host::Error>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.host.epoch != operation.epoch
            || self
                .navigation
                .managing
                .as_ref()
                .map(|pending| pending.generation)
                != Some(operation.generation)
        {
            return;
        }
        self.navigation.managing = None;
        let result = match reply {
            Ok(reply) if reply.thread.id == operation.id => {
                self.apply_managed_thread(ThreadSummary::from(&reply.thread), cx)
            }
            Ok(_) => Err("Host returned a different thread. Refresh before trying again.".into()),
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = result {
            self.navigation.error = Some(error);
            cx.notify();
            return;
        }
        // A reply belongs to the field that sent it, not a later edit or focus.
        if let Some((sent_field, sent_text)) = operation.rename
            && self.navigation.rename.as_ref().is_some_and(|(id, field)| {
                id == &operation.id && field == &sent_field && field.read(cx).text() == sent_text
            })
        {
            if sent_field.focus_handle(cx).is_focused(window) {
                window.focus(&self.focus, cx);
            }
            self.navigation.rename = None;
        }
        cx.notify();
    }

    /// New thread, the model it will start with, and a way to pick another.
    fn new_thread_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.new_thread_model().map(|model| model.name.clone());
        let name = match &model {
            Some(model) => format!("New thread with {model}"),
            None => "New thread: choose a model…".to_owned(),
        };
        row()
            .gap(px(1.))
            .child(
                nav_row("new-local-thread", name, false)
                    .aria_keyshortcuts(self.shortcut_aria("thread.new"))
                    .w_auto()
                    .flex_1()
                    .child(icon("plus").text_color(rgb(MINT)))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(rgb(TEXT))
                            .child("New thread"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(rgb(SUBTLE))
                            .child(model.unwrap_or_else(|| "Choose a model…".into())),
                    )
                    .on_activate(cx, |this, window, cx| this.new_thread(None, window, cx)),
            )
            .child(
                icon_button("new-thread-model", "down", "New thread with another model…")
                    .aria_keyshortcuts(self.shortcut_aria("thread.new_with_model"))
                    .on_activate(cx, |this, window, cx| {
                        this.open_model_picker(ModelPurpose::NewThread, window, cx)
                    }),
            )
    }

    fn actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The first row centers on the 44-point header beside it.
        column()
            .flex_shrink_0()
            .px_2()
            .pt(px((CHROME_HEIGHT - ROW_HEIGHT) / 2.))
            .pb_1()
            .gap(px(1.))
            .child(self.new_thread_row(cx))
            .child(
                nav_row("open-repository", "Open repository…", false)
                    .aria_keyshortcuts(self.shortcut_aria("repository.open"))
                    .child(icon("folder"))
                    .child(title("Open repository…").text_color(rgb(TEXT)))
                    .child(tag(self.shortcut_label("repository.open"), SUBTLE))
                    .on_activate(cx, |this, window, cx| {
                        this.open_repository(&OpenRepository, window, cx)
                    }),
            )
    }

    /// Host state stays visible; remote entries are labeled where they appear.
    fn host_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (label, tint) = if self.host.connecting {
            ("Connecting to local host…", MUTED)
        } else if self.host.connected {
            ("Local host connected", MINT)
        } else {
            ("Local host disconnected", AMBER)
        };
        row()
            .h(px(40.))
            .pl_4()
            .pr_2()
            .flex_shrink_0()
            .justify_between()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                pill()
                    .min_w_0()
                    .child(dot(tint))
                    .child(tag(label, MUTED).truncate()),
            )
            .child(
                row()
                    .gap_1()
                    .when(self.update_available(), |el| {
                        el.child(
                            toned_button("sidebar-update", "Update", Tone::Filled)
                                .aria_label("Update available. Open Settings to install.")
                                .on_activate(cx, |this, window, cx| {
                                    this.show_settings(true, window, cx)
                                }),
                        )
                    })
                    .child(
                        icon_button("settings", "sliders", "Settings…")
                            .aria_keyshortcuts(self.shortcut_aria("app.settings"))
                            .on_activate(cx, |this, window, cx| {
                                this.show_settings(true, window, cx)
                            }),
                    ),
            )
    }

    pub(super) fn navigation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        column()
            .relative()
            .w(px(self.panel_width(super::panels::Panel::Sidebar)))
            .flex_shrink_0()
            .bg(rgb(RAIL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(self.actions(cx))
            .child(self.navigation_controls(cx))
            .child(self.tree(cx))
            .child(self.navigation_menu(cx))
            .child(self.host_footer(cx))
            .child(self.resize_handle(super::panels::Panel::Sidebar, cx))
    }
}
