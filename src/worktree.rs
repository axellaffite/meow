//! Worktrees for autopilot jobs: the user's clone of a GitHub repository is
//! found on disk, the branch fetched, and a worktree added at its tip, so the
//! work never touches the user's own checkout.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::autopilot::Folder;

/// How deep under each root clones are looked for.
const MAX_DEPTH: usize = 3;
/// Where clones usually live, under the home folder.
const ROOTS: &[&str] = &[
    "Developer",
    "Projects",
    "Code",
    "code",
    "src",
    "dev",
    "git",
    "repos",
    "workspace",
    "Documents/GitHub",
];
const SKIPPED: &[&str] = &["node_modules", "target", "build", "dist", "Pods", "vendor"];
const ERROR_TAIL_LEN: usize = 400;

/// `MEOW_CODE_DIRS` (colon-separated), else the usual spots under `$HOME`.
pub fn roots() -> Vec<PathBuf> {
    if let Some(dirs) = std::env::var_os("MEOW_CODE_DIRS") {
        return std::env::split_paths(&dirs).collect();
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    ROOTS.iter().map(|dir| home.join(dir)).collect()
}

/// `owner/name` from a GitHub remote URL (`git@github.com:o/r.git`,
/// `https://github.com/o/r`, `ssh://git@github.com/o/r.git`), lowercased.
pub fn github_repo(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = url
        .split_once("github.com:")
        .or_else(|| url.split_once("github.com/"))?
        .1;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !name.is_empty()).then(|| format!("{owner}/{name}").to_lowercase())
}

/// The remotes in a clone's `.git/config`, as (name, URL).
fn remotes(config: &str) -> Vec<(String, String)> {
    let mut remotes = Vec::new();
    let mut current: Option<String> = None;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            current = line
                .strip_prefix("[remote \"")
                .and_then(|rest| rest.strip_suffix("\"]"))
                .map(str::to_string);
        } else if let Some(name) = &current
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "url"
        {
            remotes.push((name.clone(), value.trim().to_string()));
        }
    }
    remotes
}

/// A clone of `repo` (`owner/name`) under `roots`, with the remote pointing at it.
pub fn find_clone(repo: &str, roots: &[PathBuf]) -> Option<(PathBuf, String)> {
    let wanted = repo.to_lowercase();
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let config = dir.join(".git/config");
        if let Ok(text) = std::fs::read_to_string(&config) {
            if let Some((name, _)) = remotes(&text)
                .into_iter()
                .find(|(_, url)| github_repo(url).as_deref() == Some(wanted.as_str()))
            {
                return Some((dir, name));
            }
            // A clone's folders are not other clones.
            continue;
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || SKIPPED.contains(&name.as_ref()) {
                continue;
            }
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push((entry.path(), depth + 1));
            }
        }
    }
    None
}

/// A pull request's head and base, from `gh pr view`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    /// `owner/name` of the repository its branch lives in (a fork, maybe).
    pub head_repo: String,
    pub branch: String,
    pub base: String,
    pub url: String,
}

fn parse_pull_request(json: &str) -> Result<PullRequest, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| format!("Unexpected answer from gh: {e}"))?;
    let text = |pointer: &str| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("gh did not give the pull request's {pointer}"))
    };
    Ok(PullRequest {
        head_repo: format!(
            "{}/{}",
            text("/headRepositoryOwner/login")?,
            text("/headRepository/name")?
        ),
        branch: text("/headRefName")?,
        base: text("/baseRefName")?,
        url: text("/url")?,
    })
}

/// The repositories, from `gh search prs`, holding a pull request numbered `number`.
fn repos_numbered(json: &str, number: &str) -> Result<Vec<String>, String> {
    let found: Vec<Value> =
        serde_json::from_str(json).map_err(|e| format!("Unexpected answer from gh: {e}"))?;
    Ok(found
        .iter()
        .filter(|pr| pr.get("number").map(Value::to_string).as_deref() == Some(number))
        .filter_map(|pr| {
            pr.pointer("/repository/nameWithOwner")
                .and_then(Value::as_str)
        })
        .map(str::to_string)
        .collect())
}

/// The commands a worktree is made with.
pub struct Tools {
    /// PATH for `git` and `gh`.
    pub path_var: String,
    pub roots: Vec<PathBuf>,
}

