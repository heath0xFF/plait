//! The model picker: the host's catalog as palette items, what a choice does,
//! and starting a thread without an intermediate page.
//!
//! The picker shows availability the host reported. Favorites and recently
//! used models are desktop preferences: identifiers only, never credentials.

use super::{
    Plait,
    commands::Command,
    jump::PreferenceGroup,
    palette::{Item, Options, Palette},
    theme::*,
};
use gpui::{prelude::*, *};
use plait_core::{
    host::{Model, ModelChoice},
    preferences::{ModelKey, Preferences},
};
use std::collections::{HashMap, HashSet};

/// What the chosen model is for. A per-thread switch would be a third purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModelPurpose {
    /// Create a thread with the chosen model.
    NewThread,
    /// Save the chosen model as the host's default for new threads.
    DefaultModel,
}

/// What an open picker's rows and footer were built from. When it changes,
/// the picker is rebuilt, so a save or a reconnect shows without reopening.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PickerStamp {
    connected: bool,
    models: usize,
    default: Option<ModelChoice>,
    choosing: Option<ModelChoice>,
    saving: bool,
    message: Option<String>,
    favorites: Vec<ModelKey>,
}

/// The identifier shown beside a model's display name.
pub(super) fn model_label(choice: &ModelChoice) -> String {
    format!("{}/{}", choice.provider, choice.model_id)
}

/// Identity of a model across sections. The separator cannot occur in an identifier
/// the way a slash can.
fn model_key(model: &Model) -> String {
    format!("{}\u{1f}{}", model.provider, model.model_id)
}

fn find<'a>(models: &'a [Model], key: &ModelKey) -> Option<&'a Model> {
    models
        .iter()
        .find(|model| model.provider == key.provider && model.model_id == key.model_id)
}

/// Signed-in models in the order a person reaches for them: favorites, then
/// recently used, then the rest of the catalog. Each appears once.
pub(super) fn usable_models<'a>(models: &'a [Model], preferences: &Preferences) -> Vec<&'a Model> {
    let remembered = preferences
        .favorite_models
        .iter()
        .chain(&preferences.recent_models)
        .filter_map(|key| find(models, key));
    let mut seen = HashSet::new();
    remembered
        .chain(models)
        .filter(|model| model.authenticated)
        .filter(|model| seen.insert((&model.provider, &model.model_id)))
        .collect()
}

/// Picker rows before a search: Favorites, Recent, each signed-in provider,
/// then each provider that is not signed in. A favorite is also listed under
/// its provider, so the catalog stays complete.
pub(super) fn picker_items(
    models: &[Model],
    preferences: &Preferences,
    default: Option<&ModelChoice>,
) -> Vec<Item> {
    let favorites = preferences
        .favorite_models
        .iter()
        .map(|key| (key.provider.as_str(), key.model_id.as_str()))
        .collect::<HashSet<_>>();
    let item = |model: &Model, section: String| {
        let choice = model.choice();
        let item = Item::new(
            model_key(model),
            model.name.clone(),
            Command::Model(choice.clone()),
        )
        .detail(model_label(&choice))
        .section(section)
        .enabled(model.authenticated)
        .marked(favorites.contains(&(model.provider.as_str(), model.model_id.as_str())));
        if !model.authenticated {
            item.badge("Not signed in", AMBER)
        } else if default == Some(&choice) {
            item.badge("Default", MINT)
        } else {
            // One step from any usable row; the default row carries the marker instead.
            item.aside("Set default")
        }
    };
    let remembered = |keys: &[ModelKey], section: &str| {
        keys.iter()
            .filter_map(|key| find(models, key))
            .map(|model| item(model, section.into()))
            .collect::<Vec<_>>()
    };
    let mut items = remembered(&preferences.favorite_models, "Favorites");
    items.extend(remembered(&preferences.recent_models, "Recent"));

    // Providers keep the catalog's order; signed-in groups come first.
    let mut groups: Vec<((&str, bool), Vec<&Model>)> = Vec::new();
    let mut positions = HashMap::new();
    for model in models {
        let group = (model.provider.as_str(), model.authenticated);
        let position = *positions.entry(group).or_insert_with(|| {
            groups.push((group, Vec::new()));
            groups.len() - 1
        });
        groups[position].1.push(model);
    }
    groups.sort_by_key(|((_, authenticated), _)| !authenticated);
    for ((provider, authenticated), members) in groups {
        let section = if authenticated {
            provider.to_owned()
        } else {
            format!("{provider} · not signed in")
        };
        items.extend(
            members
                .into_iter()
                .map(|model| item(model, section.clone())),
        );
    }
    items
}

