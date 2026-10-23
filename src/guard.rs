//! The gate between an autopilot run and the outside world. Claude Code runs
//! the binary as its `PreToolUse` hook (`meow --guard`) before every tool
//! call: reading, searching and changing files in the run's own folder go
//! through; anything that cannot be taken back (a message sent, a Linear
//! issue changed, a push…) is refused and recorded, exactly as Claude asked
//! for it, so the user can approve it later.
//!
//! An approved call is replayed by resuming the same session: the guard then
//! lets through that one call, with that exact input, once.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The argument that makes the binary run as the hook instead of the app.
pub const GUARD_FLAG: &str = "--guard";
/// The file held calls are appended to, one JSON object per line.
pub const HELD_VAR: &str = "MEOW_GUARD_HELD";
/// The file holding the one call the user approved; removed once it ran.
pub const APPROVED_VAR: &str = "MEOW_GUARD_APPROVED";

/// What Claude is told when a call is held.
pub const HELD_REASON: &str = "Held for the user's approval: this action cannot be undone, so \
Meow recorded this exact call and will make it only once the user approves it. Do not retry it, \
and do not try to reach the same result another way (another tool, a script, the API…). Carry \
on with the rest of the work, and mention it in your final summary.";

/// A tool call, as the hook receives it and as it is recorded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub tool: String,
    pub input: Value,
}

impl Call {
    /// Whether `other` is this very call: same tool, same input. A shell
    /// command is compared by its command only (its description may differ).
    pub fn same_as(&self, other: &Call) -> bool {
        if self.tool != other.tool {
            return false;
        }
        if self.tool == "Bash" {
            return self.input.get("command") == other.input.get("command");
        }
        self.input == other.input
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Hold,
}

/// Built-in tools that only read, plan or delegate (sub-agents run through
/// the same hook).
const SAFE_TOOLS: &[&str] = &[
    "Read",
    "Glob",
    "Grep",
    "LS",
    "WebFetch",
    "WebSearch",
    "TodoWrite",
    "TodoRead",
    "Task",
    "Agent",
    "ToolSearch",
    "NotebookRead",
    "BashOutput",
    "KillShell",
    "KillBash",
    "TaskOutput",
    "TaskStop",
    "Monitor",
    "LSP",
    "Skill",
    "ExitPlanMode",
];
/// Built-in tools that write files: allowed inside the run's folder.
const FILE_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];
/// MCP tool names starting with one of these only read.
const READ_VERBS: &[&str] = &[
    "get", "list", "search", "read", "query", "fetch", "find", "describe", "view", "lookup",
    "whoami", "orient", "facets", "kpi", "document", "check", "show", "count", "extract",
];

/// What to do with `call`, made from `cwd`.
pub fn decide(call: &Call, cwd: &Path) -> Decision {
    let tool = call.tool.as_str();
    if SAFE_TOOLS.contains(&tool) {
        return Decision::Allow;
    }
    if FILE_TOOLS.contains(&tool) {
        let path = ["file_path", "notebook_path"]
            .iter()
            .find_map(|key| call.input.get(*key).and_then(Value::as_str));
        return match path {
            Some(path) if is_inside(Path::new(path), cwd) => Decision::Allow,
            _ => Decision::Hold,
        };
    }
    if tool == "Bash" {
        let command = call.input.get("command").and_then(Value::as_str);
        return match command {
            Some(command) if !bash_is_irreversible(command, cwd) => Decision::Allow,
            _ => Decision::Hold,
        };
    }
    if let Some(rest) = tool.strip_prefix("mcp__") {
        let Some((server, name)) = rest.split_once("__") else {
            return Decision::Hold;
        };
        // Meow's own tools only read.
        if server == crate::context::SERVER_NAME || mcp_only_reads(name) {
            return Decision::Allow;
        }
    }
    Decision::Hold
}

