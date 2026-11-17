//! Light and dark themes. Preferences name a theme for each appearance; the
//! appearance follows macOS unless one is chosen. Scheme files in the themes
//! folder are read in the background at launch and on request.

use super::{
    Plait,
    jump::PreferenceGroup,
    text_field::{TextField, TextFieldEvent},
    theme::*,
    thread_controls::TextTooltip,
};
use gpui::{prelude::*, *};
use plait_core::theme::{self as themes, Appearance, DEFAULT_DARK, DEFAULT_LIGHT, Theme};
use std::{path::PathBuf, sync::Arc};

/// The code font choice: the families installed on this Mac, read in the
/// background, and the field in Settings → Appearance that names one.
#[derive(Default)]
pub(super) struct CodeFont {
    /// Installed family names, sorted. `None` until they have been read.
    pub installed: Option<Arc<Vec<String>>>,
    loading: bool,
    pub field: Option<Entity<TextField>>,
    _changed: Option<Subscription>,
}

/// The installed family that `wanted` names, whatever its case.
fn installed_family<'a>(installed: &'a [String], wanted: &str) -> Option<&'a str> {
    installed
        .iter()
        .find(|name| name.eq_ignore_ascii_case(wanted))
        .map(String::as_str)
}

/// Up to `limit` installed families whose name contains `typed`, those that
/// start with it first. Nothing for an empty or an exactly matching name.
fn family_suggestions<'a>(installed: &'a [String], typed: &str, limit: usize) -> Vec<&'a str> {
    if typed.is_empty() || installed_family(installed, typed).is_some() {
        return Vec::new();
    }
    let typed = typed.to_ascii_lowercase();
    let matching = |start: bool| {
        let typed = typed.clone();
        installed.iter().map(String::as_str).filter(move |name| {
            let name = name.to_ascii_lowercase();
            // System families begin with a dot and are not for choosing.
            !name.starts_with('.') && name.contains(&typed) && name.starts_with(&typed) == start
        })
    };
    matching(true).chain(matching(false)).take(limit).collect()
}

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

    /// The palette for `name`, or the default when it is gone.
    fn palette(&self, name: Option<&str>, light: bool) -> themes::Palette {
        name.and_then(|name| self.of(light).find(|theme| theme.name == name))
            .map(|theme| theme.palette)
            .unwrap_or(if light { DEFAULT_LIGHT } else { DEFAULT_DARK })
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

    /// Read the installed font families in the background, once, then apply
    /// the saved choice: a family is drawn only once it is known to be here.
    pub(super) fn load_fonts(&mut self, cx: &mut Context<Self>) {
        if self.code_font.installed.is_some() || self.code_font.loading {
            return;
        }
        self.code_font.loading = true;
        let text_system = cx.text_system().clone();
        let task = cx
            .background_executor()
            .spawn(async move { text_system.all_font_names() });
        cx.spawn(async move |this, cx| {
            let names = task.await;
            let _ = this.update(cx, |this, cx| {
                this.code_font.loading = false;
                this.code_font.installed = Some(Arc::new(names));
                this.apply_code_font(cx);
                // A name typed while the families were being read counts now.
                if this.code_font.field.is_some() {
                    this.code_font_typed(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Draw code in the saved family when it is installed, else in the
    /// system's monospaced font.
    pub(super) fn apply_code_font(&mut self, cx: &mut Context<Self>) {
        let chosen = self.preferences.value.code_font().and_then(|wanted| {
            let installed = self.code_font.installed.as_deref()?;
            installed_family(installed, wanted)
        });
        let family = match chosen {
            Some(name) => SharedString::from(name.to_owned()),
            None => SharedString::new_static(SYSTEM_MONO),
        };
        if set_mono(family) {
            // Another font has other widths and heights.
            self.live_scroll.remeasure();
            cx.refresh_windows();
        }
    }

    /// Settings is opening: make the field that names a family, and learn
    /// which families are installed.
    pub(super) fn prepare_code_font(&mut self, cx: &mut Context<Self>) {
        if self.code_font.field.is_none() {
            let chosen = self
                .preferences
                .value
                .code_font()
                .unwrap_or_default()
                .to_owned();
            let field = cx.new(|cx| {
                let mut field = TextField::new("Code font", "System monospaced", cx);
                field.set_text(&chosen, cx);
                field
            });
            self.code_font._changed =
                Some(cx.subscribe(&field, |this, _, _: &TextFieldEvent, cx| {
                    this.code_font_typed(cx)
                }));
            self.code_font.field = Some(field);
        }
        self.load_fonts(cx);
    }

    /// What is typed in the field, without the space around it.
    fn typed_code_font(&self, cx: &App) -> String {
        self.code_font
            .field
            .as_ref()
            .map(|field| field.read(cx).text().trim().to_owned())
            .unwrap_or_default()
    }

    /// The field changed. An empty field means the system's font; an
    /// installed family's name is saved and drawn at once; anything else
    /// leaves the choice as it was.
    fn code_font_typed(&mut self, cx: &mut Context<Self>) {
        let typed = self.typed_code_font(cx);
        let chosen = if typed.is_empty() {
            None
        } else {
            let Some(installed) = self.code_font.installed.as_deref() else {
                return;
            };
            match installed_family(installed, &typed) {
                Some(name) => Some(name.to_owned()),
                None => {
                    cx.notify();
                    return;
                }
            }
        };
        if chosen.as_deref() != self.preferences.value.code_font() {
            let mut value = self.preferences.value.clone();
            value.code_font = chosen;
            self.set_preferences(value, PreferenceGroup::View, cx);
        }
        self.apply_code_font(cx);
        cx.notify();
    }

    /// Settings → Appearance: the font code, paths, and commands are drawn in.
    fn code_font_setting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        /// Installed families offered while a name is being typed.
        const OFFERED: usize = 5;
        let typed = self.typed_code_font(cx);
        let installed = self.code_font.installed.clone();
        let known = installed
            .as_deref()
            .and_then(|names| installed_family(names, &typed));
        let (tint, note) = if typed.is_empty() {
            (
                colors().mint,
                "Using the system's monospaced font.".to_owned(),
            )
        } else if let Some(name) = known {
            (colors().mint, format!("Using {name}."))
        } else if installed.is_none() {
            (colors().subtle, "Reading the installed fonts…".to_owned())
        } else {
            (
                colors().amber,
                format!("No installed font is named “{typed}”. The last choice stays in use."),
            )
        };
        let offered: Vec<String> = installed
            .as_deref()
            .map(|names| family_suggestions(names, &typed, OFFERED))
            .unwrap_or_default()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let control = column()
            .w(px(260.))
            .flex_shrink_0()
            .gap(px(6.))
            .when_some(self.code_font.field.clone(), |el, field| el.child(field))
            .child(
                row()
                    .gap(px(6.))
                    .child(dot(tint))
                    .child(tag(note, colors().subtle).line_height(px(16.))),
            )
            .when(!offered.is_empty(), |el| {
                el.child(row().flex_wrap().gap(px(4.)).children(
                    offered.into_iter().enumerate().map(|(index, name)| {
                        let family = name.clone();
                        button(("code-font-offer", index), name.clone())
                            .aria_label(format!("Use {name} for code"))
                            .on_activate(cx, move |this, _, cx| {
                                if let Some(field) = this.code_font.field.clone() {
                                    field.update(cx, |field, cx| field.set_text(&family, cx));
                                }
                            })
                    }),
                ))
            });
        super::settings::setting(
            "Code font",
            "Code, file paths, and commands. Type the name of an installed font, or leave it empty for the system's monospaced font.",
            control,
        )
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
            themes::DEFAULT_LIGHT_NAME
        } else {
            themes::DEFAULT_DARK_NAME
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

    /// Settings → Appearance: light or dark, a theme for each, the themes
    /// folder, and the code font.
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
                    .aria_label(format!("Light or dark: {label}"))
                    .on_activate(cx, move |this, _, cx| this.set_appearance(value, cx)),
            );
        }
        let folder_note = match &self.themes.folder {
            Some(folder) => format!(
                "Add Base16 scheme files (.yaml) to {} and choose Reload. Hundreds are published at github.com/tinted-theming/schemes.",
                super::theme::home_relative(folder)
            ),
            None => "No themes folder: the home directory is unknown.".into(),
        };
        column()
            .gap_4()
            .child(super::settings::setting(
                "Light or dark",
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
            .child(self.code_font_setting(cx))
    }

    fn theme_list(&self, light: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let preferences = &self.preferences.value;
        let chosen = if light {
            preferences
                .light_theme
                .as_deref()
                .unwrap_or(themes::DEFAULT_LIGHT_NAME)
        } else {
            preferences
                .dark_theme
                .as_deref()
                .unwrap_or(themes::DEFAULT_DARK_NAME)
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
                    themes::Source::Included { credit } => (*credit).to_owned(),
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

#[cfg(test)]
mod tests {
    use super::{family_suggestions, installed_family};

    fn installed() -> Vec<String> {
        [
            ".AppleSystemUIFontMonospaced",
            "Courier New",
            "JetBrains Mono",
            "JetBrains Mono NL",
            "Menlo",
            "SF Compact Rounded",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn a_family_is_found_by_its_name_whatever_the_case() {
        let installed = installed();
        assert_eq!(installed_family(&installed, "menlo"), Some("Menlo"));
        assert_eq!(installed_family(&installed, "Menlo Bold"), None);
        assert_eq!(installed_family(&installed, ""), None);
    }

    #[test]
    fn suggestions_lead_with_names_that_start_with_what_was_typed() {
        let installed = installed();
        assert_eq!(
            family_suggestions(&installed, "mono", 5),
            ["JetBrains Mono", "JetBrains Mono NL"]
        );
        // Names that start with the text come before those that only contain it.
        assert_eq!(
            family_suggestions(&installed, "co", 5),
            ["Courier New", "SF Compact Rounded"]
        );
        assert_eq!(family_suggestions(&installed, "jet", 1), ["JetBrains Mono"]);
        // Nothing to offer for an empty field or a name that is already whole,
        // and system families are not offered.
        assert!(family_suggestions(&installed, "", 5).is_empty());
        assert!(family_suggestions(&installed, "Menlo", 5).is_empty());
        assert!(family_suggestions(&installed, "apple", 5).is_empty());
    }
}
