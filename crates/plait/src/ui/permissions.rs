//! The Permissions chip: a harness's own permission modes, as the host lists
//! them, in a menu. It stands alone so any row can place it: the caller says
//! which harness, which mode is current, and what to do with a choice.
//!
//! Pi threads use Plait's own approvals instead; their run settings are in
//! `thread_controls`.

use super::{
    Plait,
    commands::Command,
    palette::{Item, Options, Palette},
    theme::*,
    thread_controls::TextTooltip,
};
use gpui::{prelude::*, *};
use plait_core::host::harness::HarnessId;
use std::rc::Rc;

/// What to do with the mode id chosen from the menu.
pub(super) type OnPermissions = Rc<dyn Fn(&mut Plait, String, &mut Window, &mut Context<Plait>)>;

/// One Permissions chip and the menu it opens.
pub(super) struct Permissions {
    pub(super) harness: HarnessId,
    /// The mode id the chip shows and the menu marks current.
    pub(super) current: String,
    /// Why it cannot change now. The chip is disabled and says so.
    pub(super) blocked: Option<&'static str>,
    /// The chip's hover text.
    pub(super) tip: SharedString,
    /// Under the menu: when a choice applies, or what the harness is really using.
    pub(super) note: SharedString,
    pub(super) on_select: OnPermissions,
}

impl Plait {
    /// The chip, or `None` when the harness lists no modes of its own.
    pub(super) fn permissions_chip(
        &self,
        id: impl Into<ElementId>,
        permissions: Permissions,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let row = self.harness_row(permissions.harness)?;
        if row.modes.is_empty() {
            return None;
        }
        let label = row.mode(&permissions.current).map_or_else(
            || SharedString::from(permissions.current.clone()),
            |mode| mode.label.clone(),
        );
        let tip = permissions.tip.clone();
        let blocked = permissions.blocked;
        let permissions = Rc::new(permissions);
        Some(
            button(id, "")
                .min_w_0()
                .flex_shrink_1()
                .gap_1()
                .aria_label(format!("Permissions: {label}"))
                .aria_description(blocked.map_or_else(|| tip.clone(), SharedString::from))
                .tooltip(move |_, cx| cx.new(|_| TextTooltip(tip.to_string())).into())
                .disabled(blocked.is_some())
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(colors().subtle))
                        .child("Permissions"),
                )
                .child(div().min_w_0().truncate().child(label))
                .child(icon("down").size(px(12.)))
                .on_activate(cx, move |this, window, cx| {
                    this.open_permissions(&permissions, window, cx)
                }),
        )
    }

    /// The harness's modes as a menu, the current one marked.
    pub(super) fn open_permissions(
        &mut self,
        permissions: &Permissions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if permissions.blocked.is_some() {
            return;
        }
        let Some(row) = self.harness_row(permissions.harness) else {
            return;
        };
        let section = SharedString::from(format!("{} permissions", row.name));
        let items = row
            .modes
            .iter()
            .map(|mode| {
                let item = Item::new(
                    format!("permissions-{}", mode.id),
                    mode.label.clone(),
                    Command::Permissions {
                        mode: mode.id.clone(),
                        select: permissions.on_select.clone(),
                    },
                )
                .section(section.clone())
                .detail(mode.detail.clone());
                if mode.id == permissions.current {
                    item.badge("Current", colors().blue)
                } else {
                    item
                }
            })
            .collect();
        let options = Options {
            name: "Permissions".into(),
            placeholder: format!("Choose what {} may do…", row.name).into(),
            empty: "This harness lists no permissions of its own.".into(),
            initial: Some(format!("permissions-{}", permissions.current).into()),
            mark: None,
            aside: None,
            note: Some(permissions.note.clone()),
        };
        let palette = cx.new(|cx| Palette::new(options, items, window, cx));
        self.present(palette, None, window, cx);
    }
}
