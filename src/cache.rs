use std::{
    fs::{self, File, FileTimes, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{IoContext, Result, invalid};

const CACHE_VERSION: &str = "soc-minis-artifacts-v1";

#[derive(Clone, Debug)]
pub struct ArtifactCache {
    root: PathBuf,
    budget: u64,
}

impl ArtifactCache {
    pub fn new(root: PathBuf, budget: u64) -> Result<Self> {
        if root.is_symlink() {
            return Err(invalid("The artifact cache must not be a symlink"));
        }
        fs::create_dir_all(&root).at_path("Cannot create", &root)?;
        Ok(Self { root, budget })
    }

    pub fn key<T: Serialize>(&self, kind: &str, semantic: &T) -> Result<String> {
        let mut digest = Sha256::new();
        digest.update(CACHE_VERSION);
        digest.update([0]);
        digest.update(kind);
        digest.update([0]);
        digest.update(serde_json::to_vec(&serde_json::to_value(semantic)?)?);
        Ok(format!("{:x}", digest.finalize()))
    }

    pub fn get_or_build(
        &self,
        key: &str,
        extension: &str,
        build: impl FnOnce() -> Result<Vec<u8>>,
    ) -> Result<Vec<u8>> {
        validate_name(key, extension)?;
        let path = self.root.join(format!("{key}.{extension}"));
        if path.is_file() && !path.is_symlink() {
            let metadata = path.metadata().at_path("Cannot inspect", &path)?;
            if metadata.len() > 0 && metadata.len() <= self.budget {
                let file = File::open(&path).at_path("Cannot open", &path)?;
                file.set_times(FileTimes::new().set_modified(SystemTime::now()))
                    .at_path("Cannot refresh", &path)?;
                return fs::read(&path).at_path("Cannot read", &path);
            }
            fs::remove_file(&path).at_path("Cannot invalidate", &path)?;
        }
        let bytes = build()?;
        if bytes.is_empty() {
            return Err(invalid("Refusing to cache an empty artifact"));
        }
        let temporary = self.root.join(format!(".{key}.{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .at_path("Cannot create", &temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .at_path("Cannot protect", &temporary)?;
        }
        if let Err(error) = file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .at_path("Cannot write", &temporary)
        {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        match fs::rename(&temporary, &path) {
            Ok(()) => {}
            Err(error) if path.is_file() => {
                let _ = fs::remove_file(&temporary);
                let _ = error;
            }
            Err(source) => {
                let _ = fs::remove_file(&temporary);
                return Err(crate::error::SocError::Io {
                    context: format!("Cannot publish {}", path.display()),
                    source,
                });
            }
        }
        self.prune()?;
        Ok(bytes)
    }

    pub fn prune(&self) -> Result<()> {
        let mut entries = fs::read_dir(&self.root)
            .at_path("Cannot inspect", &self.root)?
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let metadata = entry.metadata().ok()?;
                (metadata.is_file() && !path.is_symlink() && cache_name(&path)).then_some((
                    path,
                    metadata.len(),
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                ))
            })
            .collect::<Vec<_>>();
        let mut total = entries.iter().map(|(_, size, _)| size).sum::<u64>();
        if total <= self.budget {
            return Ok(());
        }
        entries.sort_by_key(|(_, _, modified)| *modified);
        for (path, size, _) in entries {
            if total <= self.budget {
                break;
            }
            fs::remove_file(&path).at_path("Cannot evict", &path)?;
            total = total.saturating_sub(size);
        }
        Ok(())
    }
}

fn validate_name(key: &str, extension: &str) -> Result<()> {
    if key.len() != 64
        || !key.bytes().all(|byte| byte.is_ascii_hexdigit())
        || extension.is_empty()
        || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(invalid("Invalid artifact cache key"));
    }
    Ok(())
}

fn cache_name(path: &Path) -> bool {
    path.file_stem()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        && path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| {
                !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
}

pub fn write_private(path: &Path, bytes: &[u8], force: bool) -> Result<()> {
    if path.exists() && !force {
        return Err(invalid(format!(
            "Refusing to replace existing output {}",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).at_path("Cannot create", parent)?;
    }
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("output"),
        Uuid::new_v4()
    ));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .at_path("Cannot create", &temporary)?;
    if let Err(error) = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .at_path("Cannot write", &temporary)
    {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if force && path.is_file() {
        fs::remove_file(path).at_path("Cannot replace", path)?;
    }
    fs::rename(&temporary, path).at_path("Cannot publish", path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[test]
    fn semantic_key_reuses_equal_inputs_and_invalidates_changes() {
        let root = std::env::temp_dir().join(format!("soc-cache-{}", Uuid::new_v4()));
        let cache = ArtifactCache::new(root.clone(), 1024).expect("cache");
        let first = cache.key("risk", &("event", 42)).expect("key");
        let same = cache.key("risk", &("event", 42)).expect("key");
        let changed = cache.key("risk", &("event", 43)).expect("key");
        assert_eq!(first, same);
        assert_ne!(first, changed);
        let calls = AtomicUsize::new(0);
        for _ in 0..2 {
            let bytes = cache
                .get_or_build(&first, "docx", || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok(b"document".to_vec())
                })
                .expect("artifact");
            assert_eq!(bytes, b"document");
        }
        cache
            .get_or_build(&changed, "docx", || {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(b"changed document".to_vec())
            })
            .expect("invalidated artifact");
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        fs::remove_dir_all(root).expect("clean cache");
    }

    #[test]
    fn semantic_key_canonicalises_maps() {
        let root = std::env::temp_dir().join(format!("soc-cache-{}", Uuid::new_v4()));
        let cache = ArtifactCache::new(root.clone(), 1024).expect("cache");
        let first = HashMap::from([("event", 42), ("year", 2026)]);
        let reordered = HashMap::from([("year", 2026), ("event", 42)]);
        assert_eq!(
            cache.key("risk", &first).expect("key"),
            cache.key("risk", &reordered).expect("key")
        );
        fs::remove_dir_all(root).expect("clean cache");
    }

    #[test]
    fn invalid_cached_artifact_is_rebuilt() {
        let root = std::env::temp_dir().join(format!("soc-cache-{}", Uuid::new_v4()));
        let cache = ArtifactCache::new(root.clone(), 1024).expect("cache");
        let key = cache.key("risk", &"event").expect("key");
        fs::write(root.join(format!("{key}.docx")), []).expect("empty artifact");
        assert_eq!(
            cache
                .get_or_build(&key, "docx", || Ok(b"document".to_vec()))
                .expect("rebuilt artifact"),
            b"document"
        );
        fs::remove_dir_all(root).expect("clean cache");
    }
}
