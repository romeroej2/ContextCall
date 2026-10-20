//! Persistent conversations over the local Claude Code CLI.
//!
//! One long-lived `claude -p --input-format stream-json` process per session:
//! the process stays warm between turns (first token in under a second when
//! thinking is off) and the CLI keeps the conversation history itself. Auth is
//! whatever the user's own `claude` login provides; ContextCall never reads or
//! stores those credentials.

use crate::intelligence::conversation::{ContentBlock, ConversationSession, TurnEvent, TurnUsage};
use crate::intelligence::llm_client::Message;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Debug, Clone)]
pub struct ClaudeCliConfig {
    pub command_path: String,
    /// Alias ("sonnet", "opus", "haiku") or a full model id.
    pub model: String,
    pub system_prompt: String,
    /// Empty folder the process runs in, so no project CLAUDE.md is picked up.
    pub working_dir: PathBuf,
    /// When false, thinking is disabled for the fastest first token.
    pub thinking: bool,
    /// Optional `--effort` level (low, medium, high, xhigh, max).
    pub effort: Option<String>,
    /// Allow the built-in WebSearch/WebFetch tools (slower, for research answers).
    pub web_search: bool,
}

pub fn resolve_command_path(command_path: &str) -> Result<String, String> {
    let trimmed = command_path.trim();
    let command = if trimmed.is_empty() {
        "claude"
    } else {
        trimmed
    };
    let path = Path::new(command);

    if path.components().count() > 1 || path.is_absolute() {
        return if path.exists() {
            Ok(path.to_string_lossy().into_owned())
        } else {
            Err(format!(
                "Claude CLI path does not exist: {}",
                path.display()
            ))
        };
    }

    let names: Vec<String> = if cfg!(windows) && path.extension().is_none() {
        vec![
            format!("{command}.exe"),
            format!("{command}.cmd"),
            command.to_string(),
        ]
    } else {
        vec![command.to_string()]
    };

    search_dirs()
        .iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
        .map(|found| found.to_string_lossy().into_owned())
        .ok_or_else(|| {
            format!(
                "Could not find `{command}`. Install Claude Code and sign in with `claude`, or set the full path to the claude binary in Settings."
            )
        })
}

/// PATH plus the install locations GUI apps usually miss (they start without the shell PATH).
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".claude").join("local"));
        if cfg!(windows) {
            dirs.push(home.join("AppData").join("Roaming").join("npm"));
        }
    }
    if !cfg!(windows) {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    let mut unique = Vec::new();
    for dir in dirs {
        if !unique.contains(&dir) {
            unique.push(dir);
        }
    }
    unique
}

pub fn command_args(config: &ClaudeCliConfig) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--setting-sources",
        "",
        "--strict-mcp-config",
        "--no-session-persistence",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    if config.web_search {
        args.extend(["--tools".into(), "WebSearch,WebFetch".into()]);
        args.extend(["--allowedTools".into(), "WebSearch,WebFetch".into()]);
    } else {
        args.extend(["--tools".into(), String::new()]);
    }

    let model = config.model.trim();
    if !model.is_empty() {
        args.extend(["--model".into(), model.to_string()]);
    }
    if let Some(effort) = config
        .effort
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty())
    {
        args.extend(["--effort".into(), effort.to_string()]);
    }
    args.extend(["--system-prompt".into(), config.system_prompt.clone()]);
    args
}

/// Encode one user turn as a stream-json input line.
pub fn user_turn_line(content: &[ContentBlock]) -> String {
    let blocks: Vec<Value> = content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
            ContentBlock::Document { media_type, data } => json!({
                "type": "document",
                "source": { "type": "base64", "media_type": media_type, "data": data },
            }),
        })
        .collect();
    json!({ "type": "user", "message": { "role": "user", "content": blocks } }).to_string()
}

fn interrupt_line(request_id: u64) -> String {
    json!({
        "type": "control_request",
        "request_id": format!("interrupt-{request_id}"),
        "request": { "subtype": "interrupt" },
    })
    .to_string()
}

/// What one stdout line means for the in-flight turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputLine {
    TextDelta(String),
    Result {
        ok: bool,
        text: String,
        usage: Option<TurnUsage>,
    },
    Other,
}

