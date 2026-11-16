//! Desktop-only preferences. Blocking file access: call on the background executor.
//!
//! The host owns model defaults and credentials. This file holds presentation
//! choices that belong to one desktop installation, so it never leaves the Mac.

use crate::model::Project;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Larger files are not ours; refuse them rather than parse arbitrary input.
const FILE_LIMIT: u64 = 64 * 1024;

/// What Cmd-1 through Cmd-9 select, counted in sidebar order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JumpTarget {
    #[default]
    Threads,
    Machines,
    Projects,
}

impl JumpTarget {
    pub const ALL: [Self; 3] = [Self::Threads, Self::Machines, Self::Projects];

    pub fn label(self) -> &'static str {
        match self {
            Self::Threads => "Threads",
            Self::Machines => "Machines",
            Self::Projects => "Projects",
        }
    }
}

/// Unknown fields are ignored so a newer client's file still loads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub jump_target: JumpTarget,
    /// Ask before each edit and command. Off by default: runs approve their own
    /// edits and sandboxed commands, and file protections still apply.
    pub ask_before_actions: bool,
}

/// The per-user location, from the environment. `None` without a home directory.
pub fn default_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|home| home.join("Library/Application Support/Plait/desktop.json"));
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home.map(|home| home.join(".config")))
        .map(|config| config.join("plait/desktop.json"))
}

/// A missing file means defaults. A malformed one is an error, not silently reset.
pub fn load(path: &Path) -> Result<Preferences, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Preferences::default());
        }
        Err(error) => return Err(format!("Cannot read preferences: {error}")),
    };
    let size = file
        .metadata()
        .map_err(|error| format!("Cannot read preferences: {error}"))?
        .len();
    if size > FILE_LIMIT {
        return Err("Preferences file is too large; defaults are in use.".into());
    }
    serde_json::from_reader(std::io::BufReader::new(file))
        .map_err(|error| format!("Preferences file is invalid ({error}); defaults are in use."))
}

/// Replace the file atomically so a crash never leaves a half-written file.
pub fn save(path: &Path, preferences: Preferences) -> Result<(), String> {
    let fail = |error: std::io::Error| format!("Cannot save preferences: {error}");
    let directory = path
        .parent()
        .ok_or_else(|| "Cannot save preferences: no parent directory.".to_owned())?;
    fs::create_dir_all(directory).map_err(fail)?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        serde_json::to_writer_pretty(&mut file, &preferences).map_err(std::io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(fail)
}

/// One sidebar thread and the project and machine rows that contain it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Jump {
    pub project: usize,
    pub machine: usize,
    /// Index into the host's thread list. Host threads belong to the local machine.
    pub thread: usize,
}

/// Threads grouped by machine, in sidebar order.
fn machines(projects: &[Project], host_threads: usize) -> Vec<Vec<Jump>> {
    let mut rows = Vec::new();
    for (project, p) in projects.iter().enumerate() {
        for (machine, m) in p.machines.iter().enumerate() {
            let threads = if m.local { host_threads } else { 0 };
            rows.push(
                (0..threads)
                    .map(|thread| Jump {
                        project,
                        machine,
                        thread,
                    })
                    .collect(),
            );
        }
    }
    rows
}

/// The thread for Cmd-`number` (1-based). A machine or project selects its
/// first thread. Returns `None` when there is no such row or it has no thread.
pub fn jump(
    projects: &[Project],
    host_threads: usize,
    target: JumpTarget,
    number: usize,
) -> Option<Jump> {
    let index = number.checked_sub(1)?;
    let machines = machines(projects, host_threads);
    match target {
        JumpTarget::Threads => machines.into_iter().flatten().nth(index),
        JumpTarget::Machines => machines.get(index)?.first().copied(),
        JumpTarget::Projects => machines
            .into_iter()
            .flatten()
            .find(|jump| jump.project == index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Machine;

    fn scratch(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("plait-preferences-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn missing_file_is_default_and_save_round_trips() {
        let root = scratch("round-trip");
        let path = root.join("nested/desktop.json");
        assert_eq!(load(&path), Ok(Preferences::default()));
        assert!(!Preferences::default().ask_before_actions);
        let chosen = Preferences {
            jump_target: JumpTarget::Machines,
            ask_before_actions: true,
        };
        save(&path, chosen).unwrap();
        assert_eq!(load(&path), Ok(chosen));
        // No temporary file is left beside the saved one.
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_and_oversized_files_are_reported_not_reset() {
        let root = scratch("malformed");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("desktop.json");
        fs::write(&path, "{\"jump_target\": \"tabs\"}").unwrap();
        assert!(load(&path).unwrap_err().contains("invalid"));
        fs::write(&path, " ".repeat(FILE_LIMIT as usize + 1)).unwrap();
        assert!(load(&path).unwrap_err().contains("too large"));
        // The bad file is left for the user to inspect.
        assert!(path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_fields_and_missing_fields_load() {
        let root = scratch("fields");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("desktop.json");
        fs::write(&path, "{\"jump_target\": \"projects\", \"later\": 1}").unwrap();
        assert_eq!(load(&path).unwrap().jump_target, JumpTarget::Projects);
        fs::write(&path, "{}").unwrap();
        assert_eq!(load(&path), Ok(Preferences::default()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_failure_is_reported() {
        let root = scratch("blocked");
        fs::create_dir_all(&root).unwrap();
        // A file where the directory should be.
        fs::write(root.join("plait"), "").unwrap();
        assert!(save(&root.join("plait/desktop.json"), Preferences::default()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    /// Two projects: this machine and a remote one, then a remote-only project.
    fn layout() -> Vec<Project> {
        let machine = |name: &str, local| Machine {
            name: name.into(),
            local,
        };
        vec![
            Project {
                name: "plait".into(),
                machines: vec![machine("zaphod", true), machine("arthur", false)],
            },
            Project {
                name: "notes".into(),
                machines: vec![machine("arthur", false)],
            },
        ]
    }

    fn thread(thread: usize) -> Jump {
        Jump {
            project: 0,
            machine: 0,
            thread,
        }
    }

    #[test]
    fn threads_are_counted_in_sidebar_order() {
        let projects = layout();
        assert_eq!(jump(&projects, 3, JumpTarget::Threads, 1), Some(thread(0)));
        assert_eq!(jump(&projects, 3, JumpTarget::Threads, 3), Some(thread(2)));
        assert_eq!(jump(&projects, 3, JumpTarget::Threads, 4), None);
        assert_eq!(jump(&projects, 3, JumpTarget::Threads, 0), None);
        assert_eq!(jump(&projects, 0, JumpTarget::Threads, 1), None);
    }

    #[test]
    fn machines_and_projects_select_their_first_thread() {
        let projects = layout();
        assert_eq!(jump(&projects, 2, JumpTarget::Machines, 1), Some(thread(0)));
        // A machine or project with no threads selects nothing.
        assert_eq!(jump(&projects, 2, JumpTarget::Machines, 2), None);
        assert_eq!(jump(&projects, 2, JumpTarget::Machines, 3), None);
        assert_eq!(jump(&projects, 2, JumpTarget::Projects, 1), Some(thread(0)));
        assert_eq!(jump(&projects, 2, JumpTarget::Projects, 2), None);
        assert_eq!(jump(&projects, 2, JumpTarget::Projects, 9), None);
    }
}
