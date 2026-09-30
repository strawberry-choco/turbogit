//! Commit-log paging at the engine seam: `LogOpts::skip` asks for a **page**
//! of the listing instead of a prefix of it — discard this many entries from
//! the front, then return up to `max_count`. The walk always starts at HEAD,
//! so the union of the pages is a prefix of git's own list and cannot have
//! holes behind a merge's second parent.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use test_support::git_seed::git;
use turbogit_domain::model::{Commit, LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::git2_exec::Git2Executor;
use turbogit_engine_api::GitExecutor;

/// Commits per page — the app's `LOG_BATCH_SIZE`. The engine sits below
/// `turbogit-app`, so the size is restated here rather than imported.
const PAGE: usize = 50;

/// Commits in the shared linear fixture: two full pages plus a short tail.
const TOTAL: usize = 2 * PAGE + 3;

/// `git` with an author/committer epoch pinned PER CALL, so a fixture can give
/// every commit its own timestamp (see [`commit_at`]).
///
/// Kept local because `test_support::git_seed::git` takes no per-call
/// environment. The env is load-bearing, not redundant: `merge_repo`'s topology
/// (an octopus merge over a criss-cross, i.e. two merge bases) is only comparable
/// between the CLI walk and libgit2's revwalk if every commit carries an
/// unambiguous newest-first timestamp. Without the pin, same-second commits tie
/// and the order under test becomes the order of two different tie-breaks. No
/// `git config` can stand in: `user.*` is identity, not time.
fn git_at_epoch(dir: &Path, args: &[&str], epoch: u64) -> String {
    let mut cmd = std::process::Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("GIT_AUTHOR_DATE", format!("@{epoch}"))
        .env("GIT_COMMITTER_DATE", format!("@{epoch}"));
    let out = cmd.output().expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn init(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@t"]);
    git(dir, &["config", "user.name", "t"]);
}

fn commit(dir: &Path, msg: &str) -> String {
    git(dir, &["commit", "-q", "--allow-empty", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// A commit whose author AND committer epochs are pinned to `epoch`.
/// Same-second commits are ordered differently by `git log` and by libgit2's
/// revwalk, so any fixture that claims the two backends agree on ORDER has to
/// Hence [`git_at_epoch`], which the shared runner cannot stand in for.
fn commit_at(dir: &Path, msg: &str, epoch: u64) {
    git_at_epoch(dir, &["commit", "-q", "--allow-empty", "-m", msg], epoch);
}

fn cli() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn git2() -> Git2Executor {
    Git2Executor::new(cli())
}

/// Every paging case runs against both adapters, over the same repositories:
/// the two backends' walk orders are equivalent only by convention, so a page
/// boundary that holds for one has to be proved for the other.
fn backends() -> Vec<(&'static str, Box<dyn GitExecutor>)> {
    vec![("cli", Box::new(cli())), ("git2", Box::new(git2()))]
}

fn ids(commits: &[Commit]) -> Vec<String> {
    commits.iter().map(|c| c.id.clone()).collect()
}

/// The whole listing, uncapped — the oracle every page is checked against.
fn uncapped(exec: &dyn GitExecutor, repo: &Path, base: &LogOpts) -> Vec<String> {
    ids(&exec.log(repo, base).expect("uncapped log"))
}

/// Walk `base`'s listing page by page (`PAGE` at a time) and return the
/// concatenated ids. A page that came back short ends the walk; the iteration
/// bound turns a backend that ignores `skip` — and so answers every request
/// with the same first page — into a failure instead of a hang.
fn paged(exec: &dyn GitExecutor, repo: &Path, base: &LogOpts) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = 0;
    for _ in 0..10 {
        let page = exec
            .log(
                repo,
                &LogOpts {
                    skip: Some(skip),
                    ..base.clone()
                },
            )
            .expect("page");
        assert!(
            page.len() <= PAGE,
            "a page never exceeds max_count ({} > {PAGE})",
            page.len()
        );
        if page.is_empty() {
            return out;
        }
        skip += page.len();
        out.extend(ids(&page));
    }
    panic!("paging never reached the end of the listing");
}

/// One shared fixture for every page-shape case: `TOTAL` commits, of which
/// the even-numbered ones touch `tracked.txt` so a path-scoped listing still
/// spans more than one page. Seeding costs seconds, so it happens once, and
/// every case only ever reads it.
fn linear_repo() -> &'static PathBuf {
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("linear");
        init(&repo);
        for i in 0..TOTAL {
            if i % 2 == 0 {
                std::fs::write(repo.join("tracked.txt"), format!("revision {i}\n"))
                    .expect("write tracked.txt");
                git(&repo, &["add", "tracked.txt"]);
            }
            commit(&repo, &format!("c{i}"));
        }
        // A branch partway back whose own listing spans pages, so branch
        // scoping can be paged without moving HEAD.
        git(&repo, &["branch", "half", &format!("HEAD~{PAGE}")]);
        // The temp dir outlives every test in this binary.
        std::mem::forget(dir);
        repo
    })
}

/// A history that is not a chain: a `--no-ff` branch merge plus an octopus
/// merge of two more branches. Merge topology is where an offset walk and a
/// graph walk can disagree, so backend parity is proved here as well. Every
/// commit gets its own epoch — later than all its ancestors — so the fixture
/// pins one unambiguous newest-first order instead of leaving same-second ties
/// to each backend's own tie-break.
fn merge_repo() -> &'static PathBuf {
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("merged");
        init(&repo);
        // Every commit gets its own epoch, later than all its ancestors.
        let mut clock = 1_700_000_000u64;
        for i in 0..25 {
            clock += 1;
            commit_at(&repo, &format!("base{i}"), clock);
        }
        git(&repo, &["checkout", "-q", "-b", "feature"]);
        for i in 0..10 {
            clock += 1;
            commit_at(&repo, &format!("feat{i}"), clock);
        }
        git(&repo, &["checkout", "-q", "main"]);
        for i in 0..10 {
            clock += 1;
            commit_at(&repo, &format!("main{i}"), clock);
        }
        clock += 1;
        git_at_epoch(
            &repo,
            &["merge", "--no-ff", "-m", "merge feature", "feature"],
            clock,
        );
        git(&repo, &["checkout", "-q", "-b", "octo1"]);
        for i in 0..6 {
            clock += 1;
            commit_at(&repo, &format!("octo1-{i}"), clock);
        }
        git(&repo, &["checkout", "-q", "main"]);
        git(&repo, &["checkout", "-q", "-b", "octo2"]);
        for i in 0..6 {
            clock += 1;
            commit_at(&repo, &format!("octo2-{i}"), clock);
        }
        git(&repo, &["checkout", "-q", "main"]);
        clock += 1;
        git_at_epoch(
            &repo,
            &["merge", "--no-ff", "-m", "octopus", "octo1", "octo2"],
            clock,
        );

        // Criss-cross on top: two branches that each merge the other's tip,
        // so they share TWO merge bases — the topology where a walk's parent
        // ordering is genuinely under-determined.
        let fork = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
        git(&repo, &["checkout", "-q", "-b", "cross-a", &fork]);
        clock += 1;
        commit_at(&repo, "a1", clock);
        let a1 = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
        git(&repo, &["checkout", "-q", "-b", "cross-b", &fork]);
        clock += 1;
        commit_at(&repo, "b1", clock);
        let b1 = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
        clock += 1;
        git_at_epoch(&repo, &["merge", "--no-ff", "-m", "b merges a", &a1], clock);
        git(&repo, &["checkout", "-q", "cross-a"]);
        clock += 1;
        git_at_epoch(&repo, &["merge", "--no-ff", "-m", "a merges b", &b1], clock);
        git(&repo, &["checkout", "-q", "main"]);
        clock += 1;
        git_at_epoch(
            &repo,
            &[
                "merge",
                "--no-ff",
                "-m",
                "octopus over the criss-cross",
                "cross-a",
                "cross-b",
            ],
            clock,
        );
        std::mem::forget(dir);
        repo
    })
}