pub fn parse_output_line(line: &str) -> OutputLine {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return OutputLine::Other;
    };
    match value["type"].as_str() {
        Some("stream_event") => {
            let delta = &value["event"]["delta"];
            if delta["type"] == "text_delta" {
                if let Some(text) = delta["text"].as_str() {
                    return OutputLine::TextDelta(text.to_string());
                }
            }
            OutputLine::Other
        }
        Some("result") => {
            let ok = value["subtype"] == "success" && !value["is_error"].as_bool().unwrap_or(false);
            let text = value["result"].as_str().unwrap_or_default().to_string();
            let usage = value.get("usage").map(|u| TurnUsage {
                input_tokens: u["input_tokens"].as_u64(),
                cache_read_input_tokens: u["cache_read_input_tokens"].as_u64(),
                cache_creation_input_tokens: u["cache_creation_input_tokens"].as_u64(),
                output_tokens: u["output_tokens"].as_u64(),
            });
            OutputLine::Result { ok, text, usage }
        }
        _ => OutputLine::Other,
    }
}

struct ActiveTurn {
    events: mpsc::UnboundedSender<TurnEvent>,
    done: oneshot::Sender<()>,
    started: Instant,
    first_token_logged: bool,
    interrupted: bool,
}

pub struct ClaudeCliSession {
    model: String,
    stdin: Mutex<ChildStdin>,
    active: Arc<std::sync::Mutex<Option<ActiveTurn>>>,
    turn_lock: Mutex<()>,
    alive: Arc<AtomicBool>,
    next_request_id: AtomicU64,
    _child: Mutex<Child>,
}

impl ClaudeCliSession {
    pub async fn spawn(config: &ClaudeCliConfig) -> Result<Self, String> {
        let program = resolve_command_path(&config.command_path)?;
        tokio::fs::create_dir_all(&config.working_dir)
            .await
            .map_err(|e| format!("Failed to create Claude workspace: {e}"))?;

        let mut command = Command::new(&program);
        command
            .args(command_args(config))
            .current_dir(&config.working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if !config.thinking {
            command.env("MAX_THINKING_TOKENS", "0");
        }
        if let Some(path) = std::env::join_paths(search_dirs()).ok() {
            command.env("PATH", path);
        }

        log::info!(
            "Claude CLI spawn: program={program} model={} thinking={} effort={:?} webSearch={}",
            config.model,
            config.thinking,
            config.effort,
            config.web_search
        );
        let mut child = command
            .spawn()
            .map_err(|e| format!("Failed to start Claude CLI `{program}`: {e}"))?;
        let stdin = child.stdin.take().ok_or("Claude CLI stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("Claude CLI stdout unavailable")?;
        let stderr = child.stderr.take();

        let active: Arc<std::sync::Mutex<Option<ActiveTurn>>> =
            Arc::new(std::sync::Mutex::new(None));
        let alive = Arc::new(AtomicBool::new(true));

        tokio::spawn(read_stdout(stdout, Arc::clone(&active), Arc::clone(&alive)));
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    log::warn!(
                        "Claude CLI stderr: {}",
                        line.chars().take(500).collect::<String>()
                    );
                }
            });
        }

        Ok(Self {
            model: config.model.clone(),
            stdin: Mutex::new(stdin),
            active,
            turn_lock: Mutex::new(()),
            alive,
            next_request_id: AtomicU64::new(1),
            _child: Mutex::new(child),
        })
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    async fn write_line(&self, line: &str) -> Result<(), String> {
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|e| format!("Failed to write to Claude CLI: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to write to Claude CLI: {e}"))
    }
}

