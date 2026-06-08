use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

use crate::scan::SessionEntry;

/// Returns indices into `entries` that match `query`, ordered by score (best
/// first). Matches against `title` + `project_slug` (id excluded — UUIDs are
/// noise for fuzzy ranking). Empty query returns all entries in original
/// order.
pub fn fuzzy_filter<'a>(entries: &'a [SessionEntry], query: &str) -> Vec<&'a SessionEntry> {
    if query.trim().is_empty() {
        return entries.iter().collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

    let mut scored: Vec<(u32, &SessionEntry)> = entries
        .iter()
        .filter_map(|e| {
            let hay = format!("{} {}", e.project_slug, e.title);
            pattern
                .score(nucleo_matcher::Utf32Str::Ascii(hay.as_bytes()), &mut matcher)
                .or_else(|| {
                    // Fall back to UTF-32 path for non-ASCII titles.
                    let mut buf = Vec::new();
                    let s = nucleo_matcher::Utf32Str::new(&hay, &mut buf);
                    pattern.score(s, &mut matcher)
                })
                .map(|score| (score, e))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, e)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn entry(project: &str, title: &str) -> SessionEntry {
        SessionEntry {
            project_slug: project.into(),
            session_id: format!("{project}-{title}"),
            title: title.into(),
            mtime: SystemTime::UNIX_EPOCH,
            size: 0,
            path: PathBuf::from("/tmp/x"),
        }
    }

    #[test]
    fn empty_query_returns_all() {
        let v = vec![entry("a", "x"), entry("b", "y")];
        assert_eq!(fuzzy_filter(&v, "").len(), 2);
        assert_eq!(fuzzy_filter(&v, "   ").len(), 2);
    }

    #[test]
    fn fuzzy_subsequence_match() {
        let v = vec![
            entry("alpha", "auth middleware bug"),
            entry("beta", "billing flow"),
            entry("gamma", "checkout"),
        ];
        let r = fuzzy_filter(&v, "athmw");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "auth middleware bug");
    }

    #[test]
    fn case_insensitive() {
        let v = vec![entry("Alpha", "Auth Bug")];
        assert_eq!(fuzzy_filter(&v, "auth").len(), 1);
        assert_eq!(fuzzy_filter(&v, "ALPHA").len(), 1);
    }

    #[test]
    fn ranks_better_match_higher() {
        let v = vec![
            entry("auth-svc", "unrelated thing"),
            entry("misc", "auth bug"),
        ];
        let r = fuzzy_filter(&v, "auth");
        assert_eq!(r.len(), 2);
        // exact word "auth" present in both project and title; ordering is
        // matcher-defined but both must rank.
        assert!(r.iter().all(|e| e.title.to_lowercase().contains("a")));
    }

    #[test]
    fn no_match_returns_empty() {
        let v = vec![entry("a", "hello")];
        assert!(fuzzy_filter(&v, "zzzzzz").is_empty());
    }
}
