//! Interview mode: detect interviewer questions in the live transcript and
//! build the turns for two persistent conversations — a fast tier that drafts
//! what the candidate can say right now, and a deep tier (Opus) that adds the
//! technical depth a moment later. Transport-agnostic: the caller owns the
//! `ConversationSession`s and streams their events to the UI.

use crate::intelligence::conversation::ContentBlock;
use crate::intelligence::notes_engine::language_response_instruction;
use crate::models::Speaker;
use base64::Engine as _;
use std::path::Path;

/// Reply the fast tier gives when the interviewer's words need no answer.
pub const NO_ANSWER_TOKEN: &str = "NONE";
const MAX_PENDING_QUESTION_CHARS: usize = 600;
const MAX_CONTEXT_LINES: usize = 40;
const MIN_QUESTION_WORDS: usize = 3;
const MAX_RESUME_BYTES: u64 = 10 * 1024 * 1024;

/// Who the copilot sits next to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CopilotRole {
    /// The user is a candidate in a job interview.
    #[default]
    Interview,
    /// The user is in any call (client, consulting, sales) and wants an expert at hand.
    Consultant,
}

impl CopilotRole {
    pub fn from_setting(value: &str) -> Self {
        match value.trim() {
            "consultant" => Self::Consultant,
            _ => Self::Interview,
        }
    }
}

/// Prefix that marks a turn as the user's own question, not the other party's words.
const DIRECT_QUESTION_LABEL: &str = "Direct question from the user:";

#[derive(Debug, Clone, Default)]
pub struct InterviewProfile {
    pub role: CopilotRole,
    pub resume: Option<ContentBlock>,
    pub job_description: String,
    pub extra_context: String,
    /// BCP-47 locale from transcription settings ("auto" or empty = match the interviewer).
    pub response_language: String,
}

