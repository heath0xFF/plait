//! Additive schema-1 draft identities. Legacy fields remain complete snapshots.
use super::{Draft, Session, identifier};
use crate::host::{ApprovalMode, ModelChoice, ThinkingLevel, harness::HarnessId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DraftSettings {
    pub harness: Option<HarnessId>,
    pub model: Option<ModelChoice>,
    pub mode: Option<String>,
    pub thinking: Option<ThinkingLevel>,
    pub approval: Option<ApprovalMode>,
}

/// A first send that is never replayed on launch; explicit retry reuses this ID.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DraftCreation {
    pub request_id: String,
    pub placement: crate::host::admission::Placement,
    pub harness: HarnessId,
    pub model: ModelChoice,
    pub mode: Option<String>,
    pub thinking: Option<ThinkingLevel>,
    pub approval: ApprovalMode,
    pub message_approval: Option<ApprovalMode>,
    pub thread_approval: Option<ApprovalMode>,
    pub message: Draft,
    pub needs_choice: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DraftTab {
    /// Owns this checkout's backwards-compatible draft snapshot.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legacy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation: Option<DraftCreation>,
    pub checkout: PathBuf,
    pub draft: Draft,
    #[serde(default)]
    pub settings: DraftSettings,
}

impl Session {
    pub fn draft_tab(&self, id: &str, checkout: &Path) -> Option<&DraftTab> {
        self.draft_tabs
            .get(id)
            .filter(|tab| tab.checkout == checkout)
    }

    pub fn set_draft_tab(&mut self, id: String, mut tab: DraftTab) {
        tab.legacy = self.draft_tabs.get(&id).is_some_and(|old| old.legacy)
            || !self
                .draft_tabs
                .values()
                .any(|old| old.checkout == tab.checkout && old.legacy);
        if tab.legacy {
            self.set_new_draft(tab.checkout.clone(), tab.draft.clone());
        }
        self.draft_tabs.insert(id, tab);
    }

    pub fn clear_draft_tab(&mut self, id: &str) {
        let Some(tab) = self.draft_tabs.remove(id) else {
            return;
        };
        if !tab.legacy {
            return;
        }
        // Keep an intact nonempty sibling visible to an older build.
        self.clear_new_draft(&tab.checkout);
        let next = self
            .draft_tabs
            .iter()
            .filter(|(_, other)| other.checkout == tab.checkout)
            .min_by_key(|(_, other)| other.draft.text.is_empty() && other.draft.images.is_empty())
            .map(|(id, _)| id.clone());
        if let Some(id) = next {
            let other = self.draft_tabs.get_mut(&id).unwrap();
            other.legacy = true;
            let other = other.clone();
            self.set_new_draft(other.checkout, other.draft);
        }
    }

    /// A downgrade may edit only a legacy field. Import that version as another
    /// draft rather than choosing one version and losing the other person's text.
    pub fn import_legacy_drafts(&mut self) {
        let legacy: Vec<_> = self
            .new_draft
            .iter()
            .map(|(p, d)| (p.clone(), d.clone()))
            .chain(self.new_drafts.iter().map(|(p, d)| (p.clone(), d.clone())))
            .collect();
        for (checkout, draft) in legacy {
            if self
                .draft_tabs
                .values()
                .any(|tab| tab.checkout == checkout && tab.draft == draft)
            {
                continue;
            }
            let mut n = 0;
            let id = loop {
                let id = format!("legacy-{n}");
                if !self.draft_tabs.contains_key(&id) {
                    break id;
                }
                n += 1;
            };
            for tab in self
                .draft_tabs
                .values_mut()
                .filter(|t| t.checkout == checkout)
            {
                tab.legacy = false;
            }
            self.draft_tabs.insert(
                id,
                DraftTab {
                    legacy: true,
                    creation: None,
                    checkout,
                    draft,
                    settings: DraftSettings::default(),
                },
            );
        }
    }

    pub(super) fn validate_draft_tabs(&self) -> Result<(), String> {
        if self.draft_tabs.len() > 10_000 {
            return Err("Saved workspace exceeds 10,000 drafts. Nothing was discarded.".into());
        }
        for (id, tab) in &self.draft_tabs {
            if !identifier(id)
                || !tab.checkout.is_absolute()
                || tab.checkout.as_os_str().len() > 4096
                || tab.draft.text.len() > 256 * 1024
                || tab
                    .draft
                    .submitted
                    .as_ref()
                    .is_some_and(|(text, _)| text.len() > crate::host::REQUEST_LIMIT)
                || tab
                    .settings
                    .mode
                    .as_ref()
                    .is_some_and(|mode| mode.len() > 128)
                || tab
                    .settings
                    .model
                    .as_ref()
                    .is_some_and(|m| m.provider.len() > 1024 || m.model_id.len() > 1024)
            {
                return Err(
                    "Saved new-thread draft exceeds its limits. Nothing was discarded.".into(),
                );
            }
            crate::images::validate_message(&tab.draft.images)?;
            if let Some(creation) = &tab.creation {
                if !identifier(&creation.request_id)
                    || creation.message.text.len() > 256 * 1024
                    || creation.model.provider.len() > 1024
                    || creation.model.model_id.len() > 1024
                    || creation.mode.as_ref().is_some_and(|mode| mode.len() > 128)
                    || (creation.message.text.trim().is_empty()
                        && creation.message.images.is_empty())
                    || creation.harness == HarnessId::Other
                {
                    return Err("Saved draft creation is invalid. It was not sent.".into());
                }
                crate::images::validate_message(&creation.message.images)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // The schema-1 reader before independent drafts: no knowledge of draft_tabs.
    #[derive(Deserialize)]
    struct OldSession {
        #[serde(default)]
        new_draft: Option<(PathBuf, Draft)>,
        #[serde(default)]
        new_drafts: BTreeMap<PathBuf, Draft>,
        schema_version: u32,
        host_store: PathBuf,
        checkout: Option<PathBuf>,
        selected: Option<String>,
        threads: BTreeMap<String, super::super::Thread>,
    }
    impl OldSession {
        fn decode(bytes: &[u8]) -> Result<Self, String> {
            if bytes.len() > super::super::FILE_LIMIT {
                return Err("too large".into());
            }
            let old: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            if old.schema_version != 1
                || !old.host_store.is_absolute()
                || old.checkout.as_ref().is_some_and(|p| !p.is_absolute())
                || old.threads.len() + old.new_drafts.len() > 512
            {
                return Err("invalid workspace".into());
            }
            for (path, draft) in old
                .new_draft
                .iter()
                .map(|(p, d)| (p, d))
                .chain(old.new_drafts.iter())
            {
                if !path.is_absolute() || draft.text.len() > 256 * 1024 {
                    return Err("invalid draft".into());
                }
                crate::images::validate_message(&draft.images)?;
            }
            Ok(old)
        }
    }
    fn tab(text: &str) -> DraftTab {
        DraftTab {
            legacy: false,
            creation: None,
            checkout: "/project".into(),
            draft: Draft {
                text: text.into(),
                revision: 4,
                ..Default::default()
            },
            settings: DraftSettings::default(),
        }
    }
    #[test]
    fn additive_shape_keeps_old_reader_and_imports_old_and_downgrade_edits() {
        let mut session = Session::empty("/host".into());
        session.set_new_draft("/project".into(), tab("legacy").draft);
        let old_bytes = serde_json::to_vec(&session).unwrap();
        let mut upgraded: Session = serde_json::from_slice(&old_bytes).unwrap();
        upgraded.import_legacy_drafts();
        assert_eq!(upgraded.draft_tabs.len(), 1);
        assert_eq!(upgraded.draft_tabs["legacy-0"].draft.text, "legacy");
        upgraded.set_draft_tab("second".into(), tab("second message"));
        let bytes = serde_json::to_vec(&upgraded).unwrap();
        let old = OldSession::decode(&bytes).unwrap();
        assert_eq!(old.new_draft.unwrap().1.text, "legacy");
        assert!(old.selected.is_none());
        assert!(old.threads.is_empty());
        let mut restored: Session = serde_json::from_slice(&bytes).unwrap();
        restored.import_legacy_drafts();
        assert_eq!(restored, upgraded, "mirrors must not duplicate drafts");
        restored.new_draft.as_mut().unwrap().1.text = "edited by an older build".into();
        restored.import_legacy_drafts();
        assert_eq!(restored.draft_tabs.len(), 3);
        assert!(
            restored
                .draft_tabs
                .values()
                .any(|t| t.draft.text == "second message")
        );
        assert!(
            restored
                .draft_tabs
                .values()
                .any(|t| t.draft.text == "edited by an older build")
        );
    }
    #[test]
    fn clearing_one_draft_keeps_its_siblings_and_legacy_mirror() {
        let mut session = Session::empty("/host".into());
        session.set_draft_tab("a".into(), tab("first"));
        session.set_draft_tab("b".into(), tab("second"));
        session.clear_draft_tab("b");
        assert_eq!(
            session.new_draft_for(Path::new("/project")).unwrap().text,
            "first"
        );
        assert_eq!(session.draft_tabs.len(), 1);
        session.clear_draft_tab("a");
        assert!(session.new_draft.is_none() && session.new_drafts.is_empty());
    }
    #[test]
    fn malformed_additions_fail_totally_without_touching_legacy_text() {
        let mut session = Session::empty("/host".into());
        session.set_draft_tab("a".into(), tab("saved text"));
        let bytes = serde_json::to_vec(&session).unwrap();
        for end in 0..bytes.len() {
            assert!(serde_json::from_slice::<Session>(&bytes[..end]).is_err());
        }
        let mut value = serde_json::to_value(&session).unwrap();
        for invalid in [
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!({"a":false}),
        ] {
            value["draft_tabs"] = invalid;
            assert!(serde_json::from_value::<Session>(value.clone()).is_err());
            assert_eq!(
                OldSession::decode(&serde_json::to_vec(&value).unwrap())
                    .unwrap()
                    .new_draft
                    .unwrap()
                    .1
                    .text,
                "saved text"
            );
        }
    }
    fn isolated(name: &str) -> bool {
        if std::env::var("PLAIT_DRAFT_JOURNAL_TEST").ok().as_deref() == Some(name) {
            return true;
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("PLAIT_DRAFT_JOURNAL_TEST", name)
            .status()
            .unwrap();
        assert!(status.success());
        false
    }
    #[test]
    fn full_journal_and_interrupted_write_keep_previous_drafts_readable() {
        if !isolated(
            "session::drafts::tests::full_journal_and_interrupted_write_keep_previous_drafts_readable",
        ) {
            return;
        }
        let root = std::env::temp_dir().join(format!("plait-many-journal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (journal, mut session) = super::super::Journal::open(&root, "/host".into()).unwrap();
        for n in 0..300 {
            session.set_draft_tab(format!("draft-{n}"), tab(&format!("Message {n}")));
        }
        journal.queue(session.clone());
        journal.flush().unwrap();
        let good = std::fs::read(root.join("desktop.json")).unwrap();
        assert!(OldSession::decode(&good).is_ok());
        for n in 0..20 {
            session.set_draft_tab(format!("large-{n}"), tab(&"x".repeat(256 * 1024)));
        }
        journal.queue(session);
        assert!(journal.flush().unwrap_err().contains("4 MiB"));
        assert_eq!(std::fs::read(root.join("desktop.json")).unwrap(), good);
        std::fs::write(root.join(".desktop.interrupted.tmp"), b"{\"draft_tabs\":").unwrap();
        drop(journal);
        let (_, restored) = super::super::Journal::open(&root, "/host".into()).unwrap();
        assert_eq!(restored.draft_tabs.len(), 300);
        assert_eq!(restored.draft_tabs["draft-299"].draft.text, "Message 299");
        std::fs::remove_dir_all(root).unwrap();
    }
}
