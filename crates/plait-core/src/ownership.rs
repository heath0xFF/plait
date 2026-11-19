//! Stable operation owners and short-lived read contexts. No UI state lives here.
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

/// The local host endpoint is this protocol's identity; it has no server UUID.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HostKey(pub PathBuf);

impl HostKey {
    pub fn local() -> Option<Self> {
        let store = std::env::var_os("PLAIT_HOST_STORE")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share/plait"))
            })?;
        store.is_absolute().then_some(Self(store))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThreadKey {
    pub host: Option<HostKey>,
    pub id: String,
}

/// A root pathname alone does not identify a checkout after a directory swap.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CheckoutKey {
    pub host: Option<HostKey>,
    pub root: PathBuf,
    pub device: u64,
    pub inode: u64,
}

impl CheckoutKey {
    /// Blocking: resolve only on a background executor.
    pub fn resolve(host: Option<HostKey>, root: &Path) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|error| format!("Cannot identify checkout: {error}"))?;
        let meta = std::fs::symlink_metadata(&root)
            .map_err(|error| format!("Cannot identify checkout: {error}"))?;
        if !meta.is_dir() {
            return Err("Checkout is not a directory.".into());
        }
        Ok(Self {
            host,
            root,
            device: meta.dev(),
            inode: meta.ino(),
        })
    }

    /// Blocking. A substituted symlink is a changed identity, even if it resolves back.
    pub fn verify(&self) -> Result<(), String> {
        let meta = std::fs::symlink_metadata(&self.root)
            .map_err(|_| "Checkout identity changed.".to_owned())?;
        if !meta.is_dir()
            || meta.dev() != self.device
            || meta.ino() != self.inode
            || self.root.canonicalize().ok().as_ref() != Some(&self.root)
        {
            return Err("Checkout identity changed.".into());
        }
        Ok(())
    }
}

/// Selection and checkout lifetimes advance independently of the connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadStamp {
    pub connection: u64,
    pub selection: u64,
    pub checkout: u64,
    pub thread: Option<ThreadKey>,
}

/// Acknowledgements use this owner, never a visible selection generation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OperationKey {
    pub thread: ThreadKey,
    pub request: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_aliases_share_identity_but_replacements_do_not() {
        let scratch = std::env::temp_dir().join(format!(
            "plait-identity-{}",
            crate::host_state::request_id()
        ));
        let root = scratch.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let alias = scratch.join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let key = CheckoutKey::resolve(None, &root).unwrap();
        assert_eq!(CheckoutKey::resolve(None, &alias).unwrap(), key);
        key.verify().unwrap();
        std::fs::rename(&root, scratch.join("old")).unwrap();
        std::fs::create_dir(&root).unwrap();
        assert!(key.verify().is_err());
        assert_ne!(CheckoutKey::resolve(None, &root).unwrap(), key);
        std::fs::remove_dir(&root).unwrap();
        std::os::unix::fs::symlink(scratch.join("old"), &root).unwrap();
        assert!(key.verify().is_err());
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