/// Load a resume as a content block: PDFs go to Claude as documents, text formats inline.
pub fn load_resume(path: &Path) -> Result<ContentBlock, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("Cannot read resume {}: {e}", path.display()))?;
    if metadata.len() > MAX_RESUME_BYTES {
        return Err("Resume is larger than 10 MB".into());
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let bytes =
        std::fs::read(path).map_err(|e| format!("Cannot read resume {}: {e}", path.display()))?;

    match extension.as_str() {
        "pdf" => Ok(ContentBlock::Document {
            media_type: "application/pdf".into(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }),
        "docx" | "doc" | "pages" | "odt" => Err(
            "Save the resume as PDF, Markdown or plain text — Word/Pages files are not supported."
                .into(),
        ),
        _ => {
            let text = String::from_utf8(bytes)
                .map_err(|_| "Resume must be a PDF or a UTF-8 text/Markdown file".to_string())?;
            Ok(ContentBlock::text(format!(
                "Candidate resume:\n\n{}",
                text.trim()
            )))
        }
    }
}

pub fn fast_system_prompt(language: &str, role: CopilotRole) -> String {
    let mut prompt = match role {
        CopilotRole::Interview => interview_fast_prompt(),
        CopilotRole::Consultant => consultant_fast_prompt(),
    };
    push_direct_question_rules(&mut prompt, 120);
    prompt.push_str(&format!(
        "If the other party's latest words are not a question or a prompt for the user (small talk, an acknowledgement, someone describing their company), reply with exactly {NO_ANSWER_TOKEN} and nothing else. Never reply {NO_ANSWER_TOKEN} to a direct question."
    ));
    append_language(&mut prompt, language);
    prompt
}

fn interview_fast_prompt() -> String {
    String::from(
        "You are a real-time interview copilot sitting next to the candidate during a live job interview. \
The candidate's resume and the role are in the first message. The interviewer is speaking now; the transcript comes from speech-to-text and may contain recognition errors — infer the intended question.\n\n\
When given the interviewer's latest words, write what the candidate can SAY, immediately:\n\
- First line: a one-sentence direct answer the candidate can start speaking with.\n\
- Then 2-3 short bullets with the key points, in the order to say them.\n\
- First person, natural spoken language, no filler.\n\
- Behavioral questions: compressed STAR using real experience from the resume (company, project, result).\n\
- Technical questions: the correct, precise answer with the key terms an expert would use.\n\
- Never invent employers, titles, degrees or numbers that are not in the resume; if the resume has no direct example, give an honest framing from adjacent experience.\n\
- Keep it under 75 words. No headings, no preamble, no meta commentary.\n\
- Use earlier turns for follow-ups (\"why?\", \"what about at scale?\") and stay consistent with what the candidate already said.\n\n",
    )
}

fn consultant_fast_prompt() -> String {
    String::from(
        "You are a real-time expert assistant sitting next to the user during a live call (client meeting, consulting session, sales or technical discussion). \
Context about the user and the engagement is in the first message. Others are speaking; the transcript comes from speech-to-text and may contain recognition errors — infer what was meant.\n\n\
When given the other party's latest words and they ask the user something, write what the user can SAY, immediately:\n\
- First line: a one-sentence direct answer the user can start speaking with.\n\
- Then 2-3 short bullets with the key facts or recommendations, in the order to say them.\n\
- First person, confident, natural spoken language, no filler.\n\
- Be technically and factually exact; name the concrete tool, standard, figure or trade-off an expert would cite.\n\
- Never invent facts about the user or their company that are not in the context.\n\
- Keep it under 75 words. No headings, no preamble, no meta commentary.\n\
- Use earlier turns for follow-ups and stay consistent with what the user already said.\n\n",
    )
}

/// Typed questions come from the user, so they get a knowledge answer, never a script or NONE.
fn push_direct_question_rules(prompt: &mut String, max_words: usize) {
    prompt.push_str(&format!(
        "Messages that start with \"{DIRECT_QUESTION_LABEL}\" come from the user, not from the other party. Answer those as a knowledgeable assistant: \
the facts the user needs, precise and complete, with names, versions, commands or short code where useful — not a line to recite. \
If the question refers to the conversation (\"what did they mean\", \"is that right\"), use the transcript so far. \
Start with the direct answer, then compact bullets. Under {max_words} words.\n\n"
    ));
}

pub fn deep_system_prompt(language: &str, role: CopilotRole) -> String {
    let mut prompt = match role {
        CopilotRole::Interview => interview_deep_prompt(),
        CopilotRole::Consultant => consultant_deep_prompt(),
    };
    push_direct_question_rules(&mut prompt, 250);
    prompt.push_str(&format!(
        "If the other party's latest words are not a question or a prompt for the user, reply with exactly {NO_ANSWER_TOKEN}. Never reply {NO_ANSWER_TOKEN} to a direct question."
    ));
    append_language(&mut prompt, language);
    prompt
}

fn interview_deep_prompt() -> String {
    String::from(
        "You are the research companion for a candidate in a live job interview. The resume and the role are in the first message. \
A faster assistant already gives the candidate a short spoken answer; your job is the depth the candidate can glance at while talking.\n\n\
For each interviewer question:\n\
- Lead with the core concept or the strongest angle, in one line.\n\
- Then compact markdown bullets: how it works, trade-offs, complexity or numbers that matter, common pitfalls, and how it maps to the candidate's resume.\n\
- For coding or system-design questions include a minimal code or pseudocode sketch or a component outline.\n\
- End with one smart point or question the candidate can raise to stand out.\n\
- Be technically exact. Under 220 words. No preamble.\n\
- Use the conversation so far for follow-ups.\n\n",
    )
}

fn consultant_deep_prompt() -> String {
    String::from(
        "You are the research companion for the user in a live call. Context about the user and the engagement is in the first message. \
A faster assistant already gives the user a short answer; your job is the depth the user can glance at while talking.\n\n\
For each question:\n\
- Lead with the core concept or the strongest recommendation, in one line.\n\
- Then compact markdown bullets: how it works, options and trade-offs, numbers that matter, risks and pitfalls.\n\
- Include a minimal code, command or architecture sketch when it helps.\n\
- End with one sharp follow-up point or question the user can raise.\n\
- Be technically exact. Under 220 words. No preamble.\n\
- Use the conversation so far for follow-ups.\n\n",
    )
}

fn append_language(prompt: &mut String, language: &str) {
    let locale = language.trim().to_ascii_lowercase();
    if locale.starts_with("en") {
        // Pinned to English: never follow speech-to-text that misdetects the language.
        prompt.push_str("\nAlways answer in English, even if the transcript contains other languages.");
        return;
    }
    let instruction = language_response_instruction(language);
    if instruction.is_empty() {
        prompt.push_str("\nAnswer in the language the interviewer is using.");
    } else {
        prompt.push('\n');
        prompt.push_str(&instruction);
    }
}

/// First turn of each session: the candidate profile. Sent at session start so
/// the process is warm and the resume is cached before the first question.
pub fn priming_turn(profile: &InterviewProfile) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    if let Some(resume) = &profile.resume {
        blocks.push(resume.clone());
    }
    let consultant = profile.role == CopilotRole::Consultant;
    let mut text = String::from(if consultant {
        "Context for this session.\n"
    } else {
        "Interview context for this session.\n"
    });
    if profile.resume.is_none() && !consultant {
        text.push_str("\nNo resume was provided; keep answers general and honest.\n");
    }
    if !profile.job_description.trim().is_empty() {
        text.push_str(&format!(
            "\n{}:\n{}\n",
            if consultant { "Engagement / client" } else { "Role / job description" },
            profile.job_description.trim()
        ));
    }
    if !profile.extra_context.trim().is_empty() {
        text.push_str(&format!(
            "\n{}:\n{}\n",
            if consultant { "Notes from the user" } else { "Candidate notes" },
            profile.extra_context.trim()
        ));
    }
    text.push_str("\nRead it now. Reply with exactly OK. Questions follow in the next messages.");
    blocks.push(ContentBlock::text(text));
    blocks
}