/// Whether the MCP tool `name` (without its server) only reads, from its verb.
fn mcp_only_reads(name: &str) -> bool {
    let name = name.to_lowercase();
    // `slack_read_channel`, `notion-fetch`: the verb may follow the app's name.
    let words: Vec<&str> = name.split(['_', '-']).filter(|w| !w.is_empty()).collect();
    let verb = match words.as_slice() {
        [first, second, ..] if !READ_VERBS.contains(first) => *second,
        [first, ..] => *first,
        [] => return false,
    };
    READ_VERBS.contains(&verb)
}

/// Whether `path` (absolute, or relative to `cwd`) stays inside `cwd`.
fn is_inside(path: &Path, cwd: &Path) -> bool {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut normal = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                if !normal.pop() {
                    return false;
                }
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    normal.starts_with(cwd)
}

/// Whether a shell command does something that cannot be taken back: talks
/// to a remote (push, PR comment, POST…), deploys or publishes, or deletes
/// files outside the run's folder. Unsure means yes.
pub fn bash_is_irreversible(command: &str, cwd: &Path) -> bool {
    segments(command)
        .iter()
        .any(|words| segment_is_irreversible(words, cwd))
}

/// The simple commands of a command line, each as its words, with variable
/// assignments and wrappers (`sudo`, `env`…) dropped.
fn segments(command: &str) -> Vec<Vec<String>> {
    let mut segments = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    let end_word = |word: &mut String, in_word: &mut bool, words: &mut Vec<String>| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some('"') if c == '\\' => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            Some(_) => word.push(c),
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    in_word = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        word.push(next);
                        in_word = true;
                    }
                }
                ' ' | '\t' => end_word(&mut word, &mut in_word, &mut words),
                // `$(…)` and backquotes start a command of their own.
                ';' | '&' | '|' | '\n' | '(' | ')' | '`' | '{' | '}' => {
                    end_word(&mut word, &mut in_word, &mut words);
                    segments.push(std::mem::take(&mut words));
                }
                '$' if chars.peek() == Some(&'(') => {
                    end_word(&mut word, &mut in_word, &mut words);
                    segments.push(std::mem::take(&mut words));
                }
                _ => {
                    word.push(c);
                    in_word = true;
                }
            },
        }
    }
    end_word(&mut word, &mut in_word, &mut words);
    segments.push(words);
    segments
        .into_iter()
        .map(strip_wrappers)
        .filter(|words| !words.is_empty())
        .collect()
}