impl Plait {
    fn picker_items(&self) -> Vec<Item> {
        let Some(status) = self.host.status.as_ref().filter(|_| self.host.connected) else {
            return Vec::new();
        };
        picker_items(
            &status.models,
            &self.preferences.value,
            status.default_model.as_ref(),
        )
    }

    pub(super) fn picker_stamp(&self) -> PickerStamp {
        let status = self.host.status.as_ref();
        PickerStamp {
            connected: self.host.connected,
            models: status.map_or(0, |status| status.models.len()),
            default: status.and_then(|status| status.default_model.clone()),
            choosing: self.host.settings_model.clone(),
            saving: self.host.saving,
            message: self.host.save_message.clone(),
            favorites: self.preferences.value.favorite_models.clone(),
        }
    }

    /// Rebuild an open picker when the host state or favorites behind it changed.
    pub(super) fn sync_picker(&mut self, cx: &mut Context<Self>) {
        let stamp = self.picker_stamp();
        let Some(overlay) = self.overlay.as_mut() else {
            return;
        };
        if overlay.stamp.is_none() || overlay.stamp.as_ref() == Some(&stamp) {
            return;
        }
        overlay.stamp = Some(stamp);
        let palette = overlay.palette.clone();
        let items = self.picker_items();
        let note = self.picker_note();
        palette.update(cx, |palette, cx| {
            palette.set_items(items, cx);
            palette.set_note(note, cx);
        });
    }

    /// The model a new thread starts with unless another is chosen.
    pub(super) fn new_thread_model(&self) -> Option<&Model> {
        let status = self.host.status.as_ref()?;
        let default = status.default_model.as_ref()?;
        let usable = self.host.available(Some(default));
        status
            .models
            .iter()
            .find(|model| usable && model.choice() == *default)
    }

    /// What the idle workspace says about the next thread's model.
    pub(super) fn new_thread_note(&self) -> Option<String> {
        self.new_thread_model()
            .map(|model| format!("New threads use {}", model.name))
    }

    pub(super) fn model_name(&self, choice: &ModelChoice) -> String {
        let models = self.host.status.as_ref().map(|status| &status.models);
        models
            .and_then(|models| models.iter().find(|model| model.choice() == *choice))
            .map_or_else(|| model_label(choice), |model| model.name.clone())
    }

    /// Where a default-model change stands, in words and with its accent.
    /// The host's reply decides; nothing is reported saved before it.
    pub(super) fn default_model_state(&self) -> Option<(String, u32)> {
        let saved = self.host.status.as_ref()?.default_model.as_ref();
        let choosing = self.host.settings_model.as_ref();
        if self.host.saving {
            let name = choosing.map(|choice| self.model_name(choice));
            return Some((
                format!("Saving {} as the default…", name.unwrap_or_default()),
                BLUE,
            ));
        }
        if let Some(choice) = choosing.filter(|choice| Some(*choice) != saved) {
            let reason = self.host.save_message.clone().unwrap_or_default();
            return Some((
                format!(
                    "{} is not saved as the default. {reason} Choose it again to retry.",
                    self.model_name(choice)
                ),
                AMBER,
            ));
        }
        self.host
            .save_message
            .clone()
            .map(|message| (message, MUTED))
    }

