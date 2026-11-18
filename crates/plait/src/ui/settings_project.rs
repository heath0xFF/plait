//! A project's settings page: what applies to every thread in the active
//! project and nowhere else. It holds the checkout's command policy and the
//! Pi setup its threads are offered. Opened from the project's sidebar menu
//! or Project Settings…, it always shows the active project.

use super::{Plait, settings, theme::*};
use gpui::{prelude::*, *};
use std::path::PathBuf;

impl Plait {
    pub(super) fn project_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // It heads the column and keeps its own height; the page below scrolls.
        let title = column()
            .flex_shrink_0()
            .min_w_0()
            .gap(px(2.))
            .child(settings::heading(self.active_project().name.clone()))
            // The checkout the policy and the Pi setup are read for.
            .child(
                tag(
                    self.host.checkout.to_string_lossy().into_owned(),
                    colors().subtle,
                )
                .truncate(),
            );
        settings::framed(
            self.settings_column(Some(title.into_any_element())).child(
                settings::page("project-settings-page")
                    .track_scroll(&self.settings_scroll)
                    .child(self.policy_settings(cx))
                    .child(self.agent_settings(cx)),
            ),
        )
    }

    /// Project Settings… in a project's sidebar menu. Another project becomes
    /// the active one first, as choosing it in the sidebar does, so the page
    /// and everything it changes belong to the project that was chosen.
    pub(super) fn show_settings_for_project(
        &mut self,
        project: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_of(&self.host.checkout) != project.as_path() {
            self.change_checkout(project.clone(), window, cx);
            // Refused while a thread is being created; its notice says why.
            if self.project_of(&self.host.checkout) != project.as_path() {
                return;
            }
        }
        self.show_project_settings(window, cx);
    }
}
