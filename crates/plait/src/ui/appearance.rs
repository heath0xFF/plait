//! Light and dark themes. Preferences name a theme for each appearance; the
//! appearance follows macOS unless one is chosen. Scheme files in the themes
//! folder are read in the background at launch and on request.

use super::{Plait, jump::PreferenceGroup, theme::*, thread_controls::TextTooltip};
use gpui::{prelude::*, *};
use plait_core::theme::{self as themes, Appearance, PLAIT_DARK, PLAIT_LIGHT, Theme};
use std::path::PathBuf;

pub(super) struct Themes {
    built_in: Vec<Theme>,
    /// Read from the themes folder; listed before the shipped ones.
    pub user: Vec<Theme>,
    pub errors: Vec<String>,
    pub folder: Option<PathBuf>,
    /// The appearance override last given to macOS, so it is set only on change.
    applied: Option<Option<WindowAppearance>>,
    generation: u64,
    /// Scenes pin light or dark instead of asking the test platform.
    #[cfg(test)]
    pub forced: Option<bool>,
}

impl Default for Themes {
    fn default() -> Self {
        Self {
            built_in: themes::built_in(),
            user: Vec::new(),
            errors: Vec::new(),
            folder: themes::folder(),
            applied: None,
            generation: 0,
            #[cfg(test)]
            forced: Some(false),
        }
    }
}

impl Themes {
    /// Themes of one appearance: the folder's first, then the shipped ones.
    pub fn of(&self, light: bool) -> impl Iterator<Item = &Theme> {
        self.user
            .iter()
            .chain(&self.built_in)
            .filter(move |theme| theme.palette.light == light)
    }

    /// The palette for `name`, or the Plait default when it is gone.
    fn palette(&self, name: Option<&str>, light: bool) -> themes::Palette {
        name.and_then(|name| self.of(light).find(|theme| theme.name == name))
            .map(|theme| theme.palette)
            .unwrap_or(if light { PLAIT_LIGHT } else { PLAIT_DARK })
    }
}

impl Plait {
    /// Pick the palette for the current appearance and redraw if it changed.
    pub(super) fn apply_theme(&mut self, cx: &mut Context<Self>) {
        let preferences = &self.preferences.value;
        let appearance = preferences.appearance;
        // Native window chrome follows the chosen appearance too.
        let wanted = match appearance {
            Appearance::System => None,
            Appearance::Light => Some(WindowAppearance::Light),
            Appearance::Dark => Some(WindowAppearance::Dark),
        };
        if self.themes.applied != Some(wanted) {
            self.themes.applied = Some(wanted);
            cx.set_window_appearance(wanted);
        }
        let light = self.light_appearance(appearance, cx);
        let name = if light {
            preferences.light_theme.as_deref()
        } else {
            preferences.dark_theme.as_deref()
        };
        if set_colors(&self.themes.palette(name, light)) {
            cx.refresh_windows();
        }
    }

    fn light_appearance(&self, appearance: Appearance, cx: &App) -> bool {
        #[cfg(test)]
        if let Some(light) = self.themes.forced {
            return light;
        }
        match appearance {
            Appearance::Light => true,
            Appearance::Dark => false,
            Appearance::System => matches!(
                cx.window_appearance(),
                WindowAppearance::Light | WindowAppearance::VibrantLight
            ),
        }
    }

