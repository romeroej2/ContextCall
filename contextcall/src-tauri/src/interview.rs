//! Interview mode runtime: two warm Claude conversations per recording session.
//!
//! The fast tier drafts what the candidate can say (first token in ~1 s with
//! thinking off); the deep tier (Opus) streams the technical depth alongside.
//! Both keep the whole interview as context, so follow-ups make sense. A new
//! question interrupts whatever is still streaming.

use contextcall_core::intelligence::claude_client::{ClaudeCliConfig, ClaudeCliSession};
use contextcall_core::intelligence::conversation::{ContentBlock, ConversationSession, TurnEvent};
use contextcall_core::intelligence::interview_engine::{
    self, CopilotRole, GateOutput, InterviewProfile, InterviewTracker, NoneGate, QuestionSignal,
};
use contextcall_core::models::Utterance;
use contextcall_core::settings::AppSettings;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

pub const QUESTION_EVENT: &str = "interview-question";
pub const ANSWER_EVENT: &str = "interview-answer";
pub const STATUS_EVENT: &str = "interview-status";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Fast,
    Deep,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionPayload {
    pub id: String,
    pub question: String,
    pub manual: bool,
    pub asked_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerPayload {
    pub id: String,
    pub tier: Tier,
    /// "delta" | "done" | "dismissed" | "error"
    pub kind: &'static str,
    pub text: String,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    /// "starting" | "ready" | "error" | "stopped"
    pub state: &'static str,
    pub message: String,
    pub fast_model: String,
    pub deep_model: Option<String>,
}

struct Tiered {
    session: Arc<dyn ConversationSession>,
    /// Generation of the question turn this session is streaming (0 = none; the
    /// priming turn never counts, so it is never interrupted).
    answering: AtomicU64,
}

pub struct InterviewRuntime {
    app: AppHandle,
    fast: Tiered,
    deep: Option<Tiered>,
    tracker: Mutex<InterviewTracker>,
    generation: AtomicU64,
    auto_answer: bool,
}

fn claude_config(
    settings: &AppSettings,
    workspace: PathBuf,
    tier: Tier,
    language: &str,
) -> ClaudeCliConfig {
    let role = CopilotRole::from_setting(&settings.interview_role);
    let (model, system_prompt, thinking, web_search) = match tier {
        Tier::Fast => (
            settings.interview_fast_model.clone(),
            interview_engine::fast_system_prompt(language, role),
            false,
            false,
        ),
        Tier::Deep => (
            settings.interview_deep_model.clone(),
            interview_engine::deep_system_prompt(language, role),
            settings.interview_deep_thinking,
            settings.interview_deep_web_search,
        ),
    };
    ClaudeCliConfig {
        command_path: settings.claude_cli_path.clone(),
        model,
        system_prompt,
        working_dir: workspace,
        thinking,
        effort: None,
        web_search,
    }
}

async fn spawn_session(
    settings: &AppSettings,
    workspace: PathBuf,
    tier: Tier,
) -> Result<Arc<dyn ConversationSession>, String> {
    let language = settings.transcription_locale.clone();
    match settings.interview_backend.as_str() {
        // Further transports (e.g. the Anthropic API) plug in here behind ConversationSession.
        _ => {
            let config = claude_config(settings, workspace, tier, &language);
            let session = ClaudeCliSession::spawn(&config).await?;
            Ok(Arc::new(session) as Arc<dyn ConversationSession>)
        }
    }
}

pub fn load_profile(settings: &AppSettings) -> Result<InterviewProfile, String> {
    let resume = match settings
        .interview_resume_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        Some(path) => Some(interview_engine::load_resume(std::path::Path::new(path))?),
        None => None,
    };
    Ok(InterviewProfile {
        role: CopilotRole::from_setting(&settings.interview_role),
        resume,
        job_description: settings.interview_job_description.clone(),
        extra_context: settings.interview_extra_context.clone(),
        response_language: settings.transcription_locale.clone(),
    })
}

fn emit_status(
    app: &AppHandle,
    state: &'static str,
    message: impl Into<String>,
    settings: &AppSettings,
) {
    let _ = app.emit(
        STATUS_EVENT,
        &StatusPayload {
            state,
            message: message.into(),
            fast_model: settings.interview_fast_model.clone(),
            deep_model: settings
                .interview_deep_enabled
                .then(|| settings.interview_deep_model.clone()),
        },
    );
}

