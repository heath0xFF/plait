//! A project's settings → Your Pi agent: what the person's Pi setup offers
//! threads in the open checkout, read from the host with the same discovery a
//! run makes.
//!
//! Turning a skill or template off stays in Pi's own settings,
//! which Plait follows, so there is one place to manage them. Every string the
//! page and the composer draw is made once, on the background executor, when
//! the reply arrives; rendering formats nothing.

use super::{Plait, composer::AgentCommand, settings, theme::*, thread_controls::TextTooltip};
use gpui::{prelude::*, *};
use plait_core::host::{
    agent::{AgentResources, McpServer, McpTransport, Request},
    providers,
};
use std::{path::PathBuf, sync::Arc};

/// Rows each list shows before Show all.
const FOLDED: usize = 12;
/// Characters of a description drawn in one line, here and in the composer.
const DETAIL: usize = 140;

#[derive(Default)]
pub(super) struct AgentState {
    pub(super) key: Option<(u64, PathBuf)>,
    pub(super) view: Option<Arc<AgentView>>,
    busy: bool,
    pub(super) revision: u64,
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
    pub(super) mcp_servers: Vec<McpRow>,
    pub(super) commands: Arc<[AgentCommand]>,
}

pub(super) struct McpRow {
    pub(super) name: SharedString,
    pub(super) state: &'static str,
    transport: &'static str,
    pub(super) hover: SharedString,
    label: SharedString,
    pub(super) enabled: bool,
    can_enable: bool,
}

impl From<McpServer> for McpRow {
    fn from(server: McpServer) -> Self {
        let state = if server.problem.is_some() {
            "Invalid"
        } else if server.needs_sign_in == Some(true) && !server.signed_in {
            "Sign in with Pi"
        } else if server.signed_in {
            "Sign-in saved"
        } else if server.enabled {
            "Listed"
        } else {
            "Off"
        };
        let mut hover = vec![server.source.to_string_lossy().into_owned()];
        if let Some(source) = &server.override_source {
            hover.push(format!("Project override: {}", source.display()));
        }
        if let Some(target) = server.program.as_ref().or(server.host.as_ref()) {
            hover.push(target.clone());
        }
        if let Some(reason) = server.disabled_reason.as_ref().or(server.problem.as_ref()) {
            hover.push(reason.clone());
        }
        if server.needs_sign_in == Some(true) && !server.signed_in {
            hover.push("Sign in with Pi, then Refresh here.".into());
        } else if server.signed_in {
            hover.push("Pi holds a saved sign-in; its validity has not been checked.".into());
        } else if server.needs_sign_in.is_none() && server.problem.is_none() {
            hover.push("Whether this server needs sign-in is unknown until it connects.".into());
        }
        Self {
            label: format!(
                "{}: {} for this project",
                server.name,
                if server.enabled { "On" } else { "Off" }
            )
            .into(),
            name: server.name.into(),
            state,
            transport: match server.transport {
                Some(McpTransport::Stdio) => "stdio",
                Some(McpTransport::Http) => "HTTP",
                _ => "Unknown",
            },
            hover: hover.join("\n").into(),
            enabled: server.enabled,
            can_enable: server.can_enable,
        }
    }
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
            mcp_servers: reply.mcp_servers.into_iter().map(McpRow::from).collect(),
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

