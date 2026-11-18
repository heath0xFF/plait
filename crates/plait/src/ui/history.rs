//! Background history reads and projection; render only consumes prepared data.
use super::{Plait, theme::*};
use gpui::{prelude::*, *};
use plait_core::{
    history::{self, History, Key},
    host::{Client, HistoryField, Snapshot},
    markdown, transcript,
};
use std::{
    collections::{HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub(super) enum Load {
    Latest,
    Older,
    Part(Key, HistoryField, bool),
}
#[derive(Default)]
pub(super) struct HistoryView {
    pub cache: Arc<History>,
    pub display: Option<Arc<Snapshot>>,
    pub enabled: bool,
    pub loading: Option<Load>,
    pub error: Option<String>,
    failed: Option<Load>,
    tried: Option<u64>,
    revision: u64,
    projecting: bool,
    pub expanded: HashSet<String>,
    pub now: u64,
    timer: bool,
}
fn millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
fn tail_stamp(snapshot: &Snapshot) -> u64 {
    let mut hash = DefaultHasher::new();
    for message in &snapshot.messages {
        message.id.hash(&mut hash);
        message.text.len().hash(&mut hash);
        message.reasoning.len().hash(&mut hash);
    }
    for tool in &snapshot.tools {
        tool.id.hash(&mut hash);
        (tool.status as u8).hash(&mut hash);
    }
    (snapshot.status as u8).hash(&mut hash);
    hash.finish()
}
impl Plait {
    pub(super) fn conversation_snapshot(&self) -> Option<&Snapshot> {
        self.history
            .display
            .as_deref()
            .or(self.host.snapshot.as_deref())
    }
    pub(super) fn conversation_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history.enabled = self.host.protocol() >= 3;
        self.history.revision += 1;
        self.history.now = millis();
        self.prepare_conversation(window, cx);
        self.tick_retry(window, cx);
    }
    fn tick_retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.timer
            || self
                .host
                .snapshot
                .as_ref()
                .is_none_or(|s| s.retry.is_none())
        {
            return;
        }
        self.history.timer = true;
        let epoch = self.host.epoch;
        let watch_epoch = self.host.watch_epoch;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let keep = this
                    .update_in(cx, |this, _, cx| {
                        if this.host.epoch != epoch || this.host.watch_epoch != watch_epoch {
                            return false;
                        }
                        this.history.now = millis();
                        let keep = this
                            .host
                            .snapshot
                            .as_ref()
                            .is_some_and(|s| s.retry.is_some());
                        if !keep {
                            this.history.timer = false;
                        }
                        cx.notify();
                        keep
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }
    /// One projection at a time. A newer snapshot marks the result stale and is
    /// coalesced into the next job, rather than spawning a task per token.
    fn prepare_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.projecting {
            return;
        }
        let Some(snapshot) = self.host.snapshot.clone() else {
            return;
        };
        self.history.projecting = true;
        let id = snapshot.id.clone();
        let epoch = self.host.epoch;
        let watch_epoch = self.host.watch_epoch;
        let revision = self.history.revision;
        let cache = self.history.cache.clone();
        let enabled = self.history.enabled;
        let previous = self.formatted.clone();
        let task = cx.background_executor().spawn(async move {
            let display = if enabled {
                cache.project(&snapshot)
            } else {
                (*snapshot).clone()
            };
            let stored = cache
                .entries
                .iter()
                .filter_map(|entry| match &entry.item {
                    plait_core::host::HistoryItem::Tool { id, status, .. }
                        if *status != plait_core::host::ToolStatus::Running
                            && display
                                .tools
                                .iter()
                                .any(|tool| tool.id == *id && tool.status == *status) =>
                    {
                        Some(id.clone())
                    }
                    _ => None,
                })
                .collect();
            let formatted = markdown::Transcript::update_stored(&previous, &display, &stored);
            (Arc::new(display), Arc::new(formatted))
        });
        cx.spawn_in(window, async move |this, cx| {
            let (display, formatted) = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.host.accepts_watch(epoch, watch_epoch, &id) {
                    return;
                }
                this.history.projecting = false;
                if this.history.revision != revision {
                    this.prepare_conversation(window, cx);
                    return;
                }
                let previous = this.history.display.replace(display);
                let before = std::mem::replace(&mut this.formatted, formatted);
                if this.reconcile_live_list(previous.as_deref()) {
                    this.remeasure_records(previous.as_deref(), &before);
                }
                this.conversation_rows_changed(cx);
                if this.history.enabled
                    && this.history.loading.is_none()
                    && let Some(live) = this.host.snapshot.as_ref()
                    && this.history.cache.needs_latest(live)
                    && this.history.tried != Some(tail_stamp(live))
                {
                    this.load_history(Load::Latest, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn load_history(&mut self, load: Load, window: &mut Window, cx: &mut Context<Self>) {
        if !self.history.enabled || !self.host.connected || self.history.loading.is_some() {
            return;
        }
        let Some(snapshot) = self.host.snapshot.as_ref() else {
            return;
        };
        let id = snapshot.id.clone();
        let epoch = self.host.epoch;
        let watch_epoch = self.host.watch_epoch;
        let before = match &load {
            Load::Older => match self.history.cache.older.clone() {
                Some(cursor) => cursor,
                None => return,
            },
            _ => None,
        };
        self.history.tried = Some(tail_stamp(snapshot));
        self.history.loading = Some(load.clone());
        self.history.error = None;
        self.history.failed = None;
        let cache = self.history.cache.clone();
        let operation = load.clone();
        let thread = id.clone();
        let task = cx.background_executor().spawn(async move {
            let client = Client::from_environment()?;
            let mut cache = (*cache).clone();
            match operation {
                Load::Latest => cache.latest(history::read_page(&client, &thread, None)?),
                Load::Older => cache.prepend(history::read_page(&client, &thread, before)?),
                Load::Part(key, field, restart) => {
                    let entry = cache.get(&key).ok_or_else(|| {
                        plait_core::host::Error::Protocol(
                            "This text is outside the reading window. Load its page again.".into(),
                        )
                    })?;
                    let entry = history::read_part(&client, &thread, entry, field, restart)?;
                    cache.replace(entry);
                }
            }
            Ok::<_, plait_core::host::Error>(Arc::new(cache))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.host.accepts_watch(epoch, watch_epoch, &id) {
                    return;
                }
                this.history.loading = None;
                match result {
                    Ok(cache) => {
                        if matches!(load, Load::Part(..)) {
                            this.clear_reading_selection();
                        }
                        this.history.cache = cache;
                        this.history.revision += 1;
                        this.prepare_conversation(window, cx);
                    }
                    Err(error) => {
                        this.history.error =
                            Some(format!("Could not load conversation text: {error}"));
                        this.history.failed = Some(load);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn jump_latest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.cache.detached {
            self.load_history(Load::Latest, window, cx);
        }
        self.live_scroll.scroll_to_end();
        cx.notify();
    }
    pub(super) fn scroll_history(
        &mut self,
        event: &ListScrollEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.is_scrolled && event.visible_range.start <= 3 && self.history.error.is_none() {
            self.load_history(Load::Older, window, cx);
        }
    }
    pub(super) fn history_heading(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.history.enabled {
            return div().into_any_element();
        }
        let loading = self.history.loading.is_some();
        let failed = self.history.failed.clone();
        let label = if let Some(error) = &self.history.error {
            error.clone()
        } else if loading {
            "Loading conversation text…".into()
        } else if self.history.cache.detached {
            "Reading older history · newer pages are outside this window".into()
        } else if self.history.cache.loaded && self.history.cache.older.is_none() {
            if self.live_scroll.scroll_px_offset_for_scrollbar().y >= px(-1.) {
                "Beginning of conversation"
            } else {
                "History loaded · scroll up for the beginning"
            }
            .into()
        } else if self.history.cache.loaded {
            "Earlier messages available".into()
        } else {
            "Loading complete history · live preview below".into()
        };
        div()
            .flex_shrink_0()
            .px_6()
            .py_1()
            .child(
                row()
                    .w_full()
                    .max_w(px(CONTENT_WIDTH))
                    .mx_auto()
                    .min_h(px(26.))
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.))
                            .text_color(rgb(if failed.is_some() {
                                colors().amber
                            } else {
                                colors().muted
                            }))
                            .child(label),
                    )
                    .when_some(failed, |row, load| {
                        row.child(
                            button("retry-history", "Retry")
                                .on_activate(cx, move |this, window, cx| {
                                    this.load_history(load.clone(), window, cx)
                                }),
                        )
                    })
                    .when(
                        !loading
                            && self.history.error.is_none()
                            && self.history.cache.older.is_some(),
                        |row| {
                            row.child(
                                button("older-history", "Load earlier")
                                    .on_activate(cx, |this, window, cx| {
                                        this.load_history(Load::Older, window, cx)
                                    }),
                            )
                        },
                    )
                    .when(self.history.cache.detached, |row| {
                        row.child(
                            button("latest-history", "Latest")
                                .on_activate(cx, |this, window, cx| this.jump_latest(window, cx)),
                        )
                    }),
            )
            .into_any_element()
    }
    pub(super) fn history_parts(
        &self,
        key: Key,
        field: Option<HistoryField>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let entry = self.history.cache.get(&key)?;
        if !entry
            .parts
            .iter()
            .any(|part| field.is_none_or(|field| field == part.field))
        {
            return None;
        }
        let mut view = column().gap_1();
        for part in entry
            .parts
            .iter()
            .filter(|part| field.is_none_or(|field| field == part.field))
        {
            let name = match part.field {
                HistoryField::Text => "Message",
                HistoryField::Reasoning => "Reasoning",
                HistoryField::Input => "Arguments",
                HistoryField::Output => "Output",
            };
            let id = format!("part-{:?}-{name}", key);
            let field = part.field;
            let next_key = key.clone();
            let start_key = key.clone();
            view = view.child(
                row()
                    .gap_2()
                    .flex_wrap()
                    .text_size(px(11.))
                    .text_color(rgb(colors().amber))
                    .child(format!(
                        "{name} · part {}{}",
                        part.number,
                        if part.has_next() {
                            " · more text available"
                        } else {
                            " · last part"
                        }
                    ))
                    .when(part.has_next(), |row| {
                        row.child(
                            button(SharedString::from(format!("{id}-next")), "Next part")
                                .disabled(self.history.loading.is_some())
                                .on_activate(cx, move |this, window, cx| {
                                    this.load_history(
                                        Load::Part(next_key.clone(), field, false),
                                        window,
                                        cx,
                                    )
                                }),
                        )
                    })
                    .when(part.start > 0, |row| {
                        row.child(
                            button(SharedString::from(format!("{id}-start")), "Start over")
                                .disabled(self.history.loading.is_some())
                                .on_activate(cx, move |this, window, cx| {
                                    this.load_history(
                                        Load::Part(start_key.clone(), field, true),
                                        window,
                                        cx,
                                    )
                                }),
                        )
                    }),
            );
        }
        Some(view.into_any_element())
    }
    pub(super) fn toggle_reasoning(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.history.expanded.remove(id) {
            self.history.expanded.insert(id.to_owned());
        }
        if let Some(snapshot) = self.conversation_snapshot()
            && let Some(index) = self
                .live_layout
                .iter()
                .position(|slot| match transcript::resolve(snapshot, *slot) {
                    Some(transcript::Row::Reasoning(m)) => m.id == id,
                    _ => false,
                })
        {
            self.live_scroll.remeasure_items(index..index + 1);
        }
        cx.notify();
    }
}