fn strip_wrappers(words: Vec<String>) -> Vec<String> {
    let mut rest = words.as_slice();
    loop {
        match rest.first().map(String::as_str) {
            Some(word) if is_assignment(word) => rest = &rest[1..],
            Some(
                "sudo" | "env" | "command" | "exec" | "nohup" | "time" | "xargs" | "then" | "do"
                | "else" | "if" | "while" | "!",
            ) => rest = &rest[1..],
            _ => break,
        }
    }
    rest.to_vec()
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Commands that always reach out or publish.
const REMOTE_COMMANDS: &[&str] = &[
    "ssh",
    "scp",
    "sftp",
    "rsync",
    "osascript",
    "sendmail",
    "mail",
    "telnet",
    "nc",
    "ftp",
];
/// Words that make a cloud or service CLI change something.
const CLOUD_CHANGES: &[&str] = &[
    "deploy", "delete", "create", "update", "apply", "publish", "send", "post", "set", "remove",
    "destroy", "push", "upload", "release", "submit", "promote", "rollback", "scale", "add",
];
/// Read-only subcommands of the tools that otherwise change things remotely.
const GH_READS: &[&str] = &[
    "view", "list", "status", "diff", "checks", "checkout", "search", "browse", "watch",
    "download", "clone",
];

fn segment_is_irreversible(words: &[String], cwd: &Path) -> bool {
    let program = Path::new(&words[0])
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&words[0]);
    let args = &words[1..];
    let has = |flag: &str| {
        args.iter()
            .any(|a| a == flag || a.starts_with(&format!("{flag}=")))
    };
    match program {
        "git" => {
            let sub = git_subcommand(args);
            matches!(sub, Some("push" | "send-email" | "request-pull"))
                || (sub == Some("remote") && args.iter().any(|a| a == "prune"))
        }
        "gh" => gh_is_irreversible(args),
        "curl" => {
            let method = flag_value(args, &["-X", "--request"]);
            method
                .is_some_and(|m| !m.eq_ignore_ascii_case("GET") && !m.eq_ignore_ascii_case("HEAD"))
                || args.iter().any(|a| {
                    a.starts_with("-d")
                        || a.starts_with("--data")
                        || a.starts_with("-F")
                        || a.starts_with("--form")
                        || a == "-T"
                        || a.starts_with("--upload-file")
                        || a == "--json"
                })
        }
        "wget" => has("--post-data") || has("--post-file") || has("--method"),
        "npm" | "pnpm" | "yarn" | "cargo" | "gem" | "twine" | "poetry" | "flit" => {
            args.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "publish" | "unpublish" | "yank" | "upload" | "deprecate"
                )
            })
        }
        "pod" => args.iter().any(|a| a == "trunk"),
        "docker" | "podman" => args.first().is_some_and(|a| a == "push"),
        "kubectl" | "helm" => args.first().is_some_and(|a| {
            !matches!(
                a.as_str(),
                "get"
                    | "describe"
                    | "logs"
                    | "explain"
                    | "version"
                    | "list"
                    | "status"
                    | "show"
                    | "template"
                    | "lint"
                    | "diff"
                    | "top"
            )
        }),
        "terraform" | "tofu" | "pulumi" => args
            .first()
            .is_some_and(|a| matches!(a.as_str(), "apply" | "destroy" | "import" | "up" | "state")),
        "gcloud" | "aws" | "az" | "firebase" | "vercel" | "netlify" | "fly" | "flyctl"
        | "heroku" | "wrangler" | "fastlane" | "supabase" | "linear" | "slack" => {
            args.iter().any(|a| CLOUD_CHANGES.contains(&a.as_str()))
        }
        "rm" | "rmdir" | "shred" | "unlink" => args
            .iter()
            .filter(|a| !a.starts_with('-'))
            .any(|a| !is_inside(&expand_home(a), cwd)),
        "mv" => args
            .iter()
            .filter(|a| !a.starts_with('-'))
            .any(|a| !is_inside(&expand_home(a), cwd)),
        _ => REMOTE_COMMANDS.contains(&program),
    }
}

/// `git -C dir --no-pager push` → `push`.
fn git_subcommand(args: &[String]) -> Option<&str> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" => {
                iter.next();
            }
            a if a.starts_with('-') => {}
            a => return Some(a),
        }
    }
    None
}

fn gh_is_irreversible(args: &[String]) -> bool {
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with('-'))
        .collect();
    match words.as_slice() {
        ["api", ..] => {
            let method = flag_value(args, &["-X", "--method"]);
            method.is_some_and(|m| !m.eq_ignore_ascii_case("GET"))
                || args.iter().any(|a| {
                    matches!(
                        a.as_str(),
                        "-f" | "-F" | "--field" | "--raw-field" | "--input"
                    ) || a.starts_with("--field=")
                        || a.starts_with("--raw-field=")
                        || a.starts_with("--input=")
                })
        }
        ["auth", "status", ..] | ["help", ..] | ["version", ..] | [] => false,
        ["search", ..] | ["browse", ..] | ["status", ..] => false,
        [_, sub, ..] => !GH_READS.contains(sub),
        [_] => false,
    }
}

/// The value of the first of `flags` in `args` (`-X POST`, `-XPOST`, `--request=POST`).
fn flag_value<'a>(args: &'a [String], flags: &[&str]) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        for flag in flags {
            if arg == flag {
                return iter.next().map(String::as_str);
            }
            if let Some(value) = arg.strip_prefix(&format!("{flag}=")) {
                return Some(value);
            }
            if flag.len() == 2
                && let Some(value) = arg.strip_prefix(flag)
                && !value.is_empty()
            {
                return Some(value);
            }
        }
    }
    None
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix('~') {
        Some(rest) => {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(format!("{home}{rest}"))
        }
        None => PathBuf::from(path),
    }
}

/// The hook's answer refusing a call.
fn deny_output(reason: &str) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