#[test]
fn pages_concatenate_to_the_uncapped_log() {
    let repo = linear_repo();
    for (backend, exec) in backends() {
        let all = uncapped(&*exec, repo, &LogOpts::default());
        assert_eq!(all.len(), TOTAL, "{backend}: two pages plus a tail");

        let paged = paged(
            &*exec,
            repo,
            &LogOpts {
                max_count: Some(PAGE),
                ..Default::default()
            },
        );
        assert_eq!(
            paged, all,
            "{backend}: pages taken in order must be exactly the uncapped listing"
        );
    }
}

/// Paging is orthogonal to filtering: the discard applies to the already
/// filtered listing, so a page of a scoped listing is the matching slice of
/// the scoped whole.
#[test]
fn skip_pages_scoped_listings_the_same_way() {
    let repo = linear_repo();
    for (backend, exec) in backends() {
        for base in [
            LogOpts {
                branch: Some("half".to_string()),
                max_count: Some(PAGE),
                ..Default::default()
            },
            LogOpts {
                path: Some(PathBuf::from("tracked.txt")),
                max_count: Some(PAGE),
                ..Default::default()
            },
        ] {
            let whole = {
                let mut unscoped = base.clone();
                unscoped.max_count = None;
                unscoped.skip = None;
                uncapped(&*exec, repo, &unscoped)
            };
            assert!(
                whole.len() > PAGE,
                "{backend}: the scoped listing must span pages ({} entries)",
                whole.len()
            );
            assert_eq!(
                paged(&*exec, repo, &base),
                whole,
                "{backend}: pages of a scoped listing rebuild the scoped whole"
            );
        }
    }
}