impl Tools {
    /// `pr` (URL or number) of `repo`. Without `repo`, a number is looked for
    /// among the user's open pull requests: Meow runs outside any clone, so
    /// `gh` cannot tell the repository from the folder.
    pub fn pull_request(&self, pr: &str, repo: Option<&str>) -> Result<PullRequest, String> {
        let pr = pr.trim().trim_start_matches('#');
        let repo = match repo {
            _ if pr.contains("github.com") => None,
            Some(repo) => Some(repo.to_string()),
            None => Some(self.repo_of(pr)?),
        };
        let mut args = vec!["pr", "view", pr];
        if let Some(repo) = &repo {
            args.extend(["--repo", repo.as_str()]);
        }
        args.extend([
            "--json",
            "headRefName,baseRefName,headRepository,headRepositoryOwner,url",
        ]);
        parse_pull_request(&self.run("gh", None, &args)?)
    }

    /// The repository of the open pull request `number` the user wrote, is
    /// involved in, or is asked to review.
    fn repo_of(&self, number: &str) -> Result<String, String> {
        let mut repos = Vec::new();
        for who in ["--involves=@me", "--review-requested=@me"] {
            let json = self.run(
                "gh",
                None,
                &[
                    "search",
                    "prs",
                    who,
                    "--state=open",
                    "--limit=100",
                    "--json=number,repository",
                ],
            )?;
            repos.extend(repos_numbered(&json, number)?);
        }
        repos.sort();
        repos.dedup();
        match repos.as_slice() {
            [repo] => Ok(repo.clone()),
            [] => Err(format!(
                "No repository was named for pull request #{number}, and none of your open \
                 pull requests has that number."
            )),
            _ => Err(format!(
                "No repository was named for pull request #{number}, and it could be in {}.",
                repos.join(", ")
            )),
        }
    }