impl InterviewRuntime {
    /// Spawn both sessions and send the profile so they are warm before the first question.
    pub async fn start(
        app: AppHandle,
        settings: AppSettings,
        workspace: PathBuf,
    ) -> Result<Arc<Self>, String> {
        emit_status(&app, "starting", "Starting copilot…", &settings);
        let profile = load_profile(&settings)?;

        let fast_fut = spawn_session(&settings, workspace.clone(), Tier::Fast);
        let deep_fut = async {
            if settings.interview_deep_enabled {
                spawn_session(&settings, workspace.clone(), Tier::Deep)
                    .await
                    .map(Some)
            } else {
                Ok(None)
            }
        };
        let (fast, deep) = tokio::join!(fast_fut, deep_fut);
        let fast = fast?;
        let deep = match deep {
            Ok(deep) => deep,
            Err(err) => {
                log::warn!("interview deep tier unavailable: {err}");
                None
            }
        };

        let runtime = Arc::new(Self {
            app: app.clone(),
            fast: Tiered {
                session: fast,
                answering: AtomicU64::new(0),
            },
            deep: deep.map(|session| Tiered {
                session,
                answering: AtomicU64::new(0),
            }),
            tracker: Mutex::new(InterviewTracker::new()),
            generation: AtomicU64::new(0),
            auto_answer: settings.interview_auto_answer,
        });

        // Priming runs in the background; question turns queue behind it per session.
        let priming = interview_engine::priming_turn(&profile);
        let started = Instant::now();
        let fast_prime = prime(Arc::clone(&runtime.fast.session), priming.clone());
        let deep_prime = runtime
            .deep
            .as_ref()
            .map(|deep| prime(Arc::clone(&deep.session), priming));
        let status_app = app.clone();
        let status_settings = settings.clone();
        tauri::async_runtime::spawn(async move {
            let fast_result = fast_prime.await;
            if let Some(deep_prime) = deep_prime {
                if let Err(err) = deep_prime.await {
                    log::warn!("interview deep priming failed: {err}");
                }
            }
            match fast_result {
                Ok(()) => {
                    log::info!(
                        "interview copilot primed in {}ms",
                        started.elapsed().as_millis()
                    );
                    emit_status(
                        &status_app,
                        "ready",
                        "Copilot ready",
                        &status_settings,
                    );
                }
                Err(err) => emit_status(&status_app, "error", err, &status_settings),
            }
        });

        Ok(runtime)
    }

    pub fn on_utterance(self: &Arc<Self>, utterance: &Utterance) {
        let trigger = self
            .tracker
            .lock()
            .unwrap()
            .on_utterance(&utterance.speaker, &utterance.text);
        if let Some(trigger) = trigger {
            if self.auto_answer {
                self.ask(trigger.question, trigger.signal, false, false);
            }
        }
    }

    /// Answer on demand: a typed question (answered as the user's own knowledge
    /// question), or what the interviewer just said.
    pub fn answer_now(self: &Arc<Self>, question: Option<String>) -> Result<(), String> {
        match question
            .map(|q| q.trim().to_string())
            .filter(|q| !q.is_empty())
        {
            Some(typed) => self.ask(typed, QuestionSignal::Explicit, true, true),
            None => {
                let heard = self
                    .tracker
                    .lock()
                    .unwrap()
                    .take_manual_question()
                    .ok_or("Nothing from the other side yet — type a question instead")?;
                self.ask(heard, QuestionSignal::Explicit, true, false);
            }
        }
        Ok(())
    }

    fn ask(self: &Arc<Self>, question: String, signal: QuestionSignal, manual: bool, direct: bool) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let id = uuid::Uuid::new_v4().to_string();
        let context = self.tracker.lock().unwrap().take_context(&question);
        log::info!(
            "interview question: signal={signal:?} manual={manual} direct={direct} chars={} contextLines={}",
            question.chars().count(),
            context.len()
        );

        let _ = self.app.emit(
            QUESTION_EVENT,
            &QuestionPayload {
                id: id.clone(),
                question: question.clone(),
                manual,
                asked_at: chrono::Utc::now().to_rfc3339(),
            },
        );

        let turn = if direct {
            interview_engine::direct_question_turn(&question, &context)
        } else {
            interview_engine::question_turn(&question, &context)
        };
        let started = Instant::now();
        let (gate_open_tx, gate_open_rx) = oneshot::channel::<bool>();

        let runtime = Arc::clone(self);
        let fast_turn = turn.clone();
        let fast_id = id.clone();
        tauri::async_runtime::spawn(async move {
            runtime
                .run_tier(
                    Tier::Fast,
                    fast_id,
                    fast_turn,
                    generation,
                    started,
                    Some(gate_open_tx),
                )
                .await;
        });