pub fn question_turn(question: &str, recent_context: &[String]) -> Vec<ContentBlock> {
    let mut text = String::new();
    if !recent_context.is_empty() {
        text.push_str("Transcript since the last question:\n");
        text.push_str(&recent_context.join("\n"));
        text.push_str("\n\n");
    }
    text.push_str("Interviewer's latest words:\n");
    text.push_str(question.trim());
    vec![ContentBlock::text(text)]
}

/// A question the user typed: answered from knowledge, with the transcript as context.
pub fn direct_question_turn(question: &str, recent_context: &[String]) -> Vec<ContentBlock> {
    let mut text = String::new();
    if !recent_context.is_empty() {
        text.push_str("Transcript since the last question:\n");
        text.push_str(&recent_context.join("\n"));
        text.push_str("\n\n");
    }
    text.push_str(DIRECT_QUESTION_LABEL);
    text.push('\n');
    text.push_str(question.trim());
    vec![ContentBlock::text(text)]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum QuestionSignal {
    None,
    /// Imperative prompt or interrogative opener without a question mark (STT often drops it).
    Likely,
    /// Contains a question mark.
    Explicit,
}

const QUESTION_OPENERS: &[&str] = &[
    // English
    "what",
    "why",
    "how",
    "when",
    "where",
    "which",
    "who",
    "can you",
    "could you",
    "would you",
    "will you",
    "do you",
    "did you",
    "have you",
    "are you",
    "were you",
    "is there",
    "is it",
    "tell me",
    "walk me through",
    "walk us through",
    "explain",
    "describe",
    "give me",
    "talk about",
    "talk me through",
    "let's say",
    "imagine",
    "suppose",
    "design",
    "implement",
    "write",
    "compare",
    "what's",
    "how's",
    "share",
    // Spanish
    "qué",
    "que",
    "cómo",
    "como",
    "por qué",
    "porque",
    "cuál",
    "cuáles",
    "cuándo",
    "dónde",
    "quién",
    "puedes",
    "podrías",
    "cuéntame",
    "cuentame",
    "explícame",
    "explicame",
    "explica",
    "descríbeme",
    "describe",
    "háblame",
    "hablame",
    "dime",
    "diseña",
    "imagina",
    "supón",
    "has ",
    "tienes",
    "sabes",
];

const QUESTION_PHRASES: &[&str] = &[
    "tell me about",
    "walk me through",
    "how would you",
    "what would you",
    "can you explain",
    "could you explain",
    "what is the difference",
    "what's the difference",
    "difference between",
    "how do you",
    "have you ever",
    "give me an example",
    "cuéntame sobre",
    "cómo harías",
    "qué harías",
    "diferencia entre",
    "dame un ejemplo",
];

const FILLERS: &[&str] = &[
    "so", "okay", "ok", "alright", "and", "well", "now", "great", "right", "bueno", "entonces",
    "vale", "y", "ahora",
];

pub fn classify_question(text: &str) -> QuestionSignal {
    let lower = text.to_lowercase();
    if lower.split_whitespace().count() < MIN_QUESTION_WORDS {
        return QuestionSignal::None;
    }
    if lower.contains('?') || lower.contains('¿') {
        return QuestionSignal::Explicit;
    }
    if QUESTION_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return QuestionSignal::Likely;
    }
    let starts_like_question =
        lower
            .split(['.', '!', ';', '\n'])
            .map(strip_fillers)
            .any(|sentence| {
                QUESTION_OPENERS.iter().any(|opener| {
                    sentence.starts_with(opener)
                        && sentence[opener.len()..]
                            .chars()
                            .next()
                            .map_or(false, |c| !c.is_alphanumeric())
                })
            });
    if starts_like_question {
        QuestionSignal::Likely
    } else {
        QuestionSignal::None
    }
}

