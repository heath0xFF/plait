//! One visible project graph. Host/Git/storage reads retire at the visibility boundary.
mod render;
use super::{Plait, theme::*, thread_controls::TextTooltip};
use gpui::{prelude::*, *};
use plait_core::{
    host::{
        self, Client,
        worktree::{self, Checkout},
    },
    tabs::{ReadStamp, TabTarget},
    worktree_graph::{self, Graph},
    worktree_size::{self, Cache, Key, Size},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

actions!(plait, [Worktrees]);
#[derive(Clone)]
pub(super) enum Item {
    Head(usize, usize),
    Commit(usize),
    Changed(usize, usize),
    Detached(usize),
}
#[derive(Default)]
pub(super) struct View {
    pub rows: Vec<Checkout>,
    pub graph: Graph,
    pub items: Vec<Item>,
    pub sizes: Vec<Option<Result<Size, String>>>,
    pub titles: Vec<String>,
    pub head_labels: Vec<String>,
    pub commit_labels: Vec<String>,
    pub ages: Vec<String>,
    pub total: String,
    pub largest: f32,
    pub words: Vec<&'static str>,
    pub openable: Vec<bool>,
    pub selected: usize,
    pub selection: Option<PathBuf>,
    pub error: Option<String>,
    pub loading: bool,
    pub generation: u64,
    pub cancelled: Arc<AtomicBool>,
    pub scroll: UniformListScrollHandle,
    pub cache: Cache,
    pub sort_size: bool,
    pub removal: plait_core::worktree_removal::Removal,
}
impl View {
    pub fn park(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.generation += 1;
        self.rows.clear();
        self.graph = Graph::default();
        self.items.clear();
        self.sizes.clear();
        self.titles.clear();
        self.head_labels.clear();
        self.commit_labels.clear();
        self.ages.clear();
        self.total.clear();
        self.largest = 1.;
        self.words.clear();
        self.openable.clear();
        self.loading = false;
    }
    pub fn account(&mut self) {
        let total = self
            .sizes
            .iter()
            .filter_map(|s| s.as_ref().and_then(|s| s.as_ref().ok()))
            .map(Size::total)
            .fold(0u64, u64::saturating_add);
        let incomplete = self.sizes.iter().any(|s| {
            s.as_ref()
                .is_none_or(|s| s.as_ref().map_or(true, |s| s.incomplete))
        });
        self.largest = self
            .sizes
            .iter()
            .filter_map(|s| s.as_ref().and_then(|s| s.as_ref().ok()))
            .map(Size::total)
            .max()
            .unwrap_or(1)
            .max(1) as f32;
        self.total = if self.sizes.iter().all(Option::is_none) {
            "Measuring".into()
        } else {
            format!(
                "{}{} total",
                if incomplete { "over " } else { "" },
                bytes(total)
            )
        };
    }
    pub fn prepare(&mut self) {
        self.account();
        let now = super::commits::now().max(0) as u64 * 1000;
        self.ages = self
            .rows
            .iter()
            .map(|r| {
                r.last_used.map_or("Unknown".into(), |t| {
                    plait_core::navigation::age(Some(t), now)
                })
            })
            .collect();
        self.commit_labels = self
            .graph
            .rows
            .iter()
            .map(|r| {
                format!(
                    "{}  {}  · {}",
                    r.commit.commit.short,
                    r.commit.commit.subject,
                    plait_core::navigation::age(
                        Some(r.commit.commit.time.max(0) as u64 * 1000),
                        now
                    )
                )
            })
            .collect();
        self.items.clear();
        for (i, row) in self.graph.rows.iter().enumerate() {
            for &lane in &row.heads {
                if self.rows[lane].preservation.changed.is_some_and(|n| n > 0) {
                    self.items.push(Item::Changed(i, lane));
                }
                self.items.push(Item::Head(i, lane));
            }
            self.items.push(Item::Commit(i));
        }
        for (lane, value) in self.graph.lanes.iter().enumerate() {
            if value.head.is_none() {
                self.items.push(Item::Detached(lane));
            }
        }
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }
}
fn size_label(size: Option<&Result<Size, String>>) -> String {
    match size {
        None => "Measuring".into(),
        Some(Err(_)) => "Unavailable".into(),
        Some(Ok(s)) => format!(
            "{}{}",
            if s.incomplete { "over " } else { "" },
            bytes(s.total())
        ),
    }
}
fn bytes(n: u64) -> String {
    if n >= 1 << 30 {
        format!("{:.1} GiB", n as f64 / (1u64 << 30) as f64)
    } else if n >= 1 << 20 {
        format!("{:.1} MiB", n as f64 / (1u64 << 20) as f64)
    } else {
        format!("{:.1} KiB", n as f64 / 1024.)
    }
}
fn key(row: &Checkout) -> Key {
    Key {
        root: row.path.clone(),
        identity: row.identity.clone(),
        head: row.head.clone(),
        last_used: row.last_used,
        changed: row.preservation.changed,
        removed: row.removed,
    }
}
fn hover(text: String, cx: &mut App) -> AnyView {
    cx.new(|_| TextTooltip(text)).into()
}
fn safe_text(text: &str) -> String {
    plait_core::display_text::escape(text)
}
impl Plait {
    pub(super) fn restore_worktrees_bookmark(&mut self) {
        let bookmark = self
            .tabs
            .model
            .active()
            .and_then(|id| self.tabs.model.get(id))
            .map(|t| t.session().bookmark());
        let (selected, size_sort) = match bookmark {
            Some(plait_core::tabs::bookmark::BookmarkRecord::Worktrees {
                selected,
                size_sort,
            }) => (selected.clone(), *size_sort),
            _ => (None, false),
        };
        self.worktrees.selection = selected;
        self.worktrees.sort_size = size_sort;
    }
    fn select_worktree_lane(&mut self, lane: usize, cx: &mut Context<Self>) {
        self.worktrees.selected = lane;
        self.save_worktrees_bookmark(cx);
        cx.notify();
    }
    fn save_worktrees_bookmark(&mut self, cx: &mut Context<Self>) {
        self.worktrees.selection = self
            .worktrees
            .rows
            .get(self.worktrees.selected)
            .map(|r| r.path.clone());
        if let Some(id) = self.tabs.model.active() {
            let result = self.tabs.model.save_bookmark(
                id,
                plait_core::tabs::bookmark::BookmarkRecord::Worktrees {
                    selected: self.worktrees.selection.clone(),
                    size_sort: self.worktrees.sort_size,
                },
            );
            self.tab_error(result);
            self.queue_tab_save(cx);
        }
    }

    pub(super) fn worktree_context_line(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let snapshot = self.host.thread.snapshot.as_ref()?;
        let tree = self
            .preferences
            .value
            .navigation
            .worktree(&snapshot.checkout)?;
        let project = tree.main.clone();
        Some(
            row()
                .gap_2()
                .child(tag(
                    "thread-worktree-context",
                    format!("Worktree · {}", safe_text(&tree.branch)),
                    colors().subtle,
                ))
                .child(
                    button("thread-worktree-view", "Worktrees")
                        .tooltip(|_, cx| {
                            hover(
                                "See this project's branches, storage and removal checks.".into(),
                                cx,
                            )
                        })
                        .on_activate(cx, move |this, window, cx| {
                            this.open_worktrees(project.clone(), window, cx)
                        }),
                )
                .into_any_element(),
        )
    }

    pub(super) fn visible_project_checkout(&self) -> &std::path::Path {
        self.tabs
            .model
            .active()
            .and_then(|id| self.tabs.model.get(id))
            .filter(|tab| tab.session().target().is_worktrees())
            .map_or(&self.host.creation.checkout, |tab| {
                &tab.session().target().checkout().root
            })
    }
    pub(super) fn worktrees_visible(&self) -> bool {
        self.tabs
            .model
            .active()
            .and_then(|id| self.tabs.model.get(id))
            .is_some_and(|tab| tab.session().target().is_worktrees())
    }
    pub(super) fn open_worktrees(
        &mut self,
        project: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigation.menu = None;
        if self.settings_open {
            self.show_settings(false, window, cx);
        }
        let project = self.project_of(&project).to_owned();
        self.open_tab_target(
            TabTarget::Worktrees {
                checkout: self.tab_checkout(project),
            },
            window,
            cx,
        );
    }
    pub(super) fn refresh_worktrees_view(&mut self, force: bool, cx: &mut Context<Self>) {
        if !self.worktrees_visible() {
            return;
        }
        self.worktrees.cancelled.store(true, Ordering::Release);
        self.worktrees.cancelled = Arc::new(AtomicBool::new(false));
        self.worktrees.generation += 1;
        let generation = self.worktrees.generation;
        self.worktrees.error = None;
        if force && self.worktrees.removal.pending.is_some() && !self.worktree_busy {
            self.finish_view_removal(cx);
            return;
        }
        if !self.host.connection.connected {
            self.worktrees.loading = false;
            self.worktrees.error =
                Some("Offline · Connect the host to see this project's worktrees.".into());
            return;
        }
        let Some(tab) = self
            .tabs
            .model
            .active()
            .and_then(|id| self.tabs.model.get(id))
        else {
            return;
        };
        let Some(stamp) = self.tabs.model.read_stamp(tab.id()) else {
            return;
        };
        let root = tab.session().target().checkout().root.clone();
        self.worktrees.loading = true;
        if force {
            self.worktrees.cache.clear();
        }
        let cancelled = self.worktrees.cancelled.clone();
        let task = cx.background_executor().spawn(async move {
            let inventory = worktree::inventory(&Client::from_environment()?, root)?;
            if cancelled.load(Ordering::Acquire) {
                return Err(host::Error::Protocol("Read cancelled.".into()));
            }
            let heads = inventory
                .checkouts
                .iter()
                .filter_map(|row| row.head.clone())
                .collect::<Vec<_>>();
            let (commits, more) = plait_core::git::log::graph(&inventory.checkouts[0].path, &heads)
                .map_err(host::Error::Protocol)?;
            let graph = worktree_graph::layout(&inventory.checkouts, commits, more);
            Ok((inventory.checkouts, graph))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if !this.accepts_worktrees(stamp, generation) {
                    return;
                }
                this.worktrees.loading = false;
                match result {
                    Err(error) => this.worktrees.error = Some(error.to_string()),
                    Ok((rows, graph)) => {
                        this.worktrees.sizes = rows
                            .iter()
                            .map(|row| this.worktrees.cache.get(&key(row)).cloned().map(Ok))
                            .collect();
                        this.worktrees.rows = rows;
                        this.worktrees.graph = graph;
                        this.worktrees.selected = this
                            .worktrees
                            .selection
                            .as_ref()
                            .and_then(|p| this.worktrees.rows.iter().position(|r| &r.path == p))
                            .unwrap_or(0);
                        this.sort_worktree_lanes(this.worktrees.sort_size, cx);
                        this.measure_worktrees(stamp, generation, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn accepts_worktrees(&self, stamp: ReadStamp, generation: u64) -> bool {
        self.worktrees_visible()
            && self.tabs.model.accepts_read(stamp)
            && self.worktrees.generation == generation
    }
    pub(super) fn worktree_labels(&mut self) {
        self.worktrees.titles.clear();
        self.worktrees.words.clear();
        self.worktrees.openable.clear();
        for row in &self.worktrees.rows {
            let thread = row
                .thread_id
                .as_ref()
                .and_then(|id| self.host.connection.threads.iter().find(|t| &t.id == id));
            self.worktrees.openable.push(thread.is_some());
            self.worktrees.titles.push(if row.main {
                "Main checkout".into()
            } else if row.creation_id.is_none() {
                "External worktree".into()
            } else {
                safe_text(thread.map_or("Thread unavailable", |t| {
                    t.title
                        .as_deref()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or("Untitled thread")
                }))
            });
            self.worktrees.words.push(if row.removed {
                "Removed"
            } else if row.missing {
                "Missing"
            } else {
                thread.map_or("", |t| {
                    plait_core::tabs::row::summary_state(
                        t,
                        self.preferences.value.navigation.unread.contains(&t.id),
                    )
                    .word(true)
                })
            });
        }
        self.worktrees.head_labels = self
            .worktrees
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let mut facts = Vec::new();
                if let Some(n) = row.preservation.changed {
                    facts.push(if n == 0 {
                        "Clean".into()
                    } else {
                        format!("{n} changed")
                    });
                }
                if let Some(n) = row.preservation.unpushed
                    && n > 0
                {
                    facts.push(format!("{n} not pushed"));
                }
                if self.worktrees.graph.lanes[i].merged {
                    facts.push("Merged".into());
                } else if self.worktrees.graph.lanes[i].detached {
                    facts.push("Detached".into());
                }
                format!(
                    "{}  {} · {} · {}",
                    self.worktrees.titles[i],
                    self.worktrees.words[i],
                    safe_text(row.branch.as_deref().unwrap_or("Detached")),
                    facts.join(" · ")
                )
            })
            .collect();
    }
    fn measure_worktrees(&mut self, stamp: ReadStamp, generation: u64, cx: &mut Context<Self>) {
        let pending: Vec<_> = self
            .worktrees
            .rows
            .iter()
            .enumerate()
            .filter(|(i, _)| self.worktrees.sizes[*i].is_none())
            .map(|(i, row)| (i, key(row), row.thread_id.clone(), row.removed))
            .collect();
        // Another checkout nested in this one is measured once, as itself.
        let checkouts: Vec<_> = self
            .worktrees
            .rows
            .iter()
            .filter_map(|row| {
                let id = row.identity.as_ref()?;
                Some((
                    row.path.clone(),
                    (id.dev.parse().ok()?, id.ino.parse().ok()?),
                ))
            })
            .collect();
        let cancelled = self.worktrees.cancelled.clone();
        let store = self.tabs.host.store.clone();
        let task = cx.background_executor().spawn(async move {
            let mut results = Vec::new();
            for (i, key, thread, removed) in pending {
                if cancelled.load(Ordering::Acquire) {
                    break;
                }
                let cache = thread.map(|id| store.join("build-cache").join(id));
                let size = if removed {
                    worktree_size::measure_retained_cache(cache.as_deref(), &cancelled)
                } else {
                    let others = checkouts
                        .iter()
                        .filter(|(path, _)| *path != key.root)
                        .map(|(_, id)| *id)
                        .collect();
                    worktree_size::measure_verified(&key, cache.as_deref(), &others, &cancelled)
                };
                results.push((i, key, size));
            }
            results
        });
        cx.spawn(async move |this, cx| {
            let results = task.await;
            let _ = this.update(cx, |this, cx| {
                if !this.accepts_worktrees(stamp, generation) {
                    return;
                }
                for (_, key, size) in results {
                    let Some(i) = this
                        .worktrees
                        .rows
                        .iter()
                        .position(|row| self::key(row) == key)
                    else {
                        continue;
                    };
                    if let Ok(size) = &size {
                        this.worktrees.cache.put(key, size.clone());
                    }
                    this.worktrees.sizes[i] = Some(size);
                }
                this.sort_worktree_lanes(this.worktrees.sort_size, cx);
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn sort_worktree_lanes(&mut self, size: bool, cx: &mut Context<Self>) {
        self.worktrees.sort_size = size;
        let selected = self
            .worktrees
            .rows
            .get(self.worktrees.selected)
            .map(|r| r.path.clone());
        let mut pairs: Vec<_> = std::mem::take(&mut self.worktrees.rows)
            .into_iter()
            .zip(std::mem::take(&mut self.worktrees.sizes))
            .collect();
        pairs.sort_by_key(|(row, bytes)| {
            (
                !row.main,
                std::cmp::Reverse(if size {
                    bytes
                        .as_ref()
                        .and_then(|s| s.as_ref().ok())
                        .map_or(0, Size::total)
                } else {
                    row.last_used.unwrap_or(0)
                }),
            )
        });
        (self.worktrees.rows, self.worktrees.sizes) = pairs.into_iter().unzip();
        let commits = self
            .worktrees
            .graph
            .rows
            .iter()
            .map(|r| r.commit.clone())
            .collect();
        self.worktrees.graph =
            worktree_graph::layout(&self.worktrees.rows, commits, self.worktrees.graph.more);
        self.worktrees.selected = selected
            .and_then(|p| self.worktrees.rows.iter().position(|r| r.path == p))
            .unwrap_or(0);
        self.worktrees.prepare();
        self.worktree_labels();
        self.save_worktrees_bookmark(cx);
        cx.notify();
    }
    /// `path` and `identity` are the row drawn with the button. Input reaches the
    /// last drawn frame, and a size reply can re-sort rows before the next one,
    /// so an index would name whichever worktree moved into that place.
    pub(super) fn ask_remove_view_worktree(
        &mut self,
        path: &std::path::Path,
        identity: Option<&plait_core::git::worktree::Identity>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let n = self.worktrees.selected;
        let Some(tree) = self.worktrees.rows.get(n) else {
            return;
        };
        if tree.path != path
            || tree.identity.as_ref() != identity
            || tree.main
            || tree.preservation.refusal.is_some()
            || self.worktree_busy
            || !self.host.connection.connected
            || self.worktrees.loading
            || self.worktrees.error.is_some()
        {
            return;
        }
        let path = tree.path.clone();
        if !self.worktrees.removal.ask(
            tree,
            self.tabs.host.clone(),
            plait_core::host_state::request_id(),
        ) {
            return;
        }
        let answer=window.prompt(PromptLevel::Warning,"Remove this worktree?",Some(&format!("Delete {}? Its branch and thread history stay. Private build cache stays. The host will check for work to preserve again.",path.display())),&[PromptButton::Other("Remove".into()),PromptButton::Cancel("Cancel".into())],cx);
        cx.spawn(async move |this, cx| {
            let confirmed = answer.await == Ok(0);
            let _ = this.update(cx, |this, cx| {
                if this.worktrees.removal.answer(confirmed).is_some() {
                    this.finish_view_removal(cx);
                }
            });
        })
        .detach();
    }
    fn finish_view_removal(&mut self, cx: &mut Context<Self>) {
        let Some(attempt) = self.worktrees.removal.pending.clone() else {
            return;
        };
        if attempt.host != self.tabs.host {
            self.worktrees.removal.message =
                Some("Reconnect the original host to recover this removal.".into());
            return;
        }
        self.worktree_busy = true;
        let request = attempt.request.clone();
        let task = cx.background_executor().spawn(async move {
            super::worktree::await_operation(&Client::from_environment()?, &request)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.worktree_busy = false;
                if this
                    .worktrees
                    .removal
                    .finish(&attempt, result.map_err(|e| e.to_string()))
                {
                    this.refresh_host_worktrees(cx);
                    this.refresh_worktrees_view(false, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
