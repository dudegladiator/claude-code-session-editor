use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SaveError {
    #[error("file is open by another process; close Claude Code first or pass --force")]
    Conflict,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct SaveOutcome {
    pub backup: PathBuf,
}

/// Save `content` to `path` atomically:
///   1. If `!force`, abort when lsof reports the file open.
///   2. Copy current file to `<path>.bak` (overwrite).
///   3. Write `<path>.tmp`, fsync.
///   4. Rename `<path>.tmp` -> `<path>`.
///
/// If `path` does not yet exist, the backup step is skipped.
pub fn save(path: &Path, content: &str, force: bool) -> Result<SaveOutcome, SaveError> {
    if !force && super::lsof::is_open(path)? {
        return Err(SaveError::Conflict);
    }

    let backup = with_extension_appended(path, "bak");
    if path.exists() {
        fs::copy(path, &backup)?;
    }

    let tmp = with_extension_appended(path, "tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;

    Ok(SaveOutcome { backup })
}

fn with_extension_appended(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn save_creates_backup_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(&path, "old").unwrap();

        let outcome = save(&path, "new", true).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::read_to_string(&outcome.backup).unwrap(), "old");
    }

    #[test]
    fn save_overwrites_existing_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(&path, "v1").unwrap();
        save(&path, "v2", true).unwrap();
        save(&path, "v3", true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "v3");
        assert_eq!(
            fs::read_to_string(path.with_file_name("session.jsonl.bak")).unwrap(),
            "v2"
        );
    }

    #[test]
    fn save_to_new_file_skips_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fresh.jsonl");
        let outcome = save(&path, "content", true).unwrap();
        assert!(!outcome.backup.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), "content");
    }
}