        if self.deep.is_some() {
            let runtime = Arc::clone(self);
            // Explicit questions start the deep tier at once; likely ones wait for the
            // fast tier to confirm this is a real question (not NONE).
            let wait_for_gate = signal != QuestionSignal::Explicit && !manual;
            tauri::async_runtime::spawn(async move {
                if wait_for_gate && !gate_open_rx.await.unwrap_or(false) {
                    return;
                }
                runtime
                    .run_tier(Tier::Deep, id, turn, generation, started, None)
                    .await;
            });
        }
    }

    fn tier(&self, tier: Tier) -> Option<&Tiered> {
        match tier {
            Tier::Fast => Some(&self.fast),
            Tier::Deep => self.deep.as_ref(),
        }
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    async fn run_tier(
        self: Arc<Self>,
        tier: Tier,
        id: String,
        turn: Vec<ContentBlock>,
        generation: u64,
        started: Instant,
        mut gate_open: Option<oneshot::Sender<bool>>,
    ) {
        let Some(tiered) = self.tier(tier) else {
            return;
        };
        if !self.is_current(generation) {
            return;
        }
        // Cut off the previous answer still streaming on this session (its turn is
        // the active one: ours is only sent below).
        if tiered.answering.swap(generation, Ordering::SeqCst) != 0 {
            tiered.session.interrupt().await;
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        let runtime = Arc::clone(&self);
        let send = tokio::spawn(async move {
            let Some(tiered) = runtime.tier(tier) else {
                return Ok(());
            };
            let result = tiered.session.send_turn(turn, tx).await;
            // Clear only once the CLI turn has really ended, even if we stopped listening.
            let _ = tiered.answering.compare_exchange(
                generation,
                0,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            result
        });

        let emit = |kind: &'static str, text: String| {
            let _ = self.app.emit(
                ANSWER_EVENT,
                &AnswerPayload {
                    id: id.clone(),
                    tier,
                    kind,
                    text,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                },
            );
        };

        let mut gate = NoneGate::default();
        let mut released = false;
        let mut dismissed = false;
        while let Some(event) = rx.recv().await {
            if !self.is_current(generation) {
                // Superseded: the newer question's run_tier interrupts this turn.
                break;
            }
            let output = match &event {
                TurnEvent::TextDelta(delta) => gate.push(delta),
                TurnEvent::Done { .. } => gate.finish(),
                TurnEvent::Error(_) => GateOutput::Pending,
            };
            match output {
                GateOutput::Emit(text) => {
                    if !released {
                        released = true;
                        log::info!(
                            "interview {tier:?} first text after {}ms",
                            started.elapsed().as_millis()
                        );
                        if let Some(tx) = gate_open.take() {
                            let _ = tx.send(true);
                        }
                    }
                    if !text.is_empty() {
                        emit("delta", text);
                    }
                }
                GateOutput::Suppressed => {
                    if !dismissed {
                        dismissed = true;
                        if let Some(tx) = gate_open.take() {
                            let _ = tx.send(false);
                        }
                        emit("dismissed", String::new());
                    }
                }
                GateOutput::Pending => {}
            }
            match event {
                TurnEvent::Done { text, .. } => {
                    if !gate.is_suppressed() {
                        emit("done", text);
                    }
                    break;
                }
                TurnEvent::Error(err) => {
                    if err != "interrupted" {
                        emit("error", err);
                    }
                    break;
                }
                TurnEvent::TextDelta(_) => {}
            }
        }

        if let Some(tx) = gate_open.take() {
            let _ = tx.send(false);
        }
        if !self.is_current(generation) {
            return;
        }
        if let Ok(Err(err)) = send.await {
            if self.is_current(generation) {
                emit("error", err);
            }
        }
    }

    pub async fn shutdown(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.fast.session.interrupt().await;
        if let Some(deep) = &self.deep {
            deep.session.interrupt().await;
        }
    }
}

async fn prime(
    session: Arc<dyn ConversationSession>,
    turn: Vec<ContentBlock>,
) -> Result<(), String> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let send = session.send_turn(turn, tx);
    let wait = async {
        while let Some(event) = rx.recv().await {
            match event {
                TurnEvent::Done { .. } => return Ok(()),
                TurnEvent::Error(err) => return Err(err),
                TurnEvent::TextDelta(_) => {}
            }
        }
        Err("Interview session closed while loading the profile".to_string())
    };
    let (sent, waited) = tokio::join!(send, wait);
    sent?;
    waited
}

pub fn status_stopped(app: &AppHandle, settings: &AppSettings) {
    emit_status(app, "stopped", "Copilot stopped", settings);
}