    /// A worktree at `dest` for the pull request `pr`, or `branch` of `repo`
    /// (its default branch when `None`).
    pub fn prepare(
        &self,
        repo: Option<&str>,
        pr: Option<&str>,
        branch: Option<&str>,
        dest: &Path,
    ) -> Result<Folder, String> {
        let pull = pr.map(|pr| self.pull_request(pr, repo)).transpose()?;
        let head_repo = pull
            .as_ref()
            .map(|p| p.head_repo.clone())
            .or_else(|| repo.map(str::to_string))
            .ok_or("No repository was named.")?;
        let (clone, remote) = find_clone(&head_repo, &self.roots).ok_or_else(|| {
            format!(
                "No clone of {head_repo} found under {} (set MEOW_CODE_DIRS).",
                self.roots
                    .iter()
                    .map(|r| r.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        let branch = match (&pull, branch) {
            (Some(pull), _) => pull.branch.clone(),
            (None, Some(branch)) => branch.to_string(),
            (None, None) => self.default_branch(&clone, &remote)?,
        };
        self.git(&clone, &["fetch", "--quiet", &remote, &branch])?;
        // A worktree of an earlier attempt, deleted since, would be in the way.
        self.git(&clone, &["worktree", "prune"])?;
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        }
        let tip = format!("refs/remotes/{remote}/{branch}");
        let dest_text = dest.display().to_string();
        self.git(&clone, &["worktree", "add", "--detach", &dest_text, &tip])?;
        Ok(Folder {
            path: dest.to_path_buf(),
            clone,
            remote,
            branch,
            base: pull.as_ref().map(|p| p.base.clone()),
            pr_url: pull.map(|p| p.url),
        })
    }

    /// Removes the worktree, uncommitted changes included.
    pub fn remove(&self, folder: &Folder) -> Result<(), String> {
        let path = folder.path.display().to_string();
        self.git(&folder.clone, &["worktree", "remove", "--force", &path])
            .map(|_| ())
    }

    fn default_branch(&self, clone: &Path, remote: &str) -> Result<String, String> {
        let out = self.git(clone, &["ls-remote", "--symref", remote, "HEAD"])?;
        out.lines()
            .find_map(|line| line.strip_prefix("ref: refs/heads/"))
            .and_then(|rest| rest.split_whitespace().next())
            .map(str::to_string)
            .ok_or_else(|| format!("Could not tell the default branch of {remote}."))
    }

    fn git(&self, clone: &Path, args: &[&str]) -> Result<String, String> {
        self.run("git", Some(clone), args)
    }

    fn run(&self, program: &str, dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
        let mut command = Command::new(program);
        command
            .args(args)
            .env("PATH", &self.path_var)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null());
        if let Some(dir) = dir {
            command.current_dir(dir);
        }
        let what = format!("{program} {}", args.first().copied().unwrap_or_default());
        let output = command
            .output()
            .map_err(|e| format!("Could not run {program}: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail: String = stderr.trim().chars().rev().take(ERROR_TAIL_LEN).collect();
            let detail: String = detail.chars().rev().collect();
            return Err(format!("{what} failed: {detail}"));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_urls_name_their_repository() {
        for url in [
            "git@github.com:Axel/Meow.git",
            "https://github.com/axel/meow",
            "https://github.com/axel/meow.git/",
            "ssh://git@github.com/axel/meow.git",
        ] {
            assert_eq!(github_repo(url).as_deref(), Some("axel/meow"), "{url}");
        }
        assert_eq!(github_repo("https://gitlab.com/a/b"), None);
    }

    #[test]
    fn clones_are_found_by_remote() {
        let root = std::env::temp_dir().join(format!("meow-worktree-{}", std::process::id()));
        let clone = root.join("work/meow");
        std::fs::create_dir_all(clone.join(".git")).unwrap();
        std::fs::write(
            clone.join(".git/config"),
            "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = git@github.com:axel/meow.git\n\
             [remote \"fork\"]\n\turl = https://github.com/cyril/meow.git\n",
        )
        .unwrap();
        let roots = vec![root.clone()];
        assert_eq!(
            find_clone("Cyril/meow", &roots),
            Some((clone.clone(), "fork".to_string()))
        );
        assert_eq!(
            find_clone("axel/meow", &roots),
            Some((clone, "origin".to_string()))
        );
        assert_eq!(find_clone("axel/other", &roots), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pull_requests_give_their_head() {
        let json = r#"{"baseRefName":"main","headRefName":"feat/x",
            "headRepository":{"id":"R1","name":"meow"},
            "headRepositoryOwner":{"id":"U1","login":"cyril"},
            "url":"https://github.com/axel/meow/pull/12"}"#;
        assert_eq!(
            parse_pull_request(json).unwrap(),
            PullRequest {
                head_repo: "cyril/meow".into(),
                branch: "feat/x".into(),
                base: "main".into(),
                url: "https://github.com/axel/meow/pull/12".into(),
            }
        );
    }

    #[test]
    fn pull_request_numbers_name_their_repositories() {
        let json = r#"[
            {"number":1727,"repository":{"name":"api","nameWithOwner":"o/api"}},
            {"number":17,"repository":{"name":"web","nameWithOwner":"o/web"}},
            {"number":1727,"repository":{"name":"app","nameWithOwner":"o/app"}}]"#;
        assert_eq!(
            repos_numbered(json, "1727").unwrap(),
            vec!["o/api", "o/app"]
        );
        assert_eq!(repos_numbered(json, "17").unwrap(), vec!["o/web"]);
        assert!(repos_numbered(json, "42").unwrap().is_empty());
        assert!(repos_numbered("oops", "1").is_err());
    }

    #[test]
    fn worktrees_are_added_at_the_branch_tip_and_removed() {
        let root = std::env::temp_dir().join(format!("meow-worktree-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let upstream = root.join("upstream.git");
        let clone = root.join("code/repo");
        let sh = |dir: &Path, script: &str| {
            let status = Command::new("sh")
                .args(["-c", script])
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "{script}");
        };
        std::fs::create_dir_all(&clone).unwrap();
        sh(&root, "git init -q --bare -b main upstream.git");
        sh(
            &clone,
            &format!(
                "git init -q -b main && git remote add origin {} && echo a > a && git add a \
                 && git commit -qm a && git push -q origin main && git checkout -qb feat/x \
                 && echo b > b && git add b && git commit -qm b && git push -q origin feat/x \
                 && git checkout -q main",
                upstream.display()
            ),
        );
        // A GitHub URL, as `find_clone` matches on: fetch through the bare repository.
        sh(
            &clone,
            &format!(
                "git config remote.origin.url https://github.com/o/repo.git && \
                 git config url.{}.insteadOf https://github.com/o/repo.git",
                upstream.display()
            ),
        );
        let tools = Tools {
            path_var: std::env::var("PATH").unwrap_or_default(),
            roots: vec![root.join("code")],
        };
        let dest = root.join("jobs/1/work");
        let folder = tools
            .prepare(Some("o/repo"), None, Some("feat/x"), &dest)
            .unwrap();
        assert_eq!(folder.remote, "origin");
        assert_eq!(folder.branch, "feat/x");
        assert!(dest.join("b").is_file(), "at the branch tip");
        assert_eq!(
            tools
                .prepare(Some("o/repo"), None, None, &root.join("jobs/2/work"))
                .unwrap()
                .branch,
            "main"
        );
        tools.remove(&folder).unwrap();
        assert!(!dest.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
