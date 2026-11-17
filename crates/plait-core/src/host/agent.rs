//! Client side of `agent_resources`: what the person's Pi setup offers runs in
//! one checkout. docs/HOST.md "Skills and user instructions" is the contract.
//!
//! The reply names files and never holds their contents. New fields may be
//! added, so unknown ones are ignored; an unknown source reads as `Other`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Hosts from before this operation reject it as `invalid_request`.
    AgentResources {
        checkout: PathBuf,
        protocol_version: u32,
    },
}

impl Request {
    pub fn agent_resources(checkout: PathBuf) -> Self {
        Self::AgentResources {
            checkout,
            protocol_version: 3,
        }
    }
}

/// Where a skill or template came from, as Pi resolved it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The Pi agent folder or `~/.agents`.
    User,
    /// The checkout, offered only when the project is trusted.
    Project,
    /// An installed Pi package.
    Package,
    /// A path named in Pi's settings.
    Path,
    #[serde(other)]
    Other,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "Your agent",
            Self::Project => "Project",
            Self::Package => "Package",
            Self::Path => "Settings path",
            Self::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Instructions {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub origin: Origin,
    pub path: PathBuf,
    /// False for a skill only `/skill:name` loads; the model is not offered it.
    pub model_invocation: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
    pub origin: Origin,
    pub path: PathBuf,
}

/// Something Pi or Plait could not use, with the file it concerns when there is one.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Problem {
    pub path: Option<PathBuf>,
    pub message: String,
}

/// What a run in `checkout` would be offered now, under its current policy.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct AgentResources {
    pub checkout: PathBuf,
    /// Project skills and templates load only for a trusted project.
    pub trusted: bool,
    pub instructions: Option<Instructions>,
    pub skills: Vec<Skill>,
    pub templates: Vec<Template>,
    pub problems: Vec<Problem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_names_the_checkout_under_version_3() {
        assert_eq!(
            serde_json::to_value(Request::agent_resources("/repo".into())).unwrap(),
            serde_json::json!({"op": "agent_resources", "checkout": "/repo", "protocol_version": 3})
        );
    }

    #[test]
    fn a_reply_decodes_and_tolerates_later_additions() {
        let reply: AgentResources = serde_json::from_str(
            r#"{"checkout":"/repo","trusted":false,
                "instructions":{"path":"/a/AGENTS.md","bytes":12},
                "skills":[{"name":"notes","description":"Write notes.","origin":"user","path":"/a/skills/notes/SKILL.md","model_invocation":false,"added":1}],
                "templates":[{"name":"review","description":"","argument_hint":null,"origin":"somewhere-new","path":"/a/prompts/review.md"}],
                "problems":[{"path":null,"message":"Only the first 500 of 501 skills are listed."}],
                "later":true}"#,
        )
        .unwrap();
        assert_eq!(reply.instructions.unwrap().bytes, 12);
        assert!(!reply.skills[0].model_invocation);
        assert_eq!(reply.skills[0].origin, Origin::User);
        assert_eq!(reply.templates[0].origin, Origin::Other);
        assert_eq!(reply.problems[0].path, None);
    }
}
