use std::path::Path;
use std::process::Command;

/// Returns true if some process currently has the file open.
///
/// Uses `lsof -t -- <path>` on unix. On non-unix or if `lsof` is unavailable,
/// returns `Ok(false)` and emits a warning to stderr (best-effort detection).
pub fn is_open(path: &Path) -> std::io::Result<bool> {
    if !cfg!(unix) {
        return Ok(false);
    }
    let out = match Command::new("lsof").arg("-t").arg("--").arg(path).output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("warning: lsof unavailable ({e}); skipping concurrent-open check");
            return Ok(false);
        }
    };
    // lsof exit codes: 0 with stdout = open by some pid; 1 = not open.
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout);
        Ok(!s.trim().is_empty())
    } else {
        Ok(false)
    }
}
