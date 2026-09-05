//! User-defined smart group rules (issue #07): predicates over per-root
//! status that collect repos into custom sidebar groups alongside the
//! built-ins.
//!
//! Plain data + pure logic (the app crate's plain-data precedent, cf.
//! `diff_data`): rules serialize into `.turbogit/ui.ron` via
//! `persistence::UiPersist`, and the UI crate maps each sidebar row onto
//! [`RepoFacts`] to evaluate them. Patterns follow the protected-branch
//! convention (`services::sync_service::is_protected`): a trailing `*`
//! matches by prefix, a leading `*` by suffix, no wildcard matches by
//! case-insensitive substring; blank means "any" (the editor normalizes
//! blank input to `None`).

/// One pattern against one text, per the module-doc convention: `*`
/// suffix → prefix match, `*` prefix → suffix match, otherwise
/// case-insensitive substring. Blank matches anything.
pub fn pattern_matches(pattern: &str, text: &str) -> bool {
    let p = pattern.trim().to_lowercase();
    let t = text.to_lowercase();
    if let Some(prefix) = p.strip_suffix('*') {
        t.starts_with(prefix)
    } else if let Some(suffix) = p.strip_prefix('*') {
        t.ends_with(suffix)
    } else {
        t.contains(&p)
    }
}

/// The per-root facts a rule's predicate reads. The UI crate projects each
/// sidebar row onto this; keeping it plain keeps the rule logic egui-free
/// and unit-testable here.
#[derive(Clone, Debug)]
pub struct RepoFacts {
    pub name: String,
    pub path: String,
    /// Current branch, `None` when detached.
    pub branch: Option<String>,
    /// Modified + unversioned paths.
    pub dirty: usize,
    /// Outgoing commits vs upstream.
    pub ahead: usize,
    /// Incoming commits vs upstream.
    pub behind: usize,
}

/// One user-defined smart group: a label plus optionally-constrained
/// predicates, all of which must hold for a repo to be a member.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SmartGroupRule {
    /// The sidebar row's label.
    pub label: String,
    /// Name/path pattern (`None` or blank = any repo).
    pub path_pattern: Option<String>,
    /// Current-branch pattern (`None` or blank = any branch; a detached
    /// repo matches only when unconstrained).
    pub branch_pattern: Option<String>,
    /// Require at least N uncommitted paths.
    pub min_dirty: Option<usize>,
    /// Require at least N outgoing commits.
    pub min_ahead: Option<usize>,
    /// Require at least N incoming commits.
    pub min_behind: Option<usize>,
}

/// Blank (or whitespace-only) patterns are stored by the editor but read
/// as "any".
fn constrained(pattern: &Option<String>) -> Option<&str> {
    pattern.as_deref().map(str::trim).filter(|p| !p.is_empty())
}

impl SmartGroupRule {
    /// The rule's predicate: every present constraint must hold.
    pub fn matches(&self, repo: &RepoFacts) -> bool {
        if let Some(p) = constrained(&self.path_pattern) {
            let basename = std::path::Path::new(&repo.path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if !(pattern_matches(p, &repo.name)
                || pattern_matches(p, &repo.path)
                || pattern_matches(p, basename))
            {
                return false;
            }
        }
        if let Some(p) = constrained(&self.branch_pattern) {
            match &repo.branch {
                Some(branch) if pattern_matches(p, branch) => {}
                _ => return false,
            }
        }
        if self.min_dirty.is_some_and(|n| repo.dirty < n) {
            return false;
        }
        if self.min_ahead.is_some_and(|n| repo.ahead < n) {
            return false;
        }
        if self.min_behind.is_some_and(|n| repo.behind < n) {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_match_prefix_suffix_substring_and_case() {
        assert!(pattern_matches("release/*", "release/2.1"), "prefix");
        assert!(!pattern_matches("release/*", "main"));
        assert!(pattern_matches("*-gui", "tools-gui"), "suffix");
        assert!(!pattern_matches("*-gui", "gui-tools"));
        assert!(pattern_matches("lib", "/w/oss/lib"), "substring");
        assert!(
            pattern_matches("RELEASE/*", "release/2.1"),
            "case-insensitive"
        );
        assert!(pattern_matches("*", "anything"), "bare star matches all");
        assert!(pattern_matches("", "anything"), "blank means any");
    }

    #[test]
    fn trailing_star_matches_only_by_prefix() {
        // "release/*" must not match a repo merely containing the text.
        assert!(!pattern_matches("release/*", "forks/release-tools"));
    }

    fn facts(name: &str, path: &str, branch: Option<&str>) -> RepoFacts {
        RepoFacts {
            name: name.to_string(),
            path: path.to_string(),
            branch: branch.map(str::to_string),
            dirty: 0,
            ahead: 0,
            behind: 0,
        }
    }

    #[test]
    fn a_rule_without_predicates_matches_every_repo() {
        let rule = SmartGroupRule {
            label: "everything".into(),
            ..SmartGroupRule::default()
        };
        assert!(rule.matches(&facts("anything", "/w/anything", Some("main"))));
    }

    #[test]
    fn path_pattern_matches_name_or_full_path() {
        let rule = SmartGroupRule {
            label: "release".into(),
            path_pattern: Some("release-*".into()),
            ..SmartGroupRule::default()
        };
        assert!(
            rule.matches(&facts("release-2", "/w/oss/other", Some("main"))),
            "by name"
        );
        assert!(
            rule.matches(&facts("other", "/w/oss/release-2", Some("main"))),
            "by path"
        );
        assert!(!rule.matches(&facts("other", "/w/oss/lib", Some("main"))));
    }

    #[test]
    fn branch_pattern_requires_a_current_branch() {
        let rule = SmartGroupRule {
            label: "release".into(),
            branch_pattern: Some("release/*".into()),
            ..SmartGroupRule::default()
        };
        assert!(rule.matches(&facts("lib", "/w/lib", Some("release/2"))));
        assert!(!rule.matches(&facts("lib", "/w/lib", Some("main"))));
        assert!(
            !rule.matches(&facts("lib", "/w/lib", None)),
            "a detached repo has no branch to match"
        );
    }

    #[test]
    fn thresholds_need_at_least_the_given_counts() {
        let rule = SmartGroupRule {
            label: "wip".into(),
            min_dirty: Some(2),
            min_ahead: Some(1),
            ..SmartGroupRule::default()
        };
        assert!(rule.matches(&RepoFacts {
            dirty: 2,
            ahead: 3,
            ..facts("a", "/w/a", Some("main"))
        }));
        assert!(
            !rule.matches(&RepoFacts {
                dirty: 1,
                ahead: 3,
                ..facts("a", "/w/a", Some("main"))
            }),
            "below the dirty threshold"
        );
        assert!(
            !rule.matches(&RepoFacts {
                dirty: 5,
                ahead: 0,
                ..facts("a", "/w/a", Some("main"))
            }),
            "below the ahead threshold"
        );
    }

    #[test]
    fn predicates_compose_and_blank_patterns_mean_any() {
        let rule = SmartGroupRule {
            label: "releases".into(),
            path_pattern: Some("".into()),
            branch_pattern: Some("release/*".into()),
            min_behind: Some(1),
            ..SmartGroupRule::default()
        };
        assert!(rule.matches(&RepoFacts {
            behind: 4,
            ..facts("lib", "/w/lib", Some("release/2"))
        }));
        assert!(!rule.matches(&RepoFacts {
            behind: 0,
            ..facts("lib", "/w/lib", Some("release/2"))
        }));
    }
}