/// The boundary contract the app pages on: it requests `skip = have - 1` with
/// `max_count = PAGE + 1`, where the page's first entry must be the commit it
/// already holds at that position — the checksum row, which the app then
/// drops. If the walk ever started elsewhere than HEAD this would break.
#[test]
fn an_overlapping_page_starts_on_the_commit_the_app_already_holds() {
    let repo = linear_repo();
    for (backend, exec) in backends() {
        let all = uncapped(&*exec, repo, &LogOpts::default());
        for have in [1, PAGE, PAGE + 1, all.len() - 1] {
            let page = exec
                .log(
                    repo,
                    &LogOpts {
                        max_count: Some(PAGE + 1),
                        skip: Some(have - 1),
                        ..Default::default()
                    },
                )
                .expect("overlapping page");
            assert_eq!(
                page.len(),
                (all.len() - (have - 1)).min(PAGE + 1),
                "{backend}: the page carries the anchor row plus up to {PAGE} new rows"
            );
            assert_eq!(
                ids(&page).first(),
                Some(&all[have - 1]),
                "{backend}: entry {} of the listing leads the page",
                have - 1
            );
            assert_eq!(
                ids(&page[1..]),
                all[have..have + page.len() - 1].to_vec(),
                "{backend}: everything after the anchor is what follows it"
            );
        }
    }
}

/// A short page is the authoritative end of history (`has_more` is derived
/// from it), so the boundary has to be exact: `TOTAL` = 103 is two full pages
/// and a tail of 3, and the page after the tail is empty — not an error.
#[test]
fn a_short_page_ends_history_and_the_page_after_it_is_empty() {
    let repo = linear_repo();
    for (backend, exec) in backends() {
        let page = |skip: usize| {
            exec.log(
                repo,
                &LogOpts {
                    max_count: Some(PAGE),
                    skip: Some(skip),
                    ..Default::default()
                },
            )
            .expect("page")
        };

        let tail = page(2 * PAGE);
        assert_eq!(
            tail.len(),
            TOTAL - 2 * PAGE,
            "{backend}: the last page is short, and short means final"
        );
        assert!(
            page(TOTAL).is_empty(),
            "{backend}: the page after the last is empty"
        );
        assert!(
            page(TOTAL * 10).is_empty(),
            "{backend}: skipping past the end of history is empty, not an error"
        );
    }
}

/// The adapters' newest-first orders are equivalent only by convention, so
/// paging is proved as an equality between them: the same pages in the same
/// order, on a history with a merge and an octopus merge, and on a
/// path-filtered listing where the filter has to run before the discard.
#[test]
fn both_backends_cut_the_same_pages() {
    let page_window = LogOpts {
        max_count: Some(PAGE),
        ..Default::default()
    };
    let repo = merge_repo();

    let cli_pages = paged(&cli(), repo, &page_window);
    let git2_pages = paged(&git2(), repo, &page_window);
    assert!(
        cli_pages.len() > PAGE,
        "the merged fixture spans more than one page ({} entries)",
        cli_pages.len()
    );
    assert_eq!(
        cli_pages, git2_pages,
        "page-by-page walks agree across backends on merged history"
    );
    assert_eq!(
        cli_pages,
        uncapped(&cli(), repo, &LogOpts::default()),
        "cli: pages rebuild the uncapped listing"
    );
    assert_eq!(
        git2_pages,
        uncapped(&git2(), repo, &LogOpts::default()),
        "git2: pages rebuild the uncapped listing"
    );

    let scoped = LogOpts {
        path: Some(PathBuf::from("tracked.txt")),
        max_count: Some(PAGE),
        ..Default::default()
    };
    assert_eq!(
        paged(&cli(), linear_repo(), &scoped),
        paged(&git2(), linear_repo(), &scoped),
        "a path-filtered page is identical across backends"
    );
}
