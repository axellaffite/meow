//! Runs requests through the `claude` CLI in print mode, so they use the
//! regular Claude subscription rather than API billing.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{context, guard};

const PERMISSION_MODE: &str = "auto";
/// Haiku (always its latest version): cheap and quick, and enough for fetching
/// from connectors and summarizing. `MEOW_MODEL` picks another (e.g. `sonnet`).
pub const DEFAULT_MODEL: &str = "haiku";
/// For the work where quality matters more than speed: researching a reply,
/// learning how the user writes.
pub const STRONG_MODEL: &str = "opus";
/// Plan mode: Claude may read, search and query connectors, but its tools may
/// not change anything (no edits, no commands with effects, no write tools).
const READ_ONLY_MODE: &str = "plan";
/// Refuses every tool not explicitly allowed, without asking.
const ONLY_ALLOWED_MODE: &str = "dontAsk";
const TOOL_SUMMARY_LEN: usize = 80;
const STDERR_TAIL_LEN: usize = 400;
const STDERR_KEEP_BYTES: usize = 4096;
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaudeEvent {
    Session(String),
    /// The tools the session offers (built-in and MCP), from its start.
    Tools(Vec<String>),
    Text(String),
    Tool(String),
    /// A sub-agent started; `id` is the tool call that launched it.
    AgentStarted {
        id: String,
        description: String,
    },
    AgentFinished {
        id: String,
    },
    Done {
        is_error: bool,
        result: String,
    },
    Failed(String),
}

/// Turns one line of `--output-format stream-json` into UI events.
pub fn parse_line(line: &str) -> Vec<ClaudeEvent> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let str_field = |key: &str| value.get(key).and_then(Value::as_str);

    match str_field("type") {
        Some("system") => parse_system(&value),
        // Sub-agents' own steps carry their parent's id; only their outcome matters here.
        Some("assistant") if str_field("parent_tool_use_id").is_some() => Vec::new(),
        Some("assistant") => value
            .pointer("/message/content")
            .and_then(Value::as_array)
            .map(|blocks| blocks.iter().filter_map(parse_block).collect())
            .unwrap_or_default(),
        Some("result") => vec![ClaudeEvent::Done {
            is_error: value
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            result: str_field("result").unwrap_or_default().to_string(),
        }],
        _ => Vec::new(),
    }
}