async fn read_stdout(
    stdout: tokio::process::ChildStdout,
    active: Arc<std::sync::Mutex<Option<ActiveTurn>>>,
    alive: Arc<AtomicBool>,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        match parse_output_line(&line) {
            OutputLine::TextDelta(text) => {
                if let Some(turn) = active.lock().unwrap().as_mut() {
                    if !turn.first_token_logged {
                        turn.first_token_logged = true;
                        log::info!(
                            "Claude CLI first token after {}ms",
                            turn.started.elapsed().as_millis()
                        );
                    }
                    let _ = turn.events.send(TurnEvent::TextDelta(text));
                }
            }
            OutputLine::Result { ok, text, usage } => {
                if let Some(turn) = active.lock().unwrap().take() {
                    log::info!(
                        "Claude CLI turn finished in {}ms ok={ok} interrupted={} usage={usage:?}",
                        turn.started.elapsed().as_millis(),
                        turn.interrupted
                    );
                    let event = if turn.interrupted {
                        TurnEvent::Error("interrupted".into())
                    } else if ok {
                        TurnEvent::Done { text, usage }
                    } else {
                        TurnEvent::Error(if text.trim().is_empty() {
                            "Claude CLI turn failed".into()
                        } else {
                            text
                        })
                    };
                    let _ = turn.events.send(event);
                    let _ = turn.done.send(());
                }
            }
            OutputLine::Other => {}
        }
    }

    alive.store(false, Ordering::Relaxed);
    log::warn!("Claude CLI process exited");
    if let Some(turn) = active.lock().unwrap().take() {
        let _ = turn.events.send(TurnEvent::Error(
            "Claude CLI exited. Check that `claude` is installed and signed in.".into(),
        ));
        let _ = turn.done.send(());
    }
}

#[async_trait]
impl ConversationSession for ClaudeCliSession {
    async fn send_turn(
        &self,
        content: Vec<ContentBlock>,
        events: mpsc::UnboundedSender<TurnEvent>,
    ) -> Result<(), String> {
        let _turn = self.turn_lock.lock().await;
        if !self.is_alive() {
            return Err("Claude CLI process is not running".into());
        }

        let (done_tx, done_rx) = oneshot::channel();
        *self.active.lock().unwrap() = Some(ActiveTurn {
            events,
            done: done_tx,
            started: Instant::now(),
            first_token_logged: false,
            interrupted: false,
        });

        if let Err(err) = self.write_line(&user_turn_line(&content)).await {
            self.active.lock().unwrap().take();
            return Err(err);
        }
        let _ = done_rx.await;
        Ok(())
    }

    async fn interrupt(&self) {
        let has_turn = {
            let mut active = self.active.lock().unwrap();
            match active.as_mut() {
                Some(turn) => {
                    turn.interrupted = true;
                    true
                }
                None => false,
            }
        };
        if has_turn {
            let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
            if let Err(err) = self.write_line(&interrupt_line(id)).await {
                log::warn!("Claude CLI interrupt failed: {err}");
            }
        }
    }

    fn model(&self) -> &str {
        &self.model
    }
}

/// Render role-tagged messages as one user turn (the CLI owns the real system prompt).
pub fn messages_as_turn(messages: &[Message]) -> String {
    let mut prompt = String::new();
    for message in messages {
        prompt.push_str(&format!(
            "<message role=\"{}\">\n{}\n</message>\n\n",
            message.role, message.content
        ));
    }
    prompt
}

/// Send one turn and wait for the full answer.
pub async fn complete_turn(
    session: &dyn ConversationSession,
    content: Vec<ContentBlock>,
    timeout: Duration,
) -> Result<String, String> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let send = session.send_turn(content, tx);
    let collect = async {
        let mut streamed = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                TurnEvent::TextDelta(text) => streamed.push_str(&text),
                TurnEvent::Done { text, .. } => {
                    return Ok(if text.trim().is_empty() {
                        streamed
                    } else {
                        text
                    });
                }
                TurnEvent::Error(err) => return Err(err),
            }
        }
        Err("Claude CLI returned no answer".to_string())
    };
    match tokio::time::timeout(timeout, async {
        let (sent, answer) = tokio::join!(send, collect);
        sent?;
        answer
    })
    .await
    {
        Ok(result) => result,
        Err(_) => {
            session.interrupt().await;
            Err("Claude CLI timed out".into())
        }
    }
}