/// Runs as Claude Code's `PreToolUse` hook: reads the call on stdin, prints a
/// refusal for a held one (recording it), nothing for the rest.
pub fn serve() {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return deny(HELD_REASON);
    }
    let Ok(hook) = serde_json::from_str::<Value>(&raw) else {
        return deny(HELD_REASON);
    };
    let call = Call {
        tool: hook["tool_name"].as_str().unwrap_or_default().to_string(),
        input: hook["tool_input"].clone(),
    };
    let cwd = hook["cwd"]
        .as_str()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    if decide(&call, &cwd) == Decision::Allow {
        return;
    }
    if let Some(path) = std::env::var_os(APPROVED_VAR).map(PathBuf::from)
        && take_approved(&path, &call)
    {
        return;
    }
    if let Some(path) = std::env::var_os(HELD_VAR) {
        let _ = record(Path::new(&path), &call);
    }
    deny(HELD_REASON);
}

/// Whether `call` is the approved one in `path`; it is then used up.
fn take_approved(path: &Path, call: &Call) -> bool {
    let Some(approved) = std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Call>(&raw).ok())
    else {
        return false;
    };
    approved.same_as(call) && std::fs::remove_file(path).is_ok()
}

/// Appends `call` to the held calls in `path`.
pub fn record(path: &Path, call: &Call) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", serde_json::to_string(call)?)
}

/// The calls held in `path`, each once, in the order first held.
pub fn read_held(path: &Path) -> Vec<Call> {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    let mut calls: Vec<Call> = Vec::new();
    for line in raw.lines() {
        if let Ok(call) = serde_json::from_str::<Call>(line)
            && !calls.iter().any(|c| c.same_as(&call))
        {
            calls.push(call);
        }
    }
    calls
}

fn deny(reason: &str) {
    print!("{}", deny_output(reason));
}