    /// Follow macOS switching between light and dark.
    pub(super) fn observe_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.observe_window_appearance(window, |this, _, cx| this.apply_theme(cx))
            .detach();
    }

    /// Read the themes folder in the background, then apply the choice again,
    /// since a chosen theme may live there.
    pub(super) fn load_themes(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.themes.folder.clone() else {
            return;
        };
        self.themes.generation += 1;
        let generation = self.themes.generation;
        let task = cx
            .background_executor()
            .spawn(async move { themes::load_folder(&folder) });
        cx.spawn(async move |this, cx| {
            let (user, errors) = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.themes.generation != generation {
                    return;
                }
                this.themes.user = user;
                this.themes.errors = errors;
                this.apply_theme(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        let mut value = self.preferences.value.clone();
        value.appearance = appearance;
        self.set_preferences(value, PreferenceGroup::View, cx);
        self.apply_theme(cx);
    }

    fn set_theme(&mut self, light: bool, name: String, cx: &mut Context<Self>) {
        let mut value = self.preferences.value.clone();
        // The default is stored as unset, so it follows future defaults.
        let default = if light {
            themes::PLAIT_LIGHT_NAME
        } else {
            themes::PLAIT_DARK_NAME
        };
        let name = (name != default).then_some(name);
        if light {
            value.light_theme = name;
        } else {
            value.dark_theme = name;
        }
        self.set_preferences(value, PreferenceGroup::View, cx);
        self.apply_theme(cx);
    }

    /// Open the themes folder in Finder, creating it first so there is
    /// somewhere to put scheme files.
    fn open_themes_folder(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.themes.folder.clone() else {
            return;
        };
        let task = cx.background_executor().spawn({
            let folder = folder.clone();
            async move { std::fs::create_dir_all(&folder) }
        });
        cx.spawn(async move |this, cx| {
            let created = task.await;
            let _ = this.update(cx, |this, cx| match created {
                Ok(()) => cx.open_with_system(&folder),
                Err(error) => {
                    this.themes.errors = vec![format!("Cannot create the themes folder: {error}")];
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Settings → View: appearance, a theme for each, and the themes folder.
    pub(super) fn theme_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.preferences.value.appearance;
        let mut choices = segmented();
        for (index, (label, value)) in [
            ("System", Appearance::System),
            ("Light", Appearance::Light),
            ("Dark", Appearance::Dark),
        ]
        .into_iter()
        .enumerate()
        {
            choices = choices.child(
                segment(("appearance", index), label, value == chosen)
                    .role(Role::RadioButton)
                    .aria_toggled(if value == chosen {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .aria_label(format!("Appearance: {label}"))
                    .on_activate(cx, move |this, _, cx| this.set_appearance(value, cx)),
            );
        }
        let folder_note = match &self.themes.folder {
            Some(folder) => format!(
                "Add Base16 scheme files (.yaml) to {} and choose Reload. Hundreds are published at github.com/tinted-theming/schemes.",
                home_relative(folder)
            ),
            None => "No themes folder: the home directory is unknown.".into(),
        };
        column()
            .gap_4()
            .child(super::settings::setting(
                "Appearance",
                "System follows macOS between light and dark.",
                choices,
            ))
            .child(
                row()
                    .gap_4()
                    .items_start()
                    .child(self.theme_list(true, cx))
                    .child(self.theme_list(false, cx)),
            )
            .child(
                column()
                    .gap_2()
                    .child(tag(folder_note, colors().subtle).line_height(px(16.)))
                    .child(
                        row()
                            .gap_2()
                            .child(
                                button("themes-open-folder", "Open themes folder")
                                    .on_activate(cx, |this, _, cx| this.open_themes_folder(cx)),
                            )
                            .child(
                                button("themes-reload", "Reload")
                                    .on_activate(cx, |this, _, cx| this.load_themes(cx)),
                            ),
                    )
                    .children(
                        self.themes
                            .errors
                            .iter()
                            .map(|error| tag(error.clone(), colors().amber).line_height(px(16.))),
                    ),
            )
    }

    fn theme_list(&self, light: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let preferences = &self.preferences.value;
        let chosen = if light {
            preferences
                .light_theme
                .as_deref()
                .unwrap_or(themes::PLAIT_LIGHT_NAME)
        } else {
            preferences
                .dark_theme
                .as_deref()
                .unwrap_or(themes::PLAIT_DARK_NAME)
        };
        let label = if light { "Light theme" } else { "Dark theme" };
        column()
            .id(if light { "themes-light" } else { "themes-dark" })
            .flex_1()
            .min_w_0()
            .gap_1()
            .role(Role::RadioGroup)
            .aria_label(label)
            .child(section_label(label))
            .children(self.themes.of(light).enumerate().map(|(index, theme)| {
                let selected = theme.name == chosen;
                let name = theme.name.clone();
                let tip = match &theme.source {
                    themes::Source::BuiltIn => "Plait's own theme".to_owned(),
                    themes::Source::Bundled { author } if !author.is_empty() => {
                        format!("Base16 scheme by {author}")
                    }
                    themes::Source::Bundled { .. } => "Base16 scheme".to_owned(),
                    themes::Source::File(path) => path.display().to_string(),
                };
                nav_row(
                    (if light { "theme-light" } else { "theme-dark" }, index),
                    format!("{label}: {}", theme.name),
                    selected,
                )
                .role(Role::RadioButton)
                .aria_toggled(if selected {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .gap_2()
                .px_2()
                .tooltip(move |_, cx| cx.new(|_| TextTooltip(tip.clone())).into())
                .child(swatch(&theme.palette))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .child(theme.name.clone()),
                )
                .on_activate(cx, move |this, _, cx| {
                    this.set_theme(light, name.clone(), cx)
                })
            }))
    }
}

/// A theme's background with its text and four accents on it.
fn swatch(palette: &themes::Palette) -> Div {
    let mark = |color: u32| div().size(px(6.)).rounded_full().bg(rgb(color));
    row()
        .flex_shrink_0()
        .gap(px(2.))
        .px(px(5.))
        .h(px(16.))
        .rounded_sm()
        .border_1()
        .border_color(rgb(colors().border))
        .bg(rgb(palette.bg))
        .child(mark(palette.text))
        .child(mark(palette.blue))
        .child(mark(palette.mint))
        .child(mark(palette.amber))
        .child(mark(palette.red))
}

/// `~/…` for a path under the home directory, so the note stays short.
fn home_relative(path: &std::path::Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.and_then(|home| path.strip_prefix(home).ok().map(PathBuf::from)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
