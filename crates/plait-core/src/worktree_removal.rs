//! Manual removal keeps the confirmed owner and request through a lost reply.
use crate::{
    host::worktree::{Checkout, Operation, Request, Stage},
    tabs::HostKey,
};
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub host: HostKey,
    pub path: PathBuf,
    pub request: Request,
}
#[derive(Default)]
pub struct Removal {
    pub asking: Option<Attempt>,
    pub pending: Option<Attempt>,
    pub message: Option<String>,
}
impl Removal {
    pub fn ask(&mut self, row: &Checkout, host: HostKey, id: String) -> bool {
        if self.asking.is_some()
            || self.pending.is_some()
            || row.main
            || row.missing
            || row.preservation.refusal.is_some()
        {
            return false;
        }
        let (Some(creation_id), Some(identity)) = (row.creation_id.clone(), row.identity.clone())
        else {
            return false;
        };
        self.asking = Some(Attempt {
            host,
            path: row.path.clone(),
            request: Request::RemoveWorktree {
                request_id: id,
                creation_id,
                identity,
                allow_unpushed: false,
            },
        });
        true
    }
    pub fn answer(&mut self, confirmed: bool) -> Option<Attempt> {
        let attempt = self.asking.take()?;
        if !confirmed {
            return None;
        }
        self.message = None;
        self.pending = Some(attempt.clone());
        Some(attempt)
    }
    pub fn finish(&mut self, attempt: &Attempt, result: Result<Operation, String>) -> bool {
        if self.pending.as_ref() != Some(attempt) {
            return false;
        }
        self.message = Some(match result {
            Ok(operation) => {
                self.pending = None;
                operation.error.map_or_else(
                    || {
                        if operation.state == Stage::Removed {
                            format!(
                                "Worktree removed: {}. Its branch and history remain.",
                                attempt.path.display()
                            )
                        } else {
                            "The host did not remove this worktree.".into()
                        }
                    },
                    |error| error.message,
                )
            }
            Err(error) => format!("{error} Removal is unconfirmed. Refresh to recover its result."),
        });
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        git::worktree::{Identity, inventory::Preservation},
        host::HostError,
    };
    fn row() -> Checkout {
        Checkout {
            path: "/repo.worktrees/tree".into(),
            main: false,
            branch: Some("plait/tree".into()),
            head: None,
            base: None,
            thread_id: Some("thread".into()),
            identity: Some(Identity {
                dev: "1".into(),
                ino: "2".into(),
            }),
            missing: false,
            removed: false,
            preservation: Preservation {
                changed: Some(0),
                unpushed: Some(0),
                exclusive: Some(0),
                refusal: None,
            },
            creation_id: Some("creation".into()),
            last_used: None,
        }
    }
    fn host() -> HostKey {
        HostKey {
            id: "host".into(),
            store: "/store".into(),
        }
    }
    #[test]
    fn asks_once_cancels_and_preserves_the_same_request_after_a_lost_reply() {
        let mut removal = Removal::default();
        assert!(removal.ask(&row(), host(), "id".into()));
        assert!(!removal.ask(&row(), host(), "other".into()));
        assert!(removal.answer(false).is_none());
        assert!(removal.pending.is_none());
        assert!(removal.ask(&row(), host(), "id".into()));
        let attempt = removal.answer(true).unwrap();
        assert!(removal.finish(&attempt, Err("Connection lost.".into())));
        assert_eq!(removal.pending.as_ref(), Some(&attempt));
        assert!(removal.message.as_ref().unwrap().contains("unconfirmed"));
        assert!(removal.finish(
            &attempt,
            Ok(Operation {
                request_id: "creation".into(),
                thread_id: "thread".into(),
                state: Stage::Removed,
                worktree: None,
                error: None
            })
        ));
        assert!(removal.pending.is_none());
        assert!(removal.message.as_ref().unwrap().contains("removed"));
        assert!(!removal.finish(&attempt, Err("late".into())));
    }
    #[test]
    fn refusal_before_and_after_confirmation_uses_the_hosts_words() {
        let mut removal = Removal::default();
        let mut row = row();
        row.preservation.refusal = Some("New work appeared.".into());
        assert!(!removal.ask(&row, host(), "id".into()));
        row.preservation.refusal = None;
        assert!(removal.ask(&row, host(), "id".into()));
        let attempt = removal.answer(true).unwrap();
        removal.finish(
            &attempt,
            Ok(Operation {
                request_id: "creation".into(),
                thread_id: "thread".into(),
                state: Stage::Ready,
                worktree: None,
                error: Some(HostError {
                    code: "busy".into(),
                    message: "New work appeared.".into(),
                }),
            }),
        );
        assert_eq!(removal.message.as_deref(), Some("New work appeared."));
        assert!(removal.pending.is_none());
    }
}
