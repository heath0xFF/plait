//! Settings → Agent: what the person's Pi setup offers threads in the open
//! checkout, read from the host with the same discovery a run makes.
//!
//! Read-only. Turning a skill or template off stays in Pi's own settings,
//! which Plait follows, so there is one place to manage them. Every string the
//! page and the composer draw is made once, on the background executor, when
//! the reply arrives; rendering formats nothing.

use super::{Plait, composer::AgentCommand, settings, theme::*};
use gpui::{prelude::*, *};
use plait_core::host::{
    agent::{AgentResources, Request},
    providers,
};
use std::{path::PathBuf, sync::Arc};

/// Rows each list shows before Show all.
const FOLDED: usize = 12;
/// Characters of a description drawn in one line, here and in the composer.
const DETAIL: usize = 140;

#[derive(Default)]
pub(super) struct AgentState {
    key: Option<(u64, PathBuf)>,
    pub(super) view: Option<Arc<AgentView>>,
    busy: bool,
    note: Option<String>,
    expanded: bool,
    /// Given to composers before the setup is read, so an unchanged empty list costs nothing.
    empty: Arc<[AgentCommand]>,
}

/// One line of a list: what it is, what it says, where it came from, and its file.
pub(super) struct Row {
    title: SharedString,
    detail: SharedString,
    tag: SharedString,
    path: SharedString,
    folder: Option<PathBuf>,
}

pub(super) struct AgentView {
    trusted: bool,
    instructions: Option<Row>,
    skills: Vec<Row>,
    templates: Vec<Row>,
    problems: Vec<Row>,
    pub(super) commands: Arc<[AgentCommand]>,
}

/// The first line, cut to `DETAIL` characters.
fn line(text: &str) -> String {
    let first = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    match first.char_indices().nth(DETAIL) {
        Some((cut, _)) => format!("{}…", &first[..cut]),
        None => first.to_owned(),
    }
}

fn size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} bytes")
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.)
    }
}

impl AgentView {
    pub(super) fn new(reply: AgentResources) -> Self {
        let path = |path: &std::path::Path| SharedString::from(home_relative(path));
        let folder = |path: &std::path::Path| path.parent().map(std::path::Path::to_path_buf);
        let skills = reply.skills.iter().map(|skill| Row {
            title: format!("/skill:{}", skill.name).into(),
            detail: line(&skill.description).into(),
            tag: if skill.model_invocation {
                skill.origin.label().into()
            } else {
                format!("{} · only when you type it", skill.origin.label()).into()
            },
            path: path(&skill.path),
            folder: folder(&skill.path),
        });
        let templates = reply.templates.iter().map(|template| Row {
            title: match &template.argument_hint {
                Some(hint) => format!("/{} {hint}", template.name).into(),
                None => format!("/{}", template.name).into(),
            },
            detail: line(&template.description).into(),
            tag: template.origin.label().into(),
            path: path(&template.path),
            folder: folder(&template.path),
        });
        let problems = reply.problems.iter().map(|problem| Row {
            title: problem.message.clone().into(),
            detail: SharedString::default(),
            tag: SharedString::default(),
            path: problem.path.as_deref().map(path).unwrap_or_default(),
            folder: problem.path.as_deref().and_then(folder),
        });
        // A name with a space can never be typed as one command, so it is not offered.
        let commands = reply
            .skills
            .iter()
            .map(|skill| (format!("skill:{}", skill.name), line(&skill.description)))
            .chain(reply.templates.iter().map(|template| {
                let detail = match &template.argument_hint {
                    Some(hint) => line(&format!("{hint} · {}", template.description)),
                    None => line(&template.description),
                };
                (template.name.clone(), detail)
            }))
            .filter(|(name, _)| !name.contains(char::is_whitespace))
            .map(|(name, detail)| AgentCommand::new(name, detail))
            .collect();
        Self {
            trusted: reply.trusted,
            instructions: reply.instructions.as_ref().map(|file| Row {
                title: "User instructions".into(),
                detail: size(file.bytes).into(),
                tag: SharedString::default(),
                path: path(&file.path),
                folder: folder(&file.path),
            }),
            skills: skills.collect(),
            templates: templates.collect(),
            problems: problems.collect(),
            commands,
        }
    }
}

impl Plait {
    /// The open checkout's skills and templates for the composer; none until read.
    pub(super) fn agent_commands(&self) -> Arc<[AgentCommand]> {
        match &self.agent.view {
            Some(view)
                if self.agent.key.as_ref()
                    == Some(&(self.host.epoch, self.host.checkout.clone())) =>
            {
                view.commands.clone()
            }
            _ => self.agent.empty.clone(),
        }
    }

    /// Read the setup once per host connection and checkout. Settings refreshes it on demand.
    pub(super) fn sync_agent(&mut self, cx: &mut Context<Self>) {
        if !self.host.connected || self.state.repository.snapshot.ready().is_none() || cfg!(test) {
            return;
        }
        let key = (self.host.epoch, self.host.checkout.clone());
        if self.agent.key.as_ref() == Some(&key) {
            return;
        }
        self.agent = AgentState {
            key: Some(key),
            expanded: self.agent.expanded,
            empty: self.agent.empty.clone(),
            ..Default::default()
        };
        self.refresh_agent(cx);
    }