fn strip_fillers(sentence: &str) -> &str {
    let mut rest = sentence.trim();
    loop {
        let word_end = rest
            .find(|c: char| c.is_whitespace() || c == ',')
            .unwrap_or(rest.len());
        let word = &rest[..word_end];
        if word_end < rest.len() && FILLERS.contains(&word) {
            rest = rest[word_end..].trim_start_matches([',', ' ']).trim_start();
        } else {
            return rest;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionTrigger {
    pub question: String,
    pub signal: QuestionSignal,
}

/// Folds finalized utterances into question triggers and the running context
/// that each new turn carries.
#[derive(Debug, Default)]
pub struct InterviewTracker {
    pending_question: String,
    context: Vec<String>,
}

impl InterviewTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_utterance(&mut self, speaker: &Speaker, text: &str) -> Option<QuestionTrigger> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        match speaker {
            Speaker::You => {
                self.push_context(format!("Candidate: {text}"));
                self.pending_question.clear();
                None
            }
            _ => {
                self.push_context(format!("Interviewer: {text}"));
                if !self.pending_question.is_empty() {
                    self.pending_question.push(' ');
                }
                self.pending_question.push_str(text);
                if self.pending_question.chars().count() > MAX_PENDING_QUESTION_CHARS {
                    let skip = self.pending_question.chars().count() - MAX_PENDING_QUESTION_CHARS;
                    self.pending_question = self.pending_question.chars().skip(skip).collect();
                }
                let signal = classify_question(&self.pending_question);
                if signal == QuestionSignal::None {
                    return None;
                }
                let question = std::mem::take(&mut self.pending_question);
                Some(QuestionTrigger { question, signal })
            }
        }
    }

    /// Manual "answer now": whatever the interviewer said since the last trigger,
    /// else the last interviewer line.
    pub fn take_manual_question(&mut self) -> Option<String> {
        let pending = std::mem::take(&mut self.pending_question);
        if !pending.trim().is_empty() {
            return Some(pending);
        }
        self.context
            .iter()
            .rev()
            .find_map(|line| line.strip_prefix("Interviewer: ").map(str::to_string))
    }

    /// Context lines accumulated since the previous turn, minus the trigger itself.
    pub fn take_context(&mut self, question: &str) -> Vec<String> {
        let mut lines = std::mem::take(&mut self.context);
        let question = question.trim();
        while lines
            .last()
            .and_then(|line| line.strip_prefix("Interviewer: "))
            .map_or(false, |text| question.ends_with(text.trim()))
        {
            lines.pop();
        }
        lines
    }

    fn push_context(&mut self, line: String) {
        self.context.push(line);
        if self.context.len() > MAX_CONTEXT_LINES {
            self.context.remove(0);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateOutput {
    /// Not enough text yet to tell an answer from the NONE token.
    Pending,
    /// The answer is real: emit this text (buffered prefix included).
    Emit(String),
    /// The model replied NONE: hide this answer.
    Suppressed,
}

/// Holds back the first characters of a streamed answer until it is clear it is not "NONE".
#[derive(Debug, Default)]
pub struct NoneGate {
    buffer: String,
    state: GateState,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum GateState {
    #[default]
    Buffering,
    Open,
    Suppressed,
}

impl NoneGate {
    pub fn push(&mut self, delta: &str) -> GateOutput {
        match self.state {
            GateState::Open => GateOutput::Emit(delta.to_string()),
            GateState::Suppressed => GateOutput::Suppressed,
            GateState::Buffering => {
                self.buffer.push_str(delta);
                let head = self.buffer.trim_start();
                if NO_ANSWER_TOKEN.starts_with(head) {
                    // Empty, or a prefix of "NONE" (or exactly "NONE"): wait for more.
                    return GateOutput::Pending;
                }
                if let Some(rest) = head.strip_prefix(NO_ANSWER_TOKEN) {
                    if !rest.starts_with(char::is_alphanumeric) {
                        if rest
                            .trim_matches(|c: char| c.is_whitespace() || c == '.')
                            .is_empty()
                        {
                            return GateOutput::Pending;
                        }
                        self.state = GateState::Suppressed;
                        return GateOutput::Suppressed;
                    }
                    // A longer word such as "NONEXISTENT" is a real answer.
                }
                self.state = GateState::Open;
                GateOutput::Emit(std::mem::take(&mut self.buffer))
            }
        }
    }

    /// Called when the turn ends; a bare "NONE" resolves to suppressed.
    pub fn finish(&mut self) -> GateOutput {
        match self.state {
            GateState::Open => GateOutput::Emit(String::new()),
            GateState::Suppressed => GateOutput::Suppressed,
            GateState::Buffering => {
                let head = self.buffer.trim().trim_end_matches('.');
                if head.is_empty() || head == NO_ANSWER_TOKEN {
                    self.state = GateState::Suppressed;
                    GateOutput::Suppressed
                } else {
                    self.state = GateState::Open;
                    GateOutput::Emit(std::mem::take(&mut self.buffer))
                }
            }
        }
    }

    pub fn is_suppressed(&self) -> bool {
        self.state == GateState::Suppressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_question_marks() {
        assert_eq!(
            classify_question("Why did you choose Rust there?"),
            QuestionSignal::Explicit
        );
        assert_eq!(
            classify_question("¿Por qué elegiste Rust para eso"),
            QuestionSignal::Explicit
        );
    }

    #[test]
    fn imperative_prompts_without_punctuation() {
        assert_eq!(
            classify_question("So tell me about a time you disagreed with your manager"),
            QuestionSignal::Likely
        );
        assert_eq!(
            classify_question("Okay, walk me through how a hash map handles collisions"),
            QuestionSignal::Likely
        );
        assert_eq!(
            classify_question("How would you design a rate limiter"),
            QuestionSignal::Likely
        );
        assert_eq!(
            classify_question("Cuéntame de un proyecto difícil que hayas liderado"),
            QuestionSignal::Likely
        );
        assert_eq!(
            classify_question("Great. Explain the CAP theorem in your own words"),
            QuestionSignal::Likely
        );
    }

    #[test]
    fn statements_are_not_questions() {
        assert_eq!(
            classify_question("We are a team of twelve engineers in Berlin"),
            QuestionSignal::None
        );
        assert_eq!(
            classify_question("Thanks, that makes sense"),
            QuestionSignal::None
        );
        assert_eq!(classify_question("Why?"), QuestionSignal::None);
        // "however" must not match the "how" opener
        assert_eq!(
            classify_question("However the team moved to Go last year"),
            QuestionSignal::None
        );
    }

    #[test]
    fn tracker_joins_split_utterances_into_one_question() {
        let mut tracker = InterviewTracker::new();
        assert!(tracker
            .on_utterance(&Speaker::Them, "We use Postgres heavily here.")
            .is_none());
        let trigger = tracker
            .on_utterance(&Speaker::Them, "What would you index first?")
            .unwrap();
        assert_eq!(
            trigger.question,
            "We use Postgres heavily here. What would you index first?"
        );
        assert_eq!(trigger.signal, QuestionSignal::Explicit);
        let context = tracker.take_context(&trigger.question);
        assert_eq!(
            context,
            vec!["Interviewer: We use Postgres heavily here.".to_string()]
        );
    }

    #[test]
    fn candidate_speech_resets_pending_question_and_feeds_context() {
        let mut tracker = InterviewTracker::new();
        tracker.on_utterance(&Speaker::Them, "Our stack is mostly");
        tracker.on_utterance(&Speaker::You, "I mostly used Kafka at Acme");
        let trigger = tracker
            .on_utterance(&Speaker::Them, "Why Kafka over RabbitMQ?")
            .unwrap();
        assert_eq!(trigger.question, "Why Kafka over RabbitMQ?");
        let context = tracker.take_context(&trigger.question);
        assert_eq!(context.len(), 2);
        assert!(context[1].starts_with("Candidate:"));
    }

    #[test]
    fn manual_question_falls_back_to_last_interviewer_line() {
        let mut tracker = InterviewTracker::new();
        tracker.on_utterance(&Speaker::Them, "Our biggest problem is the deploy pipeline");
        assert_eq!(
            tracker.take_manual_question().as_deref(),
            Some("Our biggest problem is the deploy pipeline")
        );
        assert_eq!(
            tracker.take_manual_question().as_deref(),
            Some("Our biggest problem is the deploy pipeline")
        );
    }

    #[test]
    fn none_gate_suppresses_none_and_passes_answers() {
        let mut gate = NoneGate::default();
        assert_eq!(gate.push("NO"), GateOutput::Pending);
        assert_eq!(gate.push("NE"), GateOutput::Pending);
        assert_eq!(gate.finish(), GateOutput::Suppressed);

        let mut gate = NoneGate::default();
        assert_eq!(gate.push("N"), GateOutput::Pending);
        assert_eq!(
            gate.push("o, I would"),
            GateOutput::Emit("No, I would".into())
        );
        assert_eq!(gate.push(" start"), GateOutput::Emit(" start".into()));

        let mut gate = NoneGate::default();
        assert_eq!(gate.push("I led"), GateOutput::Emit("I led".into()));

        let mut gate = NoneGate::default();
        assert_eq!(gate.push("NONE\n"), GateOutput::Pending);
        assert_eq!(gate.finish(), GateOutput::Suppressed);
        assert!(gate.is_suppressed());
    }

    #[test]
    fn priming_turn_includes_resume_and_role() {
        let profile = InterviewProfile {
            role: CopilotRole::Interview,
            resume: Some(ContentBlock::text(
                "Candidate resume:\n\nJane, Rust at Acme",
            )),
            job_description: "Senior backend engineer".into(),
            extra_context: String::new(),
            response_language: "auto".into(),
        };
        let blocks = priming_turn(&profile);
        assert_eq!(blocks.len(), 2);
        match &blocks[1] {
            ContentBlock::Text { text } => assert!(text.contains("Senior backend engineer")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn direct_question_turn_is_labelled_and_prompts_never_dismiss_it() {
        let blocks = direct_question_turn("What is the ORM of Ruby?", &["Interviewer: we use Rails".into()]);
        match &blocks[0] {
            ContentBlock::Text { text } => {
                assert!(text.contains("Interviewer: we use Rails"));
                assert!(text.ends_with(&format!("{DIRECT_QUESTION_LABEL}\nWhat is the ORM of Ruby?")));
            }
            other => panic!("unexpected {other:?}"),
        }
        for role in [CopilotRole::Interview, CopilotRole::Consultant] {
            assert!(fast_system_prompt("auto", role).contains(DIRECT_QUESTION_LABEL));
            assert!(deep_system_prompt("auto", role).contains("Never reply NONE to a direct question"));
        }
        assert!(fast_system_prompt("auto", CopilotRole::Consultant).contains("live call"));
    }

    #[test]
    fn role_parses_from_setting() {
        assert_eq!(CopilotRole::from_setting("consultant"), CopilotRole::Consultant);
        assert_eq!(CopilotRole::from_setting("interview"), CopilotRole::Interview);
        assert_eq!(CopilotRole::from_setting(""), CopilotRole::Interview);
    }

    #[test]
    fn load_resume_reads_text_and_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("resume.md");
        std::fs::write(&md, "# Jane\nRust at Acme").unwrap();
        assert!(
            matches!(load_resume(&md).unwrap(), ContentBlock::Text { text } if text.contains("Rust at Acme"))
        );

        let pdf = dir.path().join("resume.PDF");
        std::fs::write(&pdf, b"%PDF-1.4 test").unwrap();
        assert!(
            matches!(load_resume(&pdf).unwrap(), ContentBlock::Document { media_type, .. } if media_type == "application/pdf")
        );

        let docx = dir.path().join("resume.docx");
        std::fs::write(&docx, b"PK").unwrap();
        assert!(load_resume(&docx).is_err());
    }
}

/// Live end-to-end check against the real `claude` CLI:
/// `cargo test -p contextcall-core interview_live -- --ignored --nocapture`.
#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::intelligence::claude_client::{complete_turn, ClaudeCliConfig, ClaudeCliSession};
    use crate::intelligence::conversation::{ConversationSession, TurnEvent};
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc;

    async fn timed_turn(
        session: &dyn ConversationSession,
        turn: Vec<ContentBlock>,
    ) -> (u128, u128, String) {
        let started = Instant::now();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let send = session.send_turn(turn, tx);
        let collect = async {
            let mut first = None;
            let mut text = String::new();
            while let Some(event) = rx.recv().await {
                match event {
                    TurnEvent::TextDelta(delta) => {
                        first.get_or_insert(started.elapsed().as_millis());
                        text.push_str(&delta);
                    }
                    TurnEvent::Done { .. } | TurnEvent::Error(_) => break,
                }
            }
            (first.unwrap_or(0), started.elapsed().as_millis(), text)
        };
        let (_, result) = tokio::join!(send, collect);
        result
    }

    #[tokio::test]
    #[ignore]
    async fn interview_live_fast_tier_answers_and_follows_up() {
        let config = ClaudeCliConfig {
            command_path: "claude".into(),
            model: "sonnet".into(),
            system_prompt: fast_system_prompt("auto", CopilotRole::Interview),
            working_dir: std::env::temp_dir().join("contextcall-interview-test"),
            thinking: false,
            effort: None,
            web_search: false,
        };
        let session = ClaudeCliSession::spawn(&config).await.unwrap();
        let profile = InterviewProfile {
            role: CopilotRole::Interview,
            resume: Some(ContentBlock::text(
                "Candidate resume:\n\nJane Doe — Senior Backend Engineer, Acme Payments (2019–2024): built a Rust ledger service \
                 handling 40k TPS on Postgres; led migration from RabbitMQ to Kafka; mentored 4 engineers.",
            )),
            job_description: "Staff engineer, distributed systems, fintech".into(),
            extra_context: String::new(),
            response_language: "auto".into(),
        };
        let primed = Instant::now();
        complete_turn(&session, priming_turn(&profile), Duration::from_secs(90))
            .await
            .unwrap();
        println!("primed in {}ms", primed.elapsed().as_millis());

        let (first, total, text) = timed_turn(
            &session,
            question_turn("Why did you move from RabbitMQ to Kafka?", &[]),
        )
        .await;
        println!("Q1 first={first}ms total={total}ms\n{text}\n");
        assert!(text.to_lowercase().contains("kafka"));

        let (first, total, text) = timed_turn(
            &session,
            question_turn(
                "And how did you keep ordering guarantees during that?",
                &["Candidate: We needed replay.".into()],
            ),
        )
        .await;
        println!("Q2 first={first}ms total={total}ms\n{text}\n");
        assert!(!text.trim().is_empty() && text.trim() != NO_ANSWER_TOKEN);

        let (_, _, text) = timed_turn(
            &session,
            question_turn("Great, thanks. Our office is in Berlin by the way.", &[]),
        )
        .await;
        println!("small talk -> {text:?}");
        assert_eq!(text.trim(), NO_ANSWER_TOKEN);
    }

    #[tokio::test]
    #[ignore]
    async fn interview_live_consultant_answers_direct_questions_on_haiku() {
        let config = ClaudeCliConfig {
            command_path: "claude".into(),
            model: "haiku".into(),
            system_prompt: fast_system_prompt("auto", CopilotRole::Consultant),
            working_dir: std::env::temp_dir().join("contextcall-interview-test"),
            thinking: false,
            effort: None,
            web_search: false,
        };
        let session = ClaudeCliSession::spawn(&config).await.unwrap();
        let profile = InterviewProfile {
            role: CopilotRole::Consultant,
            job_description: "Client is migrating a Rails monolith".into(),
            ..Default::default()
        };
        complete_turn(&session, priming_turn(&profile), Duration::from_secs(90))
            .await
            .unwrap();

        let (first, total, text) =
            timed_turn(&session, direct_question_turn("what is the orm of ruby", &[])).await;
        println!("direct first={first}ms total={total}ms\n{text}\n");
        assert!(text.contains("Active Record") || text.contains("ActiveRecord"));

        // A short, statement-like direct question must not be dismissed.
        let (_, _, text) = timed_turn(
            &session,
            direct_question_turn("rails version", &["Interviewer: we're still on Rails 5".into()]),
        )
        .await;
        println!("short direct -> {text}");
        assert_ne!(text.trim(), NO_ANSWER_TOKEN);
    }
}