/// Claude Code's `--settings` value running `exe` as the hook on every call.
pub fn settings(exe: &Path) -> String {
    let command = format!(
        "'{}' {GUARD_FLAG}",
        exe.display().to_string().replace('\'', r"'\''")
    );
    json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": "*",
                "hooks": [{ "type": "command", "command": command }]
            }]
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cwd() -> PathBuf {
        PathBuf::from("/work/repo")
    }

    fn bash(command: &str) -> Decision {
        decide(
            &Call {
                tool: "Bash".into(),
                input: json!({ "command": command }),
            },
            &cwd(),
        )
    }

    fn tool(name: &str) -> Decision {
        decide(
            &Call {
                tool: name.into(),
                input: json!({}),
            },
            &cwd(),
        )
    }

    #[test]
    fn reading_and_local_git_go_through() {
        for command in [
            "git status && git diff origin/main...HEAD",
            "git fetch origin && git merge origin/main",
            "git add -A && git commit -m 'Merge main; fix conflicts'",
            "cargo test 2>&1 | tail -20",
            "gh pr view 12 --json headRefName,mergeable",
            "gh pr checks 12",
            "gh api repos/o/r/pulls/12",
            "curl -s https://api.github.com/repos/o/r",
            "rm -rf target/debug",
            "grep -rn 'push' src",
            "echo \"git push\"",
        ] {
            assert_eq!(bash(command), Decision::Allow, "{command}");
        }
    }

    #[test]
    fn remote_changes_are_held() {
        for command in [
            "git push",
            "git -C /work/repo push origin HEAD:feat/x",
            "cargo test && git push --force-with-lease",
            "FOO=1 git push",
            "echo $(git push)",
            "gh pr comment 12 --body done",
            "gh pr merge 12 --squash",
            "gh pr create --fill",
            "gh issue close 3",
            "gh api -X POST repos/o/r/issues/1/comments -f body=hi",
            "gh api repos/o/r/issues/1/comments -f body=hi",
            "curl -X POST https://hooks.slack.com/x -d '{}'",
            "curl --json '{}' https://example.com",
            "npm publish",
            "cargo publish",
            "ssh host deploy.sh",
            "osascript -e 'tell application \"Messages\" to send'",
            "rm -rf ~/Documents",
            "rm ../other/file",
            "gcloud run deploy api",
            "kubectl apply -f x.yaml",
        ] {
            assert_eq!(bash(command), Decision::Hold, "{command}");
        }
    }

    #[test]
    fn files_change_only_inside_the_folder() {
        let edit = |path: &str| {
            decide(
                &Call {
                    tool: "Edit".into(),
                    input: json!({ "file_path": path }),
                },
                &cwd(),
            )
        };
        assert_eq!(edit("/work/repo/src/main.rs"), Decision::Allow);
        assert_eq!(edit("src/main.rs"), Decision::Allow);
        assert_eq!(edit("/work/repo/../other/x"), Decision::Hold);
        assert_eq!(edit("/Users/me/.zshrc"), Decision::Hold);
    }

    #[test]
    fn connectors_may_only_read() {
        for name in [
            "mcp__claude_ai_Slack__slack_read_channel",
            "mcp__claude_ai_Slack__slack_search_public",
            "mcp__claude_ai_Linear__get_issue",
            "mcp__claude_ai_Linear__list_comments",
            "mcp__plugin_delivery_notion__notion-fetch",
            "mcp__claude_ai_Gmail__search_threads",
            "mcp__meow__slack_history",
            "Read",
            "Grep",
            "Task",
        ] {
            assert_eq!(tool(name), Decision::Allow, "{name}");
        }
        for name in [
            "mcp__claude_ai_Slack__slack_send_message",
            "mcp__claude_ai_Slack__slack_schedule_message",
            "mcp__claude_ai_Linear__save_issue",
            "mcp__claude_ai_Linear__save_comment",
            "mcp__claude_ai_Gmail__send_message",
            "mcp__claude_ai_Gmail__reply",
            "mcp__claude_ai_Google_Calendar__respond_to_event",
            "mcp__plugin_delivery_notion__notion-update-page",
            "mcp__claude-in-chrome__computer",
            "mcp__weird",
            "Artifact",
            "PushNotification",
        ] {
            assert_eq!(tool(name), Decision::Hold, "{name}");
        }
    }

    #[test]
    fn calls_match_by_tool_and_input() {
        let push = Call {
            tool: "Bash".into(),
            input: json!({ "command": "git push", "description": "Push" }),
        };
        let other_description = Call {
            tool: "Bash".into(),
            input: json!({ "command": "git push", "description": "Push it" }),
        };
        let force = Call {
            tool: "Bash".into(),
            input: json!({ "command": "git push -f" }),
        };
        assert!(push.same_as(&other_description));
        assert!(!push.same_as(&force));
        let send = |text: &str| Call {
            tool: "mcp__claude_ai_Slack__slack_send_message".into(),
            input: json!({ "channel_id": "D1", "message": text }),
        };
        assert!(send("hi").same_as(&send("hi")));
        assert!(!send("hi").same_as(&send("hi!")));
    }

    #[test]
    fn held_calls_are_recorded_once_and_approvals_used_up() {
        let dir = std::env::temp_dir().join(format!("meow-guard-{}", std::process::id()));
        let held = dir.join("held.jsonl");
        let push = Call {
            tool: "Bash".into(),
            input: json!({ "command": "git push" }),
        };
        record(&held, &push).unwrap();
        record(&held, &push).unwrap();
        assert_eq!(read_held(&held), vec![push.clone()]);

        let approved = dir.join("approved.json");
        std::fs::write(&approved, serde_json::to_string(&push).unwrap()).unwrap();
        let force = Call {
            tool: "Bash".into(),
            input: json!({ "command": "git push -f" }),
        };
        assert!(!take_approved(&approved, &force));
        assert!(take_approved(&approved, &push));
        assert!(!take_approved(&approved, &push), "only once");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_hook_is_quoted() {
        let settings = settings(Path::new("/Applications/My Meow.app/Contents/MacOS/meow"));
        let value: Value = serde_json::from_str(&settings).unwrap();
        assert_eq!(
            value["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "'/Applications/My Meow.app/Contents/MacOS/meow' --guard"
        );
    }
}
