use crate::scan::SessionEntry;

pub fn matches(entry: &SessionEntry, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let q = query.to_lowercase();
    let hay = format!(
        "{} {} {}",
        entry.project_slug.to_lowercase(),
        entry.title.to_lowercase(),
        entry.session_id.to_lowercase()
    );
    hay.contains(&q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn entry(project: &str, title: &str) -> SessionEntry {
        SessionEntry {
            project_slug: project.into(),
            session_id: "uuid-1".into(),
            title: title.into(),
            mtime: SystemTime::UNIX_EPOCH,
            size: 0,
            path: PathBuf::from("/tmp/x"),
        }
    }

    #[test]
    fn empty_query_matches_all() {
        assert!(matches(&entry("p", "t"), ""));
    }

    #[test]
    fn case_insensitive_substring() {
        let e = entry("MyProject", "Auth Middleware Bug");
        assert!(matches(&e, "auth middle"));
        assert!(matches(&e, "myproject"));
    }

    #[test]
    fn no_match() {
        let e = entry("p", "t");
        assert!(!matches(&e, "xyz"));
    }
}
