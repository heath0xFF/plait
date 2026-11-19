//! Bounded logical storage accounting. No Git, symlink following, or reclamation promise.
use std::{
    collections::HashSet,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub source: u64,
    pub build: u64,
    pub incomplete: bool,
    pub entries: usize,
}
impl Size {
    pub fn total(&self) -> u64 {
        self.source.saturating_add(self.build)
    }
}
pub const ENTRY_LIMIT: usize = 50_000;
pub const DEADLINE: Duration = Duration::from_millis(250);
pub const BUILD_DIRS: &[&str] = &["target", "build", "dist", ".next", "node_modules"];
pub fn measure(root: &Path, cache: Option<&Path>, cancelled: &AtomicBool) -> Result<Size, String> {
    measure_bounded(root, cache, ENTRY_LIMIT, DEADLINE, cancelled)
}
pub fn measure_bounded(
    root: &Path,
    cache: Option<&Path>,
    limit: usize,
    duration: Duration,
    cancelled: &AtomicBool,
) -> Result<Size, String> {
    let deadline = Instant::now() + duration;
    let meta =
        fs::symlink_metadata(root).map_err(|e| format!("Cannot measure this folder: {e}"))?;
    if !meta.is_dir() || meta.is_symlink() {
        return Err("The checkout folder is missing or replaced.".into());
    }
    let mut result = Size::default();
    let mut seen = HashSet::new();
    let mut stack = vec![(root.to_owned(), false)];
    if let Some(cache) = cache
        && cache != root
        && fs::symlink_metadata(cache).is_ok()
    {
        stack.push((cache.to_owned(), true));
    }
    while let Some((path, build)) = stack.pop() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Measurement cancelled.".into());
        }
        if result.entries >= limit || Instant::now() >= deadline {
            result.incomplete = true;
            break;
        }
        result.entries += 1;
        let Ok(meta) = fs::symlink_metadata(&path) else {
            result.incomplete = true;
            continue;
        };
        if meta.is_symlink() {
            continue;
        }
        if !seen.insert((meta.dev(), meta.ino())) {
            continue;
        }
        if meta.is_file() {
            if build {
                result.build = result.build.saturating_add(meta.len());
            } else {
                result.source = result.source.saturating_add(meta.len());
            }
        } else if meta.is_dir() {
            let Ok(entries) = fs::read_dir(&path) else {
                result.incomplete = true;
                continue;
            };
            for entry in entries {
                if stack.len() + result.entries >= limit || Instant::now() >= deadline {
                    result.incomplete = true;
                    break;
                }
                let Ok(entry) = entry else {
                    result.incomplete = true;
                    continue;
                };
                if entry.file_name() == ".git" {
                    continue;
                }
                let output = build
                    || path == root
                        && entry
                            .file_name()
                            .to_str()
                            .is_some_and(|s| BUILD_DIRS.contains(&s));
                stack.push((entry.path(), output));
            }
        }
    }
    Ok(result)
}
pub fn measure_retained_cache(
    cache: Option<&Path>,
    cancelled: &AtomicBool,
) -> Result<Size, String> {
    let Some(cache) = cache else {
        return Ok(Size::default());
    };
    match fs::symlink_metadata(cache) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Size::default()),
        _ => {}
    }
    let mut result = measure(cache, None, cancelled)?;
    result.build = result.total();
    result.source = 0;
    Ok(result)
}
pub fn measure_verified(
    key: &Key,
    cache: Option<&Path>,
    cancelled: &AtomicBool,
) -> Result<Size, String> {
    if let Some(expected) = &key.identity {
        let meta = fs::symlink_metadata(&key.root)
            .map_err(|e| format!("Cannot measure this folder: {e}"))?;
        if expected.dev != meta.dev().to_string() || expected.ino != meta.ino().to_string() {
            return Err(
                "The checkout folder changed identity. Refresh before measuring it.".into(),
            );
        }
    }
    measure(&key.root, cache, cancelled)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub root: PathBuf,
    pub identity: Option<crate::git::worktree::Identity>,
    pub head: Option<String>,
    pub last_used: Option<u64>,
    pub changed: Option<u64>,
    pub removed: bool,
}
#[derive(Default)]
pub struct Cache(Vec<(Key, Size)>);
impl Cache {
    pub fn get(&self, key: &Key) -> Option<&Size> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, s)| s)
    }
    pub fn put(&mut self, key: Key, size: Size) {
        self.0.retain(|(k, _)| k.root != key.root);
        self.0.push((key, size));
        if self.0.len() > 256 {
            self.0.remove(0);
        }
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes_bound_entries_deadline_links_and_cache_without_claiming_reclaimed_space() {
        let root = std::env::temp_dir().join(format!("plait-size-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("source"), vec![0; 100]).unwrap();
        fs::write(root.join("target/output"), vec![0; 200]).unwrap();
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
        // The host sandbox denies link creation; the ordinary test still covers deduplication.
        if let Err(error) = fs::hard_link(root.join("source"), root.join("copy")) {
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        }
        let cancel = AtomicBool::new(false);
        let full = measure_bounded(&root, None, 100, Duration::from_secs(1), &cancel).unwrap();
        assert_eq!((full.source, full.build), (100, 200));
        assert!(!full.incomplete);
        assert!(
            measure_bounded(&root, None, 1, Duration::from_secs(1), &cancel)
                .unwrap()
                .incomplete
        );
        assert!(
            measure_bounded(&root, None, 100, Duration::ZERO, &cancel)
                .unwrap()
                .incomplete
        );
        cancel.store(true, Ordering::Relaxed);
        assert!(measure(&root, None, &cancel).is_err());
        let key = Key {
            root: root.clone(),
            identity: None,
            head: Some("a".into()),
            last_used: None,
            changed: Some(0),
            removed: false,
        };
        let mut cache = Cache::default();
        cache.put(key.clone(), full.clone());
        assert_eq!(cache.get(&key), Some(&full));
        let changed = Key {
            head: Some("b".into()),
            ..key.clone()
        };
        assert!(cache.get(&changed).is_none());
        let removed = Key {
            removed: true,
            ..key.clone()
        };
        assert!(cache.get(&removed).is_none());
        cancel.store(false, Ordering::Relaxed);
        let retained = measure_retained_cache(Some(&root.join("target")), &cancel).unwrap();
        assert_eq!((retained.source, retained.build), (0, 200));
        let replaced = Key {
            identity: Some(crate::git::worktree::Identity {
                dev: "0".into(),
                ino: "0".into(),
            }),
            ..key.clone()
        };
        assert!(measure_verified(&replaced, None, &cancel).is_err());
        cache.clear();
        assert!(cache.get(&key).is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
