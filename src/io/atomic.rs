use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Write `content` to `path` atomically: write `<path>.tmp`, fsync, then
/// rename. Used by fork so partial writes never appear at the destination.
pub fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = with_extension_appended(path, "tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
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
    fn write_atomic_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        write_atomic(&path, "hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn write_atomic_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        write_atomic(&path, "v1").unwrap();
        write_atomic(&path, "v2").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "v2");
    }
}