/// One-shot completion in a throwaway process (notes and other non-live calls).
pub async fn complete(
    config: &ClaudeCliConfig,
    messages: &[Message],
    timeout: Duration,
) -> Result<String, String> {
    let session = ClaudeCliSession::spawn(config).await?;
    let text = complete_turn(
        &session,
        vec![ContentBlock::text(messages_as_turn(messages))],
        timeout,
    )
    .await?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Err("Claude CLI returned an empty response".into())
    } else {
        Ok(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ClaudeCliConfig {
        ClaudeCliConfig {
            command_path: "claude".into(),
            model: "sonnet".into(),
            system_prompt: "Be brief.".into(),
            working_dir: PathBuf::from("/tmp/contextcall-claude"),
            thinking: false,
            effort: None,
            web_search: false,
        }
    }

    #[test]
    fn args_stream_json_without_tools_or_user_settings() {
        let args = command_args(&config());
        assert!(args
            .windows(2)
            .any(|w| w == ["--input-format", "stream-json"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["--output-format", "stream-json"]));
        assert!(args.contains(&"--include-partial-messages".into()));
        assert!(args.windows(2).any(|w| w == ["--tools", ""]));
        assert!(args.windows(2).any(|w| w == ["--setting-sources", ""]));
        assert!(args.windows(2).any(|w| w == ["--model", "sonnet"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["--system-prompt", "Be brief."]));
        assert!(!args.contains(&"--effort".into()));
    }

    #[test]
    fn args_enable_web_tools_and_effort() {
        let mut cfg = config();
        cfg.web_search = true;
        cfg.effort = Some("low".into());
        let args = command_args(&cfg);
        assert!(args
            .windows(2)
            .any(|w| w == ["--tools", "WebSearch,WebFetch"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["--allowedTools", "WebSearch,WebFetch"]));
        assert!(args.windows(2).any(|w| w == ["--effort", "low"]));
    }

    #[test]
    fn user_turn_encodes_text_and_documents() {
        let line = user_turn_line(&[
            ContentBlock::Document {
                media_type: "application/pdf".into(),
                data: "QUJD".into(),
            },
            ContentBlock::text("Hi"),
        ]);
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(
            v["message"]["content"][0]["source"]["media_type"],
            "application/pdf"
        );
        assert_eq!(v["message"]["content"][1]["text"], "Hi");
    }

    #[test]
    fn parses_text_delta_and_results() {
        let delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}}"#;
        assert_eq!(
            parse_output_line(delta),
            OutputLine::TextDelta("Hel".into())
        );

        let ok = r#"{"type":"result","subtype":"success","is_error":false,"result":"Hello","usage":{"input_tokens":3,"cache_read_input_tokens":10,"output_tokens":2}}"#;
        match parse_output_line(ok) {
            OutputLine::Result { ok, text, usage } => {
                assert!(ok);
                assert_eq!(text, "Hello");
                assert_eq!(usage.unwrap().cache_read_input_tokens, Some(10));
            }
            other => panic!("unexpected {other:?}"),
        }

        let interrupted = r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#;
        assert!(matches!(
            parse_output_line(interrupted),
            OutputLine::Result { ok: false, .. }
        ));
        assert_eq!(
            parse_output_line(r#"{"type":"system","subtype":"init"}"#),
            OutputLine::Other
        );
        assert_eq!(parse_output_line("not json"), OutputLine::Other);
    }

    #[test]
    fn missing_explicit_path_errors() {
        assert!(resolve_command_path("/definitely/missing/claude")
            .unwrap_err()
            .contains("does not exist"));
    }

    /// Live round trip against the real CLI: `cargo test -p contextcall-core claude_cli_live -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn claude_cli_live_keeps_context() {
        let mut cfg = config();
        cfg.working_dir = std::env::temp_dir().join("contextcall-claude-test");
        let session = ClaudeCliSession::spawn(&cfg).await.unwrap();
        let first = complete_turn(
            &session,
            vec![ContentBlock::text(
                "Remember the word: lighthouse. Reply OK.",
            )],
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        assert!(!first.is_empty());
        let second = complete_turn(
            &session,
            vec![ContentBlock::text(
                "Which word did I ask you to remember? One word.",
            )],
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        assert!(second.to_lowercase().contains("lighthouse"));
    }
}
