//! Agent harnesses: which harnesses can run a thread here, and which one runs
//! the open thread. A version 4 host lists them once per connection; Settings
//! checks again on demand. Every string the model picker, the composer line,
//! and Settings draw is made when the reply arrives; rendering formats nothing.
//!
//! An older host lists none, so every thread runs in Pi and nothing changes.

use super::{
    Plait,
    commands::Command,
    model_picker::{model_key, model_label},
    palette::Item,
    settings,
    theme::*,
};
use gpui::{prelude::*, *};
use plait_core::host::{
    self, Model, ModelChoice, Snapshot,
    harness::{Harness, HarnessId, HarnessState, Harnesses, Request, ThreadHarness},
    providers,
};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct HarnessesState {
    /// The host connection `view` was read from.
    epoch: Option<u64>,
    view: Option<Arc<HarnessesView>>,
    /// Bumped whenever the list changes, so an open picker and the composer line rebuild.
    pub(super) generation: u64,
    busy: bool,
    /// The latest request; an earlier reply that lands after it is dropped.
    serial: u64,
    note: Option<SharedString>,
    /// The harness of the model last chosen for a new thread.
    chosen: Option<(HarnessId, ModelChoice)>,
    /// The harness `host.pending_create` was made for, so a retry repeats it.
    pending: HarnessId,
    /// The open thread's harness, the list generation, and what its composer line says.
    line: Option<(ThreadHarness, u64, ThreadLine)>,
}

/// What the composer line says about a thread that does not run in Pi.
pub(super) struct ThreadLine {
    /// The harness and the version of the thread's latest run, as "Codex 0.169.0".
    pub(super) label: SharedString,
    pub(super) protection: Option<SharedString>,
    /// The harness's own permission mode, when the host lists its modes.
    pub(super) mode: Option<ModeLine>,
}

/// A harness thread's own permission mode as the composer shows it.
#[derive(Debug, PartialEq)]
pub(super) struct ModeLine {
    /// The thread's chosen mode, or the harness default before it has one.
    pub(super) id: String,
    pub(super) label: SharedString,
    /// What the mode lets the harness do, or that the harness used another.
    pub(super) note: SharedString,
    /// Whether `note` says the harness is not using the chosen mode.
    pub(super) overridden: bool,
    /// The Permissions chip's hover text: what protects the checkout, and when a change applies.
    pub(super) tip: SharedString,
    /// Under the Permissions menu: when a choice applies.
    pub(super) applies: SharedString,
    /// What the composer says while a run is under way.
    pub(super) busy: SharedString,
}

/// One of a harness's own permission modes, as the host names it.
pub(super) struct ModeRow {
    pub(super) id: String,
    pub(super) label: SharedString,
    pub(super) detail: SharedString,
}

/// One harness as the picker, the composer, and Settings show it.
pub(super) struct HarnessRow {
    pub(super) id: HarnessId,
    pub(super) name: SharedString,
    pub(super) ready: bool,
    pub(super) state: SharedString,
    pub(super) note: Option<SharedString>,
    /// Version, the oldest Plait runs, and the one it was tested with.
    pub(super) version: SharedString,
    pub(super) path: Option<SharedString>,
    pub(super) protection: SharedString,
    /// Why a thread in this harness cannot send, when it is not ready.
    blocked: Option<SharedString>,
    /// Empty for Pi, whose models are the status catalog.
    pub(super) models: Vec<Model>,
    /// The harness's own permission modes; empty for Pi, which uses Plait's approvals.
    pub(super) modes: Vec<ModeRow>,
    pub(super) default_mode: Option<String>,
    /// Settings → Agent: what the default mode lets the harness do.
    pub(super) default_detail: SharedString,
}

pub(super) struct HarnessesView {
    pub(super) rows: Vec<HarnessRow>,
}