fn parse_system(value: &Value) -> Vec<ClaudeEvent> {
    let field = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let event = match field("subtype").as_deref() {
        Some("init") => {
            let tools: Vec<String> = value
                .get("tools")
                .and_then(Value::as_array)
                .map(|tools| {
                    tools
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let session = field("session_id").map(ClaudeEvent::Session);
            let tools = (!tools.is_empty()).then_some(ClaudeEvent::Tools(tools));
            return session.into_iter().chain(tools).collect();
        }
        Some("task_started") => field("tool_use_id").map(|id| ClaudeEvent::AgentStarted {
            id,
            description: field("description").unwrap_or_default(),
        }),
        Some("task_notification") => {
            field("tool_use_id").map(|id| ClaudeEvent::AgentFinished { id })
        }
        _ => None,
    };
    event.into_iter().collect()
}

fn parse_block(block: &Value) -> Option<ClaudeEvent> {
    match block.get("type")?.as_str()? {
        "text" => {
            let text = block.get("text")?.as_str()?.trim();
            (!text.is_empty()).then(|| ClaudeEvent::Text(text.to_string()))
        }
        "tool_use" => {
            let name = block.get("name")?.as_str()?;
            Some(ClaudeEvent::Tool(describe_tool(name, block.get("input"))))
        }
        _ => None,
    }
}

fn describe_tool(name: &str, input: Option<&Value>) -> String {
    let detail = input.and_then(|input| {
        [
            "description",
            "command",
            "file_path",
            "pattern",
            "url",
            "query",
            "prompt",
        ]
        .iter()
        .find_map(|key| input.get(key).and_then(Value::as_str))
    });
    match detail {
        Some(detail) => format!("{name}: {}", truncate(detail.lines().next().unwrap_or(""))),
        None => name.to_string(),
    }
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= TOOL_SUMMARY_LEN {
        return text.to_string();
    }
    let cut: String = text.chars().take(TOOL_SUMMARY_LEN - 1).collect();
    format!("{cut}…")
}

/// Where and how to launch `claude`.
#[derive(Clone, Debug)]
pub struct ClaudeEnv {
    pub binary: PathBuf,
    pub path_var: String,
    pub cwd: PathBuf,
    /// Model for requests and for the sub-agents they launch.
    pub model: String,
    /// Meow's own binary, served to Claude as the `meow` MCP server; `None`
    /// leaves Claude without Meow's tools.
    pub meow_bin: Option<PathBuf>,
}

impl ClaudeEnv {
    /// Apps launched from Finder get a minimal PATH, so ask the login shell
    /// for the real one before looking for `claude`.
    pub fn discover() -> Result<Self, String> {
        let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
        let path_var = login_shell_path().unwrap_or_else(fallback_path);
        let binary = std::env::var_os("MEOW_CLAUDE_BIN")
            .map(PathBuf::from)
            .or_else(|| find_in_path("claude", &path_var))
            .ok_or_else(|| {
                "Could not find the `claude` command. Is Claude Code installed?".to_string()
            })?;
        let cwd = std::env::var_os("MEOW_CWD")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(home));
        let model = std::env::var("MEOW_MODEL")
            .ok()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        Ok(Self {
            binary,
            path_var,
            cwd,
            model,
            meow_bin: std::env::current_exe().ok(),
        })
    }
}

fn login_shell_path() -> Option<String> {
    const MARKER: &str = "__MEOW_PATH__";
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut child = Command::new(shell)
        .args(["-ilc", &format!("printf '{MARKER}%s{MARKER}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // A slow or interactive shell config must not block discovery forever.
    let deadline = Instant::now() + SHELL_TIMEOUT;
    while child.try_wait().ok()?.is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let mut stdout = String::new();
    child.stdout.take()?.read_to_string(&mut stdout).ok()?;
    let start = stdout.find(MARKER)? + MARKER.len();
    let end = start + stdout[start..].find(MARKER)?;
    Some(stdout[start..end].to_string())
}

/// PATH used when the login shell cannot tell us: the current one plus usual install spots.
fn fallback_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs = vec![std::env::var("PATH").unwrap_or_default()];
    dirs.extend(
        ["/.local/bin", "/.claude/local", "/.npm-global/bin"]
            .iter()
            .map(|d| format!("{home}{d}")),
    );
    dirs.extend(
        ["/opt/homebrew/bin", "/usr/local/bin"]
            .iter()
            .map(|d| d.to_string()),
    );
    dirs.join(":")
}

pub fn find_in_path(name: &str, path_var: &str) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// One in-flight `claude -p` call. Events arrive on `events`.
///
/// Dropping the run kills claude and everything it started, unless it already
/// delivered its result.
pub struct Run {
    pub events: Receiver<ClaudeEvent>,
    process_group: libc::pid_t,
    finished: Arc<AtomicBool>,
}

/// No built-in tools, and only MCP servers from `--mcp-config` (none given).
const OFFLINE_ARGS: &[&str] = &["--tools", "", "--strict-mcp-config"];

/// Set in Superset terminals; its hooks are skipped without it.
const SUPERSET_HOME_VAR: &str = "SUPERSET_HOME_DIR";

impl Run {
    pub fn start(env: &ClaudeEnv, prompt: &str, session: Option<&str>) -> Result<Self, String> {
        let args = app_context_args(env);
        Self::spawn(
            env,
            prompt,
            session,
            &env.model,
            PERMISSION_MODE,
            &as_strs(&args),
            &[],
        )
    }

    /// A one-off call with no tools and no MCP servers: Claude can only read
    /// the prompt and answer, never act (e.g. post to Slack on its own).
    pub fn start_offline(env: &ClaudeEnv, prompt: &str) -> Result<Self, String> {
        Self::start_offline_on(env, prompt, &env.model)
    }

    /// [`Run::start_offline`] on a chosen `model`.
    pub fn start_offline_on(env: &ClaudeEnv, prompt: &str, model: &str) -> Result<Self, String> {
        Self::spawn(env, prompt, None, model, PERMISSION_MODE, OFFLINE_ARGS, &[])
    }

    /// A one-off call with the user's tools, skills and connectors, read-only:
    /// Claude can look things up but not change or send anything.
    pub fn start_read_only(env: &ClaudeEnv, prompt: &str, model: &str) -> Result<Self, String> {
        let args = app_context_args(env);
        Self::spawn(
            env,
            prompt,
            None,
            model,
            READ_ONLY_MODE,
            &as_strs(&args),
            &[],
        )
    }

    /// [`Run::start`], read-only: a chat message that may look things up but
    /// not act (e.g. writing a coding agent's prompt).
    pub fn start_planning(
        env: &ClaudeEnv,
        prompt: &str,
        session: Option<&str>,
    ) -> Result<Self, String> {
        let args = app_context_args(env);
        Self::spawn(
            env,
            prompt,
            session,
            &env.model,
            READ_ONLY_MODE,
            &as_strs(&args),
            &[],
        )
    }

    /// A one-off call that may use only the `allowed` MCP tools: no built-in
    /// tools (no shell, no files), and every other tool is refused.
    pub fn start_restricted(
        env: &ClaudeEnv,
        prompt: &str,
        model: &str,
        allowed: &[String],
    ) -> Result<Self, String> {
        let allowed = allowed.join(",");
        let args = ["--tools", "", "--allowedTools", allowed.as_str()];
        Self::spawn(env, prompt, None, model, ONLY_ALLOWED_MODE, &args, &[])
    }

    /// A one-off call whose only tool is reading files under `dir` (e.g. to
    /// look at an image): no shell, no edits, no MCP servers.
    pub fn start_reading(
        env: &ClaudeEnv,
        prompt: &str,
        model: &str,
        dir: &Path,
    ) -> Result<Self, String> {
        let allowed = read_rule(dir);
        let args = [
            "--tools",
            "Read",
            "--allowedTools",
            allowed.as_str(),
            "--strict-mcp-config",
        ];
        Self::spawn(env, prompt, None, model, ONLY_ALLOWED_MODE, &args, &[])
    }

    /// An autopilot run in `cwd`: the chat's tools and connectors, with every
    /// call going through Meow's guard ([`crate::guard`]), which records the
    /// ones that cannot be undone in `held` instead of making them, and lets
    /// through the single call written in `approved`, if any.
    pub fn start_guarded(
        env: &ClaudeEnv,
        prompt: &str,
        session: Option<&str>,
        model: &str,
        cwd: &Path,
        held: &Path,
        approved: Option<&Path>,
    ) -> Result<Self, String> {
        let bin = env
            .meow_bin
            .as_deref()
            .ok_or("Meow could not find its own binary, which guards autopilot runs.")?;
        let mut args = app_context_args(env);
        args.extend(["--settings".to_string(), guard::settings(bin)]);
        let env = ClaudeEnv {
            cwd: cwd.to_path_buf(),
            ..env.clone()
        };
        let mut vars = vec![(guard::HELD_VAR, held.as_os_str())];
        vars.extend(approved.map(|path| (guard::APPROVED_VAR, path.as_os_str())));
        Self::spawn(
            &env,
            prompt,
            session,
            model,
            PERMISSION_MODE,
            &as_strs(&args),
            &vars,
        )
    }

    fn spawn(
        env: &ClaudeEnv,
        prompt: &str,
        session: Option<&str>,
        model: &str,
        permission_mode: &str,
        extra_args: &[&str],
        vars: &[(&str, &OsStr)],
    ) -> Result<Self, String> {
        let mut command = Command::new(&env.binary);
        command
            .args([
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--permission-mode",
                permission_mode,
                "--model",
                model,
            ])
            .current_dir(&env.cwd)
            .env("PATH", &env.path_var)
            // Sub-agents would otherwise run on Claude Code's default model.
            .env("CLAUDE_CODE_SUBAGENT_MODEL", model)
            // Superset's hooks (notifications…) only run when this is set: Meow's
            // requests are its own, so they stay out of them.
            .env_remove(SUPERSET_HOME_VAR)
            // Own process group, so cancelling also stops the tools claude spawned.
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(id) = session {
            command.args(["--resume", id]);
        }
        command.args(extra_args).envs(vars.iter().copied());

        let child = command
            .spawn()
            .map_err(|e| format!("Could not start claude: {e}"))?;
        let process_group = child.id() as libc::pid_t;
        let finished = Arc::new(AtomicBool::new(false));
        let (tx, events) = mpsc::channel();
        let worker = Worker {
            process_group,
            finished: Arc::clone(&finished),
            tx,
        };
        let prompt = prompt.to_string();
        thread::spawn(move || worker.drive(child, prompt));
        Ok(Self {
            events,
            process_group,
            finished,
        })
    }

    pub fn cancel(&self) {
        if !self.finished.load(Ordering::SeqCst) {
            kill_group(self.process_group);
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// What tells Claude it runs inside Meow: the description of Meow appended to
/// its system prompt, and Meow's read-only tools, alongside the user's own
/// MCP servers. Only the prompt when Meow's binary is unknown.
fn app_context_args(env: &ClaudeEnv) -> Vec<String> {
    let prompt = [
        "--append-system-prompt".to_string(),
        context::system_prompt(),
    ];
    let tools = env.meow_bin.as_deref().map(|bin| {
        [
            "--mcp-config".to_string(),
            context::mcp_config(bin),
            "--allowedTools".to_string(),
            context::allowed_tools(),
        ]
    });
    prompt
        .into_iter()
        .chain(tools.into_iter().flatten())
        .collect()
}

fn as_strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// The permission rule reading anything under `dir`: an absolute path is
/// written with a leading `//`.
fn read_rule(dir: &Path) -> String {
    format!("Read(/{}/**)", dir.display())
}

fn kill_group(process_group: libc::pid_t) {
    // SAFETY: killpg only sends a signal; a stale group id makes it fail harmlessly.
    unsafe { libc::killpg(process_group, libc::SIGKILL) };
}

/// Background side of a `Run`: feeds the prompt, streams events, reaps the process.
struct Worker {
    process_group: libc::pid_t,
    finished: Arc<AtomicBool>,
    tx: Sender<ClaudeEvent>,
}

impl Worker {
    fn drive(self, mut child: Child, prompt: String) {
        if let Some(mut stdin) = child.stdin.take() {
            // The prompt goes through stdin so text starting with `-` is never read as a flag.
            thread::spawn(move || {
                let _ = stdin.write_all(prompt.as_bytes());
            });
        }
        let stderr = child.stderr.take().map(spawn_tail_reader);
        let (got_result, receiver_gone) = match child.stdout.take() {
            Some(stdout) => self.forward_events(stdout),
            None => (false, false),
        };

        if !got_result {
            // Crashed, cancelled or stuck: make sure nothing it started keeps running.
            kill_group(self.process_group);
        }
        self.finished.store(true, Ordering::SeqCst);
        let status = child.wait();

        if got_result || receiver_gone {
            return;
        }
        let stderr = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
        let reason = match status {
            Ok(s) if s.success() => "claude exited without a result".to_string(),
            Ok(s) => format!("claude stopped ({s}). {}", tail(&stderr)),
            Err(e) => format!("claude stopped unexpectedly: {e}"),
        };
        let _ = self.tx.send(ClaudeEvent::Failed(reason.trim().to_string()));
    }

    /// Returns (result received, receiver dropped).
    fn forward_events(&self, stdout: impl Read) -> (bool, bool) {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => return (false, false),
                Ok(_) => {}
            }
            for event in parse_line(String::from_utf8_lossy(&line).trim_end()) {
                let is_result = matches!(event, ClaudeEvent::Done { .. });
                if is_result {
                    self.finished.store(true, Ordering::SeqCst);
                }
                if self.tx.send(event).is_err() {
                    return (false, true);
                }
                if is_result {
                    return (true, false);
                }
            }
        }
    }
}

/// Reads a stream to the end, keeping only its last bytes.
fn spawn_tail_reader(mut stream: impl Read + Send + 'static) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stream.read(&mut chunk) {
            if n == 0 {
                break;
            }
            kept.extend_from_slice(&chunk[..n]);
            let excess = kept.len().saturating_sub(STDERR_KEEP_BYTES);
            kept.drain(..excess);
        }
        String::from_utf8_lossy(&kept).into_owned()
    })
}

fn tail(text: &str) -> &str {
    let text = text.trim();
    let start = text.len().saturating_sub(STDERR_TAIL_LEN);
    let start = (start..=text.len())
        .find(|&i| text.is_char_boundary(i))
        .unwrap_or(text.len());
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_line_gives_session_id() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc","tools":[]}"#;
        assert_eq!(parse_line(line), vec![ClaudeEvent::Session("abc".into())]);
    }

    #[test]
    fn init_line_lists_the_tools_on_offer() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc",
            "tools":["mcp__cal__create_event","mcp__cal__list_events"]}"#;
        assert_eq!(
            parse_line(&line.replace('\n', "")),
            vec![
                ClaudeEvent::Session("abc".into()),
                ClaudeEvent::Tools(vec![
                    "mcp__cal__create_event".into(),
                    "mcp__cal__list_events".into()
                ]),
            ]
        );
    }

    #[test]
    fn hook_lines_are_ignored() {
        let line = r#"{"type":"system","subtype":"hook_started","session_id":"abc"}"#;
        assert!(parse_line(line).is_empty());
    }

    #[test]
    fn assistant_text_and_tool_use_are_extracted() {
        let line = r#"{"type":"assistant","message":{"content":[
            {"type":"text","text":"  On it.  "},
            {"type":"tool_use","name":"Bash","input":{"command":"echo hi","description":"Print hi"}},
            {"type":"thinking","thinking":"hmm"}
        ]}}"#;
        assert_eq!(
            parse_line(line),
            vec![
                ClaudeEvent::Text("On it.".into()),
                ClaudeEvent::Tool("Bash: Print hi".into())
            ]
        );
    }

    #[test]
    fn empty_text_blocks_are_dropped() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"  "}]}}"#;
        assert!(parse_line(line).is_empty());
    }

    #[test]
    fn tool_without_known_input_shows_name() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"TodoWrite","input":{}}]}}"#;
        assert_eq!(
            parse_line(line),
            vec![ClaudeEvent::Tool("TodoWrite".into())]
        );
    }

    #[test]
    fn long_tool_detail_is_truncated_to_first_line() {
        let long = "x".repeat(200);
        let input = serde_json::json!({ "command": format!("{long}\nsecond line") });
        let summary = describe_tool("Bash", Some(&input));
        assert!(summary.ends_with('…'));
        assert_eq!(summary.chars().count(), "Bash: ".len() + TOOL_SUMMARY_LEN);
    }

    #[test]
    fn result_line_reports_errors() {
        let line = r#"{"duration_ms":1,"type":"result","subtype":"error","is_error":true,"result":"boom"}"#;
        assert_eq!(
            parse_line(line),
            vec![ClaudeEvent::Done {
                is_error: true,
                result: "boom".into()
            }]
        );
    }

    #[test]
    fn subagent_messages_are_skipped() {
        let line = r#"{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"content":[
            {"type":"text","text":"inner"},{"type":"tool_use","name":"Bash","input":{}}]}}"#;
        assert!(parse_line(line).is_empty());
    }

    #[test]
    fn main_thread_messages_with_null_parent_are_kept() {
        let line = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"hi"}]}}"#;
        assert_eq!(parse_line(line), vec![ClaudeEvent::Text("hi".into())]);
    }

    #[test]
    fn subagent_lifecycle_is_reported() {
        let started = r#"{"type":"system","subtype":"task_started","task_id":"a1","tool_use_id":"toolu_1","description":"Check Linear"}"#;
        let finished = r#"{"type":"system","subtype":"task_notification","task_id":"a1","tool_use_id":"toolu_1"}"#;
        assert_eq!(
            parse_line(started),
            vec![ClaudeEvent::AgentStarted {
                id: "toolu_1".into(),
                description: "Check Linear".into()
            }]
        );
        assert_eq!(
            parse_line(finished),
            vec![ClaudeEvent::AgentFinished {
                id: "toolu_1".into()
            }]
        );
    }

    #[test]
    fn reading_is_allowed_only_under_the_folder() {
        assert_eq!(
            read_rule(Path::new("/tmp/meow/photo")),
            "Read(//tmp/meow/photo/**)"
        );
    }

    #[test]
    fn invalid_json_is_ignored() {
        assert!(parse_line("not json").is_empty());
    }

    #[test]
    fn tail_keeps_end_of_long_text_on_char_boundary() {
        let text = format!("{}é{}", "a".repeat(500), "b".repeat(10));
        let end = tail(&text);
        assert!(end.ends_with("bbbbbbbbbb"));
        assert!(end.len() <= STDERR_TAIL_LEN);
    }

    #[test]
    fn find_in_path_locates_executables() {
        let found = find_in_path("sh", "/nonexistent:/bin");
        assert_eq!(found, Some(PathBuf::from("/bin/sh")));
        assert_eq!(find_in_path("definitely-not-a-binary", "/bin"), None);
    }

    fn fake_claude(name: &str, body: &str) -> (ClaudeEnv, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("meow-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("claude");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = ClaudeEnv {
            binary: script,
            path_var: "/bin:/usr/bin".into(),
            cwd: dir.clone(),
            model: DEFAULT_MODEL.into(),
            meow_bin: None,
        };
        (env, dir)
    }

    #[test]
    fn run_streams_events_from_a_fake_claude() {
        let (env, dir) = fake_claude(
            "ok",
            "cat > /dev/null\n\
             echo '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s1\"}'\n\
             echo '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}'\n\
             echo '{\"type\":\"result\",\"is_error\":false,\"result\":\"hi\"}'",
        );
        let run = Run::start(&env, "hello", None).unwrap();
        let events: Vec<_> = run.events.iter().collect();
        assert_eq!(
            events,
            vec![
                ClaudeEvent::Session("s1".into()),
                ClaudeEvent::Text("hi".into()),
                ClaudeEvent::Done {
                    is_error: false,
                    result: "hi".into()
                },
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn run_passes_model_resume_flag_and_prompt() {
        // The arguments go to a file: the system prompt outgrows stderr's tail.
        let (env, dir) = fake_claude(
            "resume",
            "prompt=$(cat)\n\
             echo \"$@ | sub=$CLAUDE_CODE_SUBAGENT_MODEL | superset=${SUPERSET_HOME_DIR:-none} | $prompt\" > args.txt\n\
             exit 1",
        );
        let run = Run::start(&env, "do it", Some("s42")).unwrap();
        let events: Vec<_> = run.events.iter().collect();
        assert!(
            matches!(&events[..], [ClaudeEvent::Failed(_)]),
            "{events:?}"
        );
        let args = std::fs::read_to_string(dir.join("args.txt")).unwrap();
        assert!(args.contains("--model haiku --resume s42"), "{args}");
        assert!(args.contains("sub=haiku | superset=none | do it"), "{args}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn chat_runs_get_meow_context_and_offline_runs_do_not() {
        let script = "cat > /dev/null\nprintf '%s\\n' \"$@\" > args.txt\nexit 1";
        let (mut env, dir) = fake_claude("context", script);
        env.meow_bin = Some(PathBuf::from("/Apps/Meow/meow"));
        let args_of = |run: Run| {
            let events: Vec<_> = run.events.iter().collect();
            assert!(
                matches!(&events[..], [ClaudeEvent::Failed(_)]),
                "{events:?}"
            );
            std::fs::read_to_string(dir.join("args.txt")).unwrap()
        };
        let chat = args_of(Run::start(&env, "hi", None).unwrap());
        assert!(chat.contains("--append-system-prompt\nYou are the assistant built into Meow"));
        assert!(chat.contains("\"command\":\"/Apps/Meow/meow\""), "{chat}");
        assert!(chat.contains("--allowedTools\nmcp__meow\n"), "{chat}");
        let planning = args_of(Run::start_planning(&env, "hi", None).unwrap());
        assert!(planning.contains("--mcp-config"), "{planning}");
        let offline = args_of(Run::start_offline(&env, "hi").unwrap());
        assert!(!offline.contains("--mcp-config"), "{offline}");
        assert!(!offline.contains("--append-system-prompt"), "{offline}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn without_meow_binary_only_the_prompt_is_added() {
        let env = ClaudeEnv {
            binary: PathBuf::from("claude"),
            path_var: String::new(),
            cwd: PathBuf::from("/"),
            model: DEFAULT_MODEL.into(),
            meow_bin: None,
        };
        let args = app_context_args(&env);
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "--append-system-prompt");
    }

    #[test]
    fn invalid_utf8_line_does_not_stop_the_stream() {
        let (env, dir) = fake_claude(
            "utf8",
            "cat > /dev/null\nprintf '\\377\\376 junk\\n'\n\
             echo '{\"type\":\"result\",\"is_error\":false,\"result\":\"ok\"}'",
        );
        let run = Run::start(&env, "hello", None).unwrap();
        let events: Vec<_> = run.events.iter().collect();
        assert_eq!(
            events,
            vec![ClaudeEvent::Done {
                is_error: false,
                result: "ok".into()
            }]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancel_kills_claude_and_its_children() {
        let (env, dir) = fake_claude(
            "cancel",
            "cat > /dev/null\nsleep 30 &\necho $! > child.pid\nwait",
        );
        let run = Run::start(&env, "hello", None).unwrap();
        let pid_file = dir.join("child.pid");
        let deadline = Instant::now() + Duration::from_secs(5);
        while std::fs::read_to_string(&pid_file).map_or(true, |p| p.trim().is_empty()) {
            assert!(
                Instant::now() < deadline,
                "fake claude never started its child"
            );
            thread::sleep(Duration::from_millis(20));
        }
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();

        let started = Instant::now();
        run.cancel();
        let events: Vec<_> = run.events.iter().collect();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cancel did not stop the run"
        );
        assert!(
            matches!(&events[..], [ClaudeEvent::Failed(_)]),
            "{events:?}"
        );
        // SAFETY: signal 0 only checks whether the process exists.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(!alive, "child of claude survived the cancel");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