    /// Read the setup again: Pi's files can change while the app is open.
    pub(super) fn refresh_agent(&mut self, cx: &mut Context<Self>) {
        if self.agent.busy || !self.host.connected || cfg!(test) {
            return;
        }
        self.agent.busy = true;
        self.agent.note = None;
        let key = (self.host.epoch, self.host.checkout.clone());
        let checkout = self
            .state
            .repository
            .snapshot
            .ready()
            .map_or_else(|| key.1.clone(), |snapshot| snapshot.root.clone());
        let task = cx.background_executor().spawn(async move {
            let canonical = std::fs::canonicalize(&checkout)?;
            let reply: AgentResources = plait_core::host::Client::from_environment()?
                .send(&Request::agent_resources(canonical.clone()))?;
            if reply.checkout != canonical {
                return Err(plait_core::host::Error::Protocol(
                    "Host returned another checkout's agent setup.".into(),
                ));
            }
            Ok(AgentView::new(reply))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.agent.key.as_ref() != Some(&key)
                    || this.host.epoch != key.0
                    || this.host.checkout != key.1
                {
                    return;
                }
                this.agent.busy = false;
                match result {
                    Ok(view) => this.agent.view = Some(Arc::new(view)),
                    Err(error) => {
                        this.agent.note = Some(if providers::needs_host_update(&error) {
                            "Update the host to list your agent's skills and templates.".into()
                        } else {
                            providers::refusal(&error)
                        })
                    }
                }
                this.sync_composer(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Trust changes which project skills and templates load, so read them again.
    pub(super) fn forget_agent(&mut self) {
        self.agent.key = None;
    }

    pub(super) fn agent_settings(&self, cx: &mut Context<Self>) -> Div {
        let refresh = toned_button(
            "agent-refresh",
            if self.agent.busy {
                "Reading…"
            } else {
                "Refresh"
            },
            Tone::Filled,
        )
        .disabled(self.agent.busy || !self.host.connected)
        .when(!self.agent.busy && self.host.connected, |el| {
            el.on_activate(cx, |this, _, cx| this.refresh_agent(cx))
        });
        let mut page = settings::section("Your Pi agent").child(settings::setting(
            "What threads here are offered",
            "Skills, prompt templates, and instructions from your Pi setup, found the way Pi finds them. Turn one off in Pi's settings; Plait follows them.",
            refresh,
        ));
        if let Some(note) = &self.agent.note {
            page = page.child(tag(note.clone(), colors().amber).line_height(px(16.)));
        }
        let Some(view) = self.agent.view.as_ref() else {
            return page.when(self.agent.note.is_none(), |el| {
                el.child(tag(
                    if self.host.connected {
                        "Reading your agent setup…"
                    } else {
                        "Connect the host to read your agent setup."
                    },
                    colors().subtle,
                ))
            });
        };
        if !view.trusted {
            page = page.child(tag(
                "Skills and templates inside this project load only when its commands are trusted (Approvals).",
                colors().subtle,
            ).line_height(px(16.)));
        }
        let instructions = match &view.instructions {
            Some(row) => self.rows("agent-instructions", std::slice::from_ref(row), true, cx),
            None => column().child(tag(
                "No AGENTS.md or CLAUDE.md in the Pi agent folder.",
                colors().subtle,
            )),
        };
        column()
            .gap_4()
            .child(page)
            .child(settings::section("Instructions").child(instructions))
            .child(self.list(
                "Skills",
                "agent-skills",
                &view.skills,
                "No skills found.",
                cx,
            ))
            .child(self.list(
                "Prompt templates",
                "agent-templates",
                &view.templates,
                "No prompt templates found.",
                cx,
            ))
            .when(!view.problems.is_empty(), |el| {
                el.child(self.list("Problems", "agent-problems", &view.problems, "", cx))
            })
    }

    fn list(
        &self,
        title: &'static str,
        id: &'static str,
        rows: &[Row],
        empty: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let all = self.agent.expanded || rows.len() <= FOLDED;
        let shown = if all { rows } else { &rows[..FOLDED] };
        settings::section(title)
            .when(rows.is_empty(), |el| el.child(tag(empty, colors().subtle)))
            .child(self.rows(id, shown, false, cx))
            .when(!all, |el| {
                el.child(div().child(
                    button((id, usize::MAX), format!("Show all {}", rows.len())).on_activate(
                        cx,
                        |this, _, cx| {
                            this.agent.expanded = true;
                            cx.notify();
                        },
                    ),
                ))
            })
    }

    fn rows(&self, id: &'static str, rows: &[Row], quiet: bool, cx: &mut Context<Self>) -> Div {
        let mut list = column().gap_3();
        for (index, item) in rows.iter().enumerate() {
            let open = item.folder.clone();
            let mut text = column().flex_1().min_w_0().gap(px(2.)).child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(if quiet { colors().muted } else { colors().text }))
                    .child(text!(id = (id, index), item.title.clone())),
            );
            if !item.detail.is_empty() {
                text = text.child(tag(item.detail.clone(), colors().muted).truncate());
            }
            let mut place = row().gap_2().min_w_0();
            if !item.tag.is_empty() {
                place = place.child(tag(item.tag.clone(), colors().subtle).flex_shrink_0());
            }
            if !item.path.is_empty() {
                place = place.child(tag(item.path.clone(), colors().subtle).truncate());
            }
            list = list.child(
                row()
                    .items_start()
                    .gap_3()
                    .child(text.child(place))
                    .when_some(open, |el, folder| {
                        el.child(
                            toned_button((id, index), "Open folder", Tone::Ghost)
                                .flex_shrink_0()
                                .on_activate(cx, move |_, _, cx| cx.open_with_system(&folder)),
                        )
                    }),
            );
        }
        list
    }
}