    /// Read the setup once per host connection and checkout. Opening the
    /// project's settings, or Refresh there, reads it again.
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
            revision: self.agent.revision.wrapping_add(1),
            expanded: self.agent.expanded,
            empty: self.agent.empty.clone(),
            ..Default::default()
        };
        self.refresh_agent(cx);
    }

    /// Read the setup again: Pi's files can change while the app is open.
    pub(super) fn refresh_agent(&mut self, cx: &mut Context<Self>) {
        self.request_agent(None, cx);
    }

    fn request_agent(&mut self, change: Option<(String, bool)>, cx: &mut Context<Self>) {
        if self.agent.busy || !self.host.connected || cfg!(test) {
            return;
        }
        self.agent.busy = true;
        self.agent.revision = self.agent.revision.wrapping_add(1);
        let revision = self.agent.revision;
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
            let request = match change {
                Some((name, enabled)) => Request::set_mcp_server(canonical.clone(), name, enabled),
                None => Request::agent_resources(canonical.clone()),
            };
            let reply: AgentResources =
                plait_core::host::Client::from_environment()?.send(&request)?;
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
                if !this.accept_agent(&key, revision, result) {
                    return;
                }
                this.sync_composer(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// A refresh or saved switch must still belong to the same connection and checkout.
    pub(super) fn accept_agent(
        &mut self,
        key: &(u64, PathBuf),
        revision: u64,
        result: Result<AgentView, plait_core::host::Error>,
    ) -> bool {
        if self.agent.revision != revision
            || self.agent.key.as_ref() != Some(key)
            || self.host.epoch != key.0
            || self.host.checkout != key.1
        {
            return false;
        }
        self.agent.busy = false;
        match result {
            Ok(view) => self.agent.view = Some(Arc::new(view)),
            Err(error) => {
                self.agent.note = Some(if providers::needs_host_update(&error) {
                    "Update the host to manage your Pi setup.".into()
                } else {
                    providers::refusal(&error)
                })
            }
        }
        true
    }

    /// Trust changes which project skills and templates load, so read them again.
    pub(super) fn forget_agent(&mut self) {
        self.agent.key = None;
        self.agent.revision = self.agent.revision.wrapping_add(1);
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
            "Skills, prompt templates, instructions, and MCP servers from your Pi setup. Skills and templates follow Pi's settings.",
            refresh,
        ));
        if let Some(note) = &self.agent.note {
            page = page.child(tag(note.clone(), colors().amber).line_height(px(16.)));
        }
        let Some(view) = self.agent.view.as_ref() else {
            let page = page.when(self.agent.note.is_none(), |el| {
                el.child(tag(
                    if self.host.connected {
                        "Reading your agent setup…"
                    } else {
                        "Connect the host to read your agent setup."
                    },
                    colors().subtle,
                ))
            });
            return page;
        };
        if !view.trusted {
            page = page.child(tag(
                "Skills and templates inside this project load only when its commands are trusted, above.",
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
            .when(!view.mcp_servers.is_empty(), |el| {
                el.child(self.mcp_settings(&view.mcp_servers, cx))
            })
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

    fn mcp_settings(&self, servers: &[McpRow], cx: &mut Context<Self>) -> Div {
        let mut section = settings::section("MCP servers")
            // Remove with the stage that connects servers and offers their tools to Pi.
            .child(tag(
                "Servers are listed but not yet used by threads.",
                colors().subtle,
            ));
        let all = self.agent.expanded || servers.len() <= FOLDED;
        for (index, server) in servers
            .iter()
            .take(if all { servers.len() } else { FOLDED })
            .enumerate()
        {
            let hover = server.hover.clone();
            let name = server.name.clone();
            let enabled = !server.enabled;
            let available = server.can_enable
                && self.host.connected
                && !self.agent.busy
                && self.agent.key.as_ref() == Some(&(self.host.epoch, self.host.checkout.clone()));
            section = section.child(
                row()
                    .id(("mcp-server", index))
                    .min_w_0()
                    .gap_2()
                    .h(px(28.))
                    .tooltip(move |_, cx| cx.new(|_| TextTooltip(hover.to_string())).into())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(server.name.clone()),
                    )
                    .child(tag(server.transport, colors().subtle).flex_shrink_0())
                    .child(tag(server.state, colors().muted).flex_shrink_0())
                    .child(
                        chip(
                            ("mcp-switch", index),
                            if server.enabled { "On" } else { "Off" },
                            server.enabled,
                        )
                        .role(Role::Switch)
                        .aria_label(server.label.clone())
                        .disabled(!available)
                        .when(available, |el| {
                            el.on_activate(cx, move |this, _, cx| {
                                this.request_agent(Some((name.to_string(), enabled)), cx);
                            })
                        }),
                    ),
            );
        }
        section.when(!all, |el| {
            el.child(
                button("mcp-show-all", format!("Show all {}", servers.len())).on_activate(
                    cx,
                    |this, _, cx| {
                        this.agent.expanded = true;
                        cx.notify();
                    },
                ),
            )
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