    fn picker_note(&self) -> Option<SharedString> {
        let status = self.host.status.as_ref().filter(|_| self.host.connected)?;
        if !status.models.iter().any(|model| model.authenticated) {
            return Some("No model is signed in. Sign in to a provider on the host.".into());
        }
        self.default_model_state().map(|(state, _)| state.into())
    }

    /// Why the picker has nothing to list. Only real host state is described.
    fn picker_empty(&self) -> &'static str {
        if self.host.connecting {
            "Connecting to the local host…"
        } else if !self.host.connected {
            "Not connected to the local host, so there is no model catalog. Reconnect from Settings (⌘,)."
        } else {
            "The host returned no models. Settings (⌘,) links to the setup guide."
        }
    }

    pub(super) fn open_model_picker(
        &mut self,
        purpose: ModelPurpose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (name, placeholder) = match purpose {
            ModelPurpose::NewThread => (
                "Choose a model for the new thread",
                "Search models to start a thread…",
            ),
            ModelPurpose::DefaultModel => {
                ("Choose the default model", "Search models for the default…")
            }
        };
        let status = self.host.status.as_ref().filter(|_| self.host.connected);
        let default = status.and_then(|status| {
            let default = status.default_model.as_ref()?;
            let model = status
                .models
                .iter()
                .find(|model| model.choice() == *default)?;
            Some(model_key(model))
        });
        let options = Options {
            name: name.into(),
            placeholder: placeholder.into(),
            empty: self.picker_empty().into(),
            initial: default.map(Into::into),
            mark: Some("Favorite".into()),
            aside: Some("Set default".into()),
            note: self.picker_note(),
        };
        let items = self.picker_items();
        let palette = cx.new(|cx| Palette::new(options, items, window, cx));
        self.present(palette, Some(purpose), window, cx);
    }

    pub(super) fn toggle_favorite_model(&mut self, choice: &ModelChoice, cx: &mut Context<Self>) {
        let mut value = self.preferences.value.clone();
        value.toggle_favorite(choice);
        // The open picker follows through `sync_picker`.
        self.set_preferences(value, PreferenceGroup::Models, cx);
    }

    /// Called once the host confirms a thread was created with `choice`.
    pub(super) fn record_recent_model(&mut self, choice: &ModelChoice, cx: &mut Context<Self>) {
        let mut value = self.preferences.value.clone();
        value.record_recent(choice);
        self.set_preferences(value, PreferenceGroup::Models, cx);
    }

    /// Start a thread at once with `model`, or with the saved default. When
    /// neither can be used, ask with the picker and create on selection.
    pub(super) fn new_thread(
        &mut self,
        model: Option<ModelChoice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_overlay(true, window, cx);
        // An unconfirmed creation keeps its request ID. Retry it; a second
        // request could create a second thread.
        if self.host.pending_create.is_some() {
            self.new_thread_open = true;
            self.create_host_thread(window, cx);
            return;
        }
        let default = || self.host.status.as_ref()?.default_model.clone();
        let choice = model
            .or_else(default)
            .filter(|choice| self.host.available(Some(choice)));
        let Some(choice) = choice else {
            return self.open_model_picker(ModelPurpose::NewThread, window, cx);
        };
        self.host.new_model = Some(choice);
        self.new_thread_open = true;
        self.settings_open = false;
        self.show_changes = false;
        self.create_host_thread(window, cx);
        cx.notify();
    }

    /// Put the keyboard in the selected thread's composer.
    pub(super) fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let composer = self
            .host
            .selected
            .as_ref()
            .and_then(|id| self.composers.get(id));
        if let Some(composer) = composer {
            window.focus(&composer.focus_handle(cx), cx);
        }
    }

    /// Save the picked model as the host's default. The Settings row reports the result.
    pub(super) fn choose_default_model(
        &mut self,
        choice: ModelChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.host.saving || !self.host.available(Some(&choice)) {
            return;
        }
        let saved = self
            .host
            .status
            .as_ref()
            .and_then(|s| s.default_model.as_ref());
        if saved == Some(&choice) && self.host.settings_model.as_ref() == Some(&choice) {
            return;
        }
        self.host.settings_model = Some(choice);
        self.save_default_model(window, cx);
    }

    /// Thread creation in progress or unconfirmed. There is no page for it, so
    /// the workspace says so wherever the user is.
    pub(super) fn creation_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = || {
            row()
                .flex_shrink_0()
                .px_4()
                .py_2()
                .gap_3()
                .border_b_1()
                .border_color(rgb(BORDER))
                .text_size(px(12.))
        };
        if self.host.creating {
            let model = self.host.pending_create.as_ref().map(|(_, model)| model);
            let label = model.map_or_else(
                || "Creating thread…".to_owned(),
                |model| format!("Creating thread with {}…", model_label(model)),
            );
            return Some(
                bar()
                    .child(dot(BLUE))
                    .child(div().text_color(rgb(MUTED)).child(text!(label)))
                    .into_any_element(),
            );
        }
        let failure = self.host.error.clone();
        if let Some((_, model)) = &self.host.pending_create {
            let detail = failure.unwrap_or_else(|| "The host did not answer.".into());
            let connected = self.host.connected;
            return Some(
                bar()
                    .child(dot(AMBER))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(rgb(AMBER))
                            .child(text!(format!(
                                "Thread with {} is not confirmed. {detail}",
                                model_label(model)
                            ))),
                    )
                    .child(if connected {
                        toned_button("retry-create-thread", "Retry thread creation", Tone::Filled)
                            .on_activate(cx, |this, window, cx| this.new_thread(None, window, cx))
                    } else {
                        toned_button("reconnect-to-create", "Reconnect", Tone::Filled)
                            .disabled(self.host.connecting)
                            .on_activate(cx, |this, window, cx| this.reconnect_host(window, cx))
                    })
                    .into_any_element(),
            );
        }
        // A rejected request. The conversation view shows host errors itself.
        let idle = self.new_thread_open && self.host.selected.is_none() && !self.settings_open;
        let error = failure.filter(|_| idle)?;
        Some(
            bar()
                .child(dot(RED))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(rgb(RED))
                        .child(text!(format!("Thread not created. {error}"))),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    // Named imports: a glob would bring in GPUI's own `test` attribute.
    use super::{Item, Model, ModelChoice, Preferences, picker_items, usable_models};

    fn model(provider: &str, model_id: &str, authenticated: bool) -> Model {
        Model {
            provider: provider.into(),
            model_id: model_id.into(),
            name: model_id.to_uppercase(),
            authenticated,
        }
    }

    /// Signed-in models first, as the host state sorts the catalog.
    fn catalog() -> Vec<Model> {
        vec![
            model("openai", "luna", true),
            model("openai", "sol", true),
            model("router", "vendor/opus", true),
            model("google", "gemini", false),
            model("router", "vendor/preview", false),
            model("google", "flash", false),
        ]
    }

    fn choice(provider: &str, model_id: &str) -> ModelChoice {
        ModelChoice {
            provider: provider.into(),
            model_id: model_id.into(),
        }
    }

    fn listed(items: &[Item]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|item| (item.section.to_string(), item.detail.to_string()))
            .collect()
    }

    fn pair(section: &str, detail: &str) -> (String, String) {
        (section.to_owned(), detail.to_owned())
    }

    #[test]
    fn the_catalog_is_grouped_by_provider_with_signed_in_providers_first() {
        let items = picker_items(&catalog(), &Preferences::default(), None);
        assert_eq!(
            listed(&items),
            vec![
                pair("openai", "openai/luna"),
                pair("openai", "openai/sol"),
                pair("router", "router/vendor/opus"),
                pair("google · not signed in", "google/gemini"),
                pair("google · not signed in", "google/flash"),
                pair("router · not signed in", "router/vendor/preview"),
            ]
        );
        assert_eq!(items[0].title.as_ref(), "LUNA");
    }

    #[test]
    fn models_that_are_not_signed_in_are_listed_marked_and_not_selectable() {
        let default = choice("openai", "sol");
        let items = picker_items(&catalog(), &Preferences::default(), Some(&default));
        let state = |detail: &str| {
            let item = items
                .iter()
                .find(|item| item.detail.as_ref() == detail)
                .unwrap();
            let badge = item.badge.as_ref().map(|(text, _)| text.to_string());
            (item.enabled, badge)
        };
        assert_eq!(state("openai/luna"), (true, None));
        assert_eq!(state("openai/sol"), (true, Some("Default".into())));
        assert_eq!(
            state("google/gemini"),
            (false, Some("Not signed in".into()))
        );
        // Nothing is hidden: every model in the catalog has a row.
        assert_eq!(items.len(), catalog().len());
        // A default that is no longer signed in says so, not "Default".
        let stale = choice("google", "flash");
        let items = picker_items(&catalog(), &Preferences::default(), Some(&stale));
        let flash = items.last().unwrap();
        assert_eq!(flash.badge.as_ref().unwrap().0.as_ref(), "Not signed in");
    }

    #[test]
    fn favorites_and_recents_lead_and_stay_listed_under_their_provider() {
        let mut preferences = Preferences::default();
        preferences.toggle_favorite(&choice("router", "vendor/opus"));
        preferences.toggle_favorite(&choice("google", "gemini"));
        // No longer in the catalog: remembered, but not shown.
        preferences.toggle_favorite(&choice("gone", "model"));
        preferences.record_recent(&choice("openai", "sol"));
        preferences.record_recent(&choice("gone", "model"));
        let items = picker_items(&catalog(), &preferences, None);
        assert_eq!(
            listed(&items)[..4],
            [
                pair("Favorites", "router/vendor/opus"),
                pair("Favorites", "google/gemini"),
                pair("Recent", "openai/sol"),
                pair("openai", "openai/luna"),
            ]
        );
        assert_eq!(items.len(), catalog().len() + 3);
        // The toggle shows the same state in every section a model appears in.
        for item in &items {
            let favorite = ["router/vendor/opus", "google/gemini"].contains(&item.detail.as_ref());
            assert_eq!(item.marked, Some(favorite), "{}", item.detail);
        }
        // One model has one key wherever it is listed, so a search shows it once.
        let opus = items
            .iter()
            .filter(|item| item.detail.as_ref() == "router/vendor/opus")
            .map(|item| item.key.clone())
            .collect::<Vec<_>>();
        assert_eq!(opus.len(), 2);
        assert_eq!(opus[0], opus[1]);
        // A favorite that is not signed in stays unusable in Favorites too.
        assert!(!items[1].enabled);
    }

    #[test]
    fn a_slash_in_a_model_id_cannot_collide_with_another_provider() {
        let models = vec![model("a/b", "c", true), model("a", "b/c", true)];
        let items = picker_items(&models, &Preferences::default(), None);
        assert_eq!(items[0].detail, items[1].detail);
        assert_ne!(items[0].key, items[1].key);
    }

    #[test]
    fn usable_models_are_signed_in_and_ordered_by_how_they_were_used() {
        let catalog = catalog();
        let mut preferences = Preferences::default();
        let order = |preferences: &Preferences| {
            usable_models(&catalog, preferences)
                .into_iter()
                .map(|model| model.model_id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(order(&preferences), vec!["luna", "sol", "vendor/opus"]);
        preferences.record_recent(&choice("openai", "sol"));
        preferences.toggle_favorite(&choice("router", "vendor/opus"));
        preferences.toggle_favorite(&choice("google", "gemini"));
        preferences.record_recent(&choice("router", "vendor/opus"));
        assert_eq!(order(&preferences), vec!["vendor/opus", "sol", "luna"]);
    }
}