fn version_line(harness: &Harness) -> String {
    let mut parts = Vec::new();
    if let Some(version) = &harness.version {
        parts.push(format!("Version {version}"));
    }
    if let Some(supported) = &harness.supported {
        parts.push(format!("needs {} or newer", supported.min));
        parts.push(format!("tested with {}", supported.tested));
    }
    if parts.is_empty() {
        return "Built into the host".into();
    }
    let line = parts.join(" · ");
    // "needs" leads only when no version is known.
    let mut chars = line.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

impl HarnessRow {
    fn new(harness: Harness) -> Self {
        let ready = harness.state == HarnessState::Ready;
        let state = harness.state.label();
        let version = version_line(&harness).into();
        let note = harness.note.filter(|note| !note.trim().is_empty());
        let modes: Vec<ModeRow> = harness
            .modes
            .into_iter()
            .map(|mode| ModeRow {
                id: mode.id,
                label: mode.label.into(),
                detail: mode.detail.into(),
            })
            .collect();
        let default_detail = harness
            .default_mode
            .as_deref()
            .and_then(|id| modes.iter().find(|mode| mode.id == id))
            .map(|mode| mode.detail.clone())
            .unwrap_or_default();
        let blocked = (!ready).then(|| {
            let fix = note.as_deref().unwrap_or("See Settings → Agent.");
            SharedString::from(format!("{}: {}. {fix}", harness.name, state.to_lowercase()))
        });
        Self {
            id: harness.id,
            version,
            path: harness
                .path
                .as_deref()
                .map(|path| home_relative(path).into()),
            name: harness.name.into(),
            ready,
            state: state.into(),
            note: note.map(Into::into),
            protection: harness.protection.into(),
            blocked,
            models: harness.models.unwrap_or_default(),
            modes,
            default_mode: harness.default_mode,
            default_detail,
        }
    }

    pub(super) fn mode(&self, id: &str) -> Option<&ModeRow> {
        self.modes.iter().find(|mode| mode.id == id)
    }

    /// Only an installed harness can be pointed at another executable; Pi ships with the host.
    pub(super) fn has_path(&self) -> bool {
        self.id == HarnessId::Codex
    }
}

impl HarnessesView {
    pub(super) fn new(reply: Harnesses) -> Self {
        Self {
            rows: reply.harnesses.into_iter().map(HarnessRow::new).collect(),
        }
    }
}

/// One model of a harness other than Pi as a picker row. Favorites and the host's
/// default model are Pi's, so these rows have neither.
fn model_item(row: &HarnessRow, model: &Model) -> Item {
    let choice = model.choice();
    let item = Item::new(
        model_key(model),
        model.name.clone(),
        Command::HarnessModel(row.id, choice.clone()),
    )
    .detail(model_label(&choice))
    .section(row.name.clone())
    .enabled(model.authenticated);
    if model.authenticated {
        item
    } else {
        item.badge("Not signed in", colors().amber)
    }
}

/// Rows a harness thread's own model menu lists: that harness's models only.
pub(super) fn thread_items(row: &HarnessRow) -> Vec<Item> {
    row.models
        .iter()
        .map(|model| model_item(row, model))
        .collect()
}

/// Rows the new-thread picker lists after Pi's catalog: each ready harness's
/// models under its name, and one row saying what is wrong with each other one.
pub(super) fn new_thread_items(rows: &[HarnessRow]) -> Vec<Item> {
    let mut items = Vec::new();
    for row in rows.iter().filter(|row| row.id != HarnessId::Pi) {
        if row.ready && !row.models.is_empty() {
            items.extend(thread_items(row));
            continue;
        }
        let (badge, detail) = if row.ready {
            (
                "No models".into(),
                SharedString::from("It listed no models."),
            )
        } else {
            (row.state.clone(), row.note.clone().unwrap_or_default())
        };
        items.push(
            Item::new(
                format!("harness\u{1f}{}", row.name),
                row.name.clone(),
                Command::HarnessSettings,
            )
            .detail(detail)
            .section(row.name.clone())
            .badge(badge, colors().amber)
            .enabled(false),
        );
    }
    items
}

/// The harness a snapshot's thread runs in. Hosts before version 4 run only Pi.
pub(super) fn harness_of(snapshot: &Snapshot) -> HarnessId {
    snapshot
        .harness
        .as_ref()
        .map_or(HarnessId::Pi, |harness| harness.id)
}

/// What the composer says about a harness thread's own mode. The host clears
/// `reported` when the mode changes, so a report always describes the chosen mode.
pub(super) fn mode_line(row: &HarnessRow, key: &ThreadHarness) -> Option<ModeLine> {
    let id = key.mode.clone().or_else(|| row.default_mode.clone())?;
    let chosen = row.mode(&id);
    let label = chosen.map_or_else(|| SharedString::from(id.clone()), |mode| mode.label.clone());
    let name = &row.name;
    let reported = key.reported.as_deref().filter(|reported| *reported != id);
    let (note, overridden) = match reported.map(|reported| (reported, row.mode(reported))) {
        None => (
            chosen.map(|mode| mode.detail.clone()).unwrap_or_default(),
            false,
        ),
        Some((_, Some(used))) => (
            format!(
                "{name} is using {} instead of {label}, set by your {name} configuration. {}",
                used.label, used.detail
            )
            .into(),
            true,
        ),
        // `custom`, or a mode this list does not name.
        Some((_, None)) => (
            format!("{name} is using permissions its own configuration sets, not {label}.").into(),
            true,
        ),
    };
    let protection = row.protection.trim();
    let tip = format!(
        "{protection}{}Choose what {name} may do in this thread. A change applies from its next turn, even during a run.",
        if protection.is_empty() { "" } else { " " }
    );
    Some(ModeLine {
        id,
        label,
        note,
        overridden,
        tip: tip.into(),
        applies: format!("Applies from {name}'s next turn, even during a run.").into(),
        busy: format!(
            "Model locked during run · a permissions change applies from {name}'s next turn"
        )
        .into(),
    })
}

/// What a refused or failed harness request says. Host messages are shown as sent.
fn failure(error: &host::Error) -> String {
    if providers::needs_host_update(error) {
        return "Update the host to choose an agent harness.".into();
    }
    match error {
        host::Error::Rejected(rejected) => {
            format!("The host did not accept this: {}", rejected.message)
        }
        other => format!("{other} Nothing was changed."),
    }
}

impl Plait {
    /// Scene state, as if this connection's host had answered `harnesses`.
    #[cfg(test)]
    pub(super) fn inject_harnesses(&mut self, reply: Option<Harnesses>) {
        self.harnesses.epoch = Some(self.host.epoch);
        self.harnesses.view = reply.map(|reply| Arc::new(HarnessesView::new(reply)));
        self.harnesses.note = None;
        self.harnesses.generation += 1;
    }

    /// The harness list for this connection; empty before it is read and for older hosts.
    pub(super) fn harness_rows(&self) -> &[HarnessRow] {
        match &self.harnesses.view {
            Some(view)
                if self.harnesses.epoch == Some(self.host.epoch)
                    && self.host.connected
                    && self.host.protocol() >= 4 =>
            {
                &view.rows
            }
            _ => &[],
        }
    }

    pub(super) fn harness_row(&self, id: HarnessId) -> Option<&HarnessRow> {
        self.harness_rows().iter().find(|row| row.id == id)
    }

    /// A model of a harness other than Pi, as that harness listed it.
    pub(super) fn harness_model(&self, id: HarnessId, choice: &ModelChoice) -> Option<&Model> {
        self.harness_row(id)?
            .models
            .iter()
            .find(|model| model.choice() == *choice)
    }

    /// Whether `choice` can start or continue a thread in harness `id` now.
    pub(super) fn usable_in(&self, id: HarnessId, choice: &ModelChoice) -> bool {
        if id == HarnessId::Pi {
            return self.host.available(Some(choice));
        }
        self.harness_row(id).is_some_and(|row| row.ready)
            && self
                .harness_model(id, choice)
                .is_some_and(|model| model.authenticated)
    }

    /// The harness the open thread runs in.
    pub(super) fn thread_harness(&self) -> HarnessId {
        self.host
            .snapshot
            .as_deref()
            .map_or(HarnessId::Pi, harness_of)
    }

    /// Remember which harness a new thread's model came from.
    pub(super) fn choose_new_harness(&mut self, id: HarnessId, choice: &ModelChoice) {
        self.harnesses.chosen = Some((id, choice.clone()));
    }

    /// The harness for a new thread with `choice`: the one it was chosen from, else Pi.
    pub(super) fn new_thread_harness(&self, choice: &ModelChoice) -> HarnessId {
        match &self.harnesses.chosen {
            Some((id, chosen)) if chosen == choice => *id,
            _ => HarnessId::Pi,
        }
    }

    pub(super) fn set_pending_harness(&mut self, id: HarnessId) {
        self.harnesses.pending = id;
    }

    pub(super) fn pending_harness(&self) -> HarnessId {
        self.harnesses.pending
    }

    /// Why the open thread cannot send because of its harness, if it cannot.
    pub(super) fn harness_reason(&self) -> Option<String> {
        let id = self.thread_harness();
        if id == HarnessId::Pi {
            return None;
        }
        if self.host.protocol() < 4 {
            return Some("Update the host to continue this thread.".into());
        }
        match self.harness_row(id) {
            Some(row) => row.blocked.as_ref().map(ToString::to_string),
            None if self.harnesses.note.is_some() => {
                Some("Agent harnesses could not be checked · see Settings → Agent.".into())
            }
            None => Some(format!("Checking {}…", id.label())),
        }
    }

    /// Read the list once per host connection. Settings checks again on demand.
    pub(super) fn sync_harnesses(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || !self.host.connected || self.host.protocol() < 4 {
            return;
        }
        if self.harnesses.epoch == Some(self.host.epoch) {
            return;
        }
        self.harnesses = HarnessesState {
            epoch: Some(self.host.epoch),
            generation: self.harnesses.generation + 1,
            serial: self.harnesses.serial,
            chosen: self.harnesses.chosen.take(),
            pending: self.harnesses.pending,
            ..Default::default()
        };
        self.refresh_harnesses(cx);
    }

    /// Check every harness again: an install, sign-in, or update can change while the app is open.
    pub(super) fn refresh_harnesses(&mut self, cx: &mut Context<Self>) {
        self.send_harnesses(Request::harnesses(), cx);
    }

    /// Run harness `id` from `path`, or from the login shell's PATH when `None`.
    fn set_harness_path(
        &mut self,
        id: HarnessId,
        path: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.send_harnesses(Request::set_harness_path(id, path), cx);
    }

    /// Start new threads in harness `id` in `mode`. The reply is the whole list.
    pub(super) fn set_harness_mode(&mut self, id: HarnessId, mode: String, cx: &mut Context<Self>) {
        self.send_harnesses(Request::set_harness_mode(id, mode), cx);
    }

    /// Every request answers with the whole list, checked again.
    fn send_harnesses(&mut self, request: Request, cx: &mut Context<Self>) {
        if self.harnesses.busy || !self.host.connected || self.host.protocol() < 4 || cfg!(test) {
            return;
        }
        self.harnesses.busy = true;
        self.harnesses.serial += 1;
        let key = (self.host.epoch, self.harnesses.serial);
        let task = cx.background_executor().spawn(async move {
            let reply: Harnesses = host::Client::from_environment()?.send(&request)?;
            Ok::<_, host::Error>(HarnessesView::new(reply))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.host.epoch != key.0
                    || this.harnesses.epoch != Some(key.0)
                    || this.harnesses.serial != key.1
                {
                    return;
                }
                this.harnesses.busy = false;
                match result {
                    Ok(view) => {
                        this.harnesses.note = None;
                        this.harnesses.view = Some(Arc::new(view));
                        this.harnesses.generation += 1;
                    }
                    Err(error) => this.harnesses.note = Some(failure(&error).into()),
                }
                this.sync_composer(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask for an executable with the system file picker, then send it to the host.
    fn choose_harness_path(&mut self, id: HarnessId, name: SharedString, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(format!("Use this {name}").into()),
        });
        let epoch = self.host.epoch;
        cx.spawn(async move |this, cx| {
            let result = prompt.await;
            let _ = this.update(cx, |this, cx| {
                if this.host.epoch != epoch {
                    return;
                }
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            this.set_harness_path(id, Some(path), cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        this.harnesses.note =
                            Some(format!("Cannot open the file picker: {error}").into())
                    }
                    Err(error) => {
                        this.harnesses.note =
                            Some(format!("The file picker closed unexpectedly: {error}").into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Keep the composer line in step with the open thread and the list. Runs when
    /// either changes, never in render.
    pub(super) fn sync_thread_harness(&mut self) {
        let key = self
            .host
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.harness.clone())
            .filter(|harness| harness.id != HarnessId::Pi);
        let Some(key) = key else {
            self.harnesses.line = None;
            return;
        };
        let generation = self.harnesses.generation;
        if matches!(&self.harnesses.line, Some((line, made, _)) if *line == key && *made == generation)
        {
            return;
        }
        let row = self.harness_row(key.id);
        let name = row.map_or_else(|| key.id.label().into(), |row| row.name.clone());
        let label = match &key.version {
            Some(version) => format!("{name} {version}").into(),
            None => name,
        };
        let protection = row
            .map(|row| row.protection.clone())
            .filter(|protection| !protection.is_empty());
        let mode = row.and_then(|row| mode_line(row, &key));
        self.harnesses.line = Some((
            key,
            generation,
            ThreadLine {
                label,
                protection,
                mode,
            },
        ));
    }

    /// The open thread's harness line, when it runs in a harness other than Pi.
    pub(super) fn thread_harness_line(&self) -> Option<&ThreadLine> {
        let (key, _, line) = self.harnesses.line.as_ref()?;
        let snapshot = self.host.snapshot.as_ref()?;
        (snapshot.harness.as_ref() == Some(key)).then_some(line)
    }

    /// Settings → Agent: each harness, its state and version, and what protects the checkout.
    pub(super) fn harness_settings(&self, cx: &mut Context<Self>) -> Div {
        let usable = self.host.connected && self.host.protocol() >= 4;
        let busy = self.harnesses.busy;
        let refresh = toned_button(
            "harness-refresh",
            if busy { "Checking…" } else { "Refresh" },
            Tone::Filled,
        )
        .disabled(busy || !usable)
        .when(usable && !busy, |el| {
            el.on_activate(cx, |this, _, cx| this.refresh_harnesses(cx))
        });
        let mut page = settings::section("Agent harnesses").child(settings::setting(
            "What can run a thread",
            "Pi is built in. Codex runs through your own install, with its own sign-in and settings. Pick a model from any ready harness when you start a thread; the thread stays in that harness.",
            refresh,
        ));
        if !self.host.connected {
            return page.child(tag(
                "Connect the host to list agent harnesses.",
                colors().subtle,
            ));
        }
        if self.host.protocol() < 4 {
            return page.child(
                tag(
                    "This host runs every thread in Pi. Update the host to choose another harness.",
                    colors().subtle,
                )
                .line_height(px(16.)),
            );
        }
        if let Some(note) = &self.harnesses.note {
            page = page.child(tag(note.clone(), colors().amber).line_height(px(16.)));
        }
        let rows = self.harness_rows();
        if rows.is_empty() {
            return page.when(self.harnesses.note.is_none(), |el| {
                el.child(tag("Checking agent harnesses…", colors().subtle))
            });
        }
        let mut list = column().gap_4();
        for (index, row) in rows.iter().enumerate() {
            list = list.child(self.harness_entry(index, row, busy, cx));
        }
        page.child(list)
    }

    fn harness_entry(
        &self,
        index: usize,
        harness: &HarnessRow,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let tint = if harness.ready {
            colors().mint
        } else {
            colors().amber
        };
        let mut text = column()
            .flex_1()
            .min_w_0()
            .gap(px(2.))
            .child(
                row()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(colors().text))
                            .child(text!(id = ("harness-name", index), harness.name.clone())),
                    )
                    .child(
                        row()
                            .child(dot(tint))
                            .child(tag(harness.state.clone(), tint)),
                    ),
            )
            .child(tag(harness.version.clone(), colors().muted));
        if let Some(path) = &harness.path {
            text = text.child(tag(path.clone(), colors().subtle).truncate());
        }
        if let Some(note) = &harness.note {
            text = text.child(
                tag(
                    note.clone(),
                    if harness.ready {
                        colors().muted
                    } else {
                        colors().amber
                    },
                )
                .line_height(px(16.)),
            );
        }
        if !harness.protection.is_empty() {
            text =
                text.child(tag(harness.protection.clone(), colors().subtle).line_height(px(16.)));
        }
        let mut entry = row().items_start().gap_3().child(text);
        let modes = self.default_mode_setting(index, harness, busy, cx);
        if harness.has_path() {
            let (id, name) = (harness.id, harness.name.clone());
            entry = entry.child(
                row()
                    .flex_shrink_0()
                    .gap_1()
                    .child(
                        toned_button(("harness-choose", index), "Choose…", Tone::Ghost)
                            .aria_label(format!("Choose the {} executable", harness.name))
                            .disabled(busy)
                            .when(!busy, |el| {
                                el.on_activate(cx, move |this, _, cx| {
                                    this.choose_harness_path(id, name.clone(), cx)
                                })
                            }),
                    )
                    .child(
                        toned_button(("harness-path", index), "Use PATH", Tone::Ghost)
                            .aria_label(format!("Find {} on the login shell's PATH", harness.name))
                            .disabled(busy)
                            .when(!busy, |el| {
                                el.on_activate(cx, move |this, _, cx| {
                                    this.set_harness_path(id, None, cx)
                                })
                            }),
                    ),
            );
        }
        match modes {
            Some(modes) => column().gap_3().child(entry).child(modes),
            None => column().child(entry),
        }
    }

    /// The mode new threads in a harness with its own modes start in.
    fn default_mode_setting(
        &self,
        index: usize,
        harness: &HarnessRow,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if harness.modes.is_empty() {
            return None;
        }
        let mut choices = segmented();
        for (choice, mode) in harness.modes.iter().enumerate() {
            let selected = harness.default_mode.as_deref() == Some(mode.id.as_str());
            let (id, value) = (harness.id, mode.id.clone());
            choices = choices.child(
                toned_button(
                    SharedString::from(format!("harness-mode-{index}-{choice}")),
                    mode.label.clone(),
                    if selected {
                        Tone::Selected
                    } else {
                        Tone::Ghost
                    },
                )
                .h(px(24.))
                .role(Role::RadioButton)
                .aria_toggled(if selected {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .aria_label(format!(
                    "{} default permissions for new threads: {}",
                    harness.name, mode.label
                ))
                .aria_description(mode.detail.clone())
                .disabled(busy)
                .when(!busy && !selected, |el| {
                    el.on_activate(cx, move |this, _, cx| {
                        this.set_harness_mode(id, value.clone(), cx)
                    })
                }),
            );
        }
        Some(settings::setting(
            "Default permissions for new threads",
            harness.default_detail.clone(),
            choices,
        ))
    }
}

#[cfg(test)]
mod tests {
    // Named imports: a glob would bring in GPUI's own `test` attribute.
    use super::{
        HarnessId, Harnesses, HarnessesView, ThreadHarness, mode_line, new_thread_items,
        thread_items,
    };

    fn sample() -> Harnesses {
        serde_json::from_str(
            r#"{"harnesses":[
                {"id":"pi","name":"Pi","state":"ready","note":null,"version":null,"supported":null,"path":null,
                 "capabilities":{"steer":true,"approvals":true,"thinking":true,"usage":true},
                 "protection":"Plait's sandbox and approvals.","models":null},
                {"id":"codex","name":"Codex","state":"ready","note":"Newer than tested.","version":"0.170.0",
                 "supported":{"min":"0.169.0","tested":"0.169.0"},"path":"/opt/sample/bin/codex",
                 "capabilities":{"steer":true,"approvals":true,"thinking":true,"usage":true},
                 "protection":"Codex's sandbox.","models":[
                    {"provider":"codex","model_id":"a","name":"A","authenticated":true},
                    {"provider":"codex","model_id":"b","name":"B","authenticated":false}],
                 "modes":[{"id":"read_only","label":"Read only","detail":"Only reads."},
                          {"id":"ask","label":"Ask","detail":"Asks first."},
                          {"id":"full_access","label":"Full access","detail":"Never asks."}],
                 "default_mode":"full_access"},
                {"id":"someday","name":"Someday","state":"signed_out","note":"Sign in first.","version":null,
                 "supported":{"min":"1.0","tested":"1.2"},"path":null,
                 "capabilities":{"steer":false,"approvals":false,"thinking":false,"usage":false},
                 "protection":"","models":[]}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn strings_are_made_once_from_the_reply() {
        let view = HarnessesView::new(sample());
        let [pi, codex, other] = &view.rows[..] else {
            panic!("three rows")
        };
        assert_eq!(pi.version.as_ref(), "Built into the host");
        assert!(pi.models.is_empty() && pi.blocked.is_none() && !pi.has_path());
        assert_eq!(
            codex.version.as_ref(),
            "Version 0.170.0 · needs 0.169.0 or newer · tested with 0.169.0"
        );
        assert_eq!(codex.path.as_deref(), Some("/opt/sample/bin/codex"));
        assert!(codex.ready && codex.has_path());
        assert_eq!(
            other.version.as_ref(),
            "Needs 1.0 or newer · tested with 1.2"
        );
        assert_eq!(
            other.blocked.as_deref(),
            Some("Someday: signed out. Sign in first.")
        );
        assert!(!other.has_path());
    }

    #[test]
    fn the_new_thread_picker_adds_ready_models_and_one_row_per_unready_harness() {
        let view = HarnessesView::new(sample());
        let items = new_thread_items(&view.rows);
        let listed = items
            .iter()
            .map(|item| {
                (
                    item.section.to_string(),
                    item.title.to_string(),
                    item.enabled,
                    item.badge.as_ref().map(|(badge, _)| badge.to_string()),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            listed,
            vec![
                ("Codex".into(), "A".into(), true, None),
                (
                    "Codex".into(),
                    "B".into(),
                    false,
                    Some("Not signed in".into())
                ),
                (
                    "Someday".into(),
                    "Someday".into(),
                    false,
                    Some("Signed out".into())
                ),
            ]
        );
        // No favorite toggle or Set default: those are Pi's.
        assert!(
            items
                .iter()
                .all(|item| item.marked.is_none() && item.aside.is_none())
        );
        assert_eq!(items[2].detail.as_ref(), "Sign in first.");
        // A Codex thread's menu lists only Codex models.
        assert_eq!(thread_items(&view.rows[1]).len(), 2);
        assert!(thread_items(&view.rows[0]).is_empty());
    }

    fn thread(mode: Option<&str>, reported: Option<&str>) -> ThreadHarness {
        ThreadHarness {
            id: HarnessId::Codex,
            version: None,
            mode: mode.map(Into::into),
            reported: reported.map(Into::into),
        }
    }

    #[test]
    fn the_mode_line_names_the_choice_and_says_when_the_harness_used_another() {
        let view = HarnessesView::new(sample());
        let (pi, codex) = (&view.rows[0], &view.rows[1]);
        assert_eq!(codex.default_detail.as_ref(), "Never asks.");
        // Pi uses Plait's approvals, so it has no mode line.
        assert!(mode_line(pi, &thread(None, None)).is_none());
        // Before the first run, the chosen mode says what it does.
        let line = mode_line(codex, &thread(Some("ask"), None)).unwrap();
        assert_eq!((line.id.as_str(), line.label.as_ref()), ("ask", "Ask"));
        assert_eq!(
            (line.note.as_ref(), line.overridden),
            ("Asks first.", false)
        );
        assert!(
            line.tip
                .starts_with("Codex's sandbox. Choose what Codex may do")
        );
        // A thread without a mode yet shows the harness default.
        let line = mode_line(codex, &thread(None, None)).unwrap();
        assert_eq!(line.label.as_ref(), "Full access");
        // The run used what was chosen.
        let line = mode_line(codex, &thread(Some("ask"), Some("ask"))).unwrap();
        assert!(!line.overridden);
        // Codex's configuration ran another mode.
        let line = mode_line(codex, &thread(Some("ask"), Some("read_only"))).unwrap();
        assert!(line.overridden);
        assert_eq!(
            line.note.as_ref(),
            "Codex is using Read only instead of Ask, set by your Codex configuration. Only reads."
        );
        let line = mode_line(codex, &thread(Some("full_access"), Some("custom"))).unwrap();
        assert_eq!(
            line.note.as_ref(),
            "Codex is using permissions its own configuration sets, not Full access."
        );
    }
}
