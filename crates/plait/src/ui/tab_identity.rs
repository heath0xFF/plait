//! Identity is verified off the UI thread; an older host keeps session-only tabs.
use super::{Plait, tab_lifecycle::Intent};
use gpui::*;
use plait_core::{
    host::{self, Client},
    tabs::{CheckoutKey, DirectoryIdentity, HostKey, ThreadTarget},
};
use std::{collections::HashMap, os::unix::fs::MetadataExt, path::PathBuf};

#[derive(Default)]
pub(super) struct Identity {
    pub verified: bool,
    pub checkouts: HashMap<PathBuf, DirectoryIdentity>,
    pub catalog_ready: bool,
    pub generation: u64,
    pub opening: u64,
}

pub(super) fn directory(root: &std::path::Path) -> Option<DirectoryIdentity> {
    let meta = std::fs::metadata(root).ok()?;
    meta.is_dir().then_some(DirectoryIdentity {
        device: meta.dev(),
        inode: meta.ino(),
    })
}

impl Plait {
    pub(super) fn verify_tab_identity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cfg!(test) || !self.host.connection.connected {
            return;
        }
        self.tabs.identity.generation += 1;
        let generation = self.tabs.identity.generation;
        let epoch = self.host.connection.epoch;
        let thread = self.host.connection.threads.first().map(|t| t.id.clone());
        let mut roots: Vec<_> = self
            .host
            .connection
            .threads
            .iter()
            .map(|t| t.checkout.clone())
            .collect();
        roots.extend(self.preferences.value.navigation.opened.iter().cloned());
        roots.push(self.host.creation.checkout.clone());
        roots.sort();
        roots.dedup();
        let task = cx.background_executor().spawn(async move {
            let client = Client::from_environment().ok()?;
            let host = host::identity::read(&client).ok().or_else(|| {
                let info = host::review::info(&client, thread.as_deref()?).ok()?;
                let (_, store) = plait_core::session::locations().ok()?;
                host::identity::from_review(info.checkout.host_id, store).ok()
            })?;
            let checkouts = roots
                .into_iter()
                .filter_map(|root| {
                    directory(&root).map(|directory| CheckoutKey {
                        host: host.clone(),
                        root,
                        directory: Some(directory),
                    })
                })
                .collect::<Vec<_>>();
            Some((host, checkouts))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.tabs.identity.generation
                    || epoch != this.host.connection.epoch
                {
                    return;
                }
                let Some((host, checkouts)) = result else {
                    return;
                };
                this.install_tab_identity(host, checkouts, cx);
                this.restore_thread_selection(window, cx);
                let owned = this
                    .tabs
                    .model
                    .active()
                    .and_then(|id| this.tabs.model.get(id))
                    .is_some_and(|t| t.thread().target().checkout().host == this.tabs.host);
                if owned
                    && this.workspace.host_watch.is_none()
                    && let Some(id) = this.host.thread.selected.clone()
                {
                    this.watch_thread(id, window, cx);
                    this.refresh_turn_review(cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn install_tab_identity(
        &mut self,
        host: HostKey,
        checkouts: Vec<CheckoutKey>,
        cx: &mut Context<Self>,
    ) {
        let previous = self.tabs.host.clone();
        if !self.tabs.identity.verified {
            let result = self
                .tabs
                .model
                .verify_session_host(&previous, &host, &checkouts);
            if !self.tab_error(result) {
                return;
            }
            for (target, _) in &mut self.tabs.lifecycle.editors {
                let checkout = match target {
                    ThreadTarget::Existing { checkout, .. }
                    | ThreadTarget::Draft { checkout, .. } => checkout,
                };
                if checkout.host == previous {
                    checkout.host = host.clone();
                    checkout.directory = checkouts
                        .iter()
                        .find(|c| c.root == checkout.root)
                        .and_then(|c| c.directory);
                }
            }
        } else if previous != host {
            self.tabs.model.disconnected(&previous);
            self.capture_session(cx);
            self.park_workspace(cx);
            self.notice = Some("The host store changed. Open its threads from the sidebar; previous tabs remain offline.".into());
        }
        self.tabs.host = host;
        self.tabs.identity.verified = true;
        self.tabs.identity.checkouts = checkouts
            .into_iter()
            .filter_map(|c| c.directory.map(|d| (c.root, d)))
            .collect();
        let result = self
            .tabs
            .model
            .reconnected(&self.tabs.host, self.host.connection.epoch);
        self.tab_error(result);
        self.refresh_tab_rows();
        cx.notify();
    }

    pub(super) fn open_tab_target(
        &mut self,
        mut target: ThreadTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.tabs.identity.verified || cfg!(test) {
            self.request_tab(Intent::Open(target), window, cx);
            return;
        }
        self.tabs.identity.opening += 1;
        let opening = self.tabs.identity.opening;
        let host = self.tabs.host.clone();
        let root = target.checkout().root.clone();
        let task = cx
            .background_executor()
            .spawn(async move { directory(&root) });
        cx.spawn_in(window, async move |this, cx| {
            let directory = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.tabs.identity.opening != opening || this.tabs.host != host {
                    return;
                }
                let checkout = match &mut target {
                    ThreadTarget::Existing { checkout, .. }
                    | ThreadTarget::Draft { checkout, .. } => checkout,
                };
                checkout.directory = directory;
                if let Some(directory) = directory {
                    this.tabs
                        .identity
                        .checkouts
                        .insert(checkout.root.clone(), directory);
                }
                this.request_tab(Intent::Open(target), window, cx);
            });
        })
        .detach();
    }
}
