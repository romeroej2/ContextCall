# Changelog

All notable changes to ContextCall are documented here.

Format: `## [Unreleased]

### Claude brain

- **Claude CLI provider** - new "Claude CLI" AI mode that runs your own local
  `claude` login (ContextCall never reads or stores its credentials). Live
  suggestions share one warm conversation per recording session, so each one
  builds on the previous ones instead of starting from zero. Notes and
  auto-summaries stream through Claude too.
- **Anthropic API provider** - "Anthropic API" AI mode using a Console API key
  (stored in the OS keychain) with a configurable base URL and auth header for
  compatible gateways; streaming, prompt caching and multi-turn history.
- **Shared conversation interface** - `ConversationSession` abstracts
  persistent multi-turn backends, so interview mode can run over the Claude
  CLI or the Anthropic API (Settings → Interview → Connection).

### Interview mode

- **Live interview copilot** - detects interviewer questions (question marks,
  "walk me through…", "tell me about…", Spanish openers too) the moment they
  are transcribed and streams two answers: a quick first-person answer to say
  right away (first words in ~1 s) and a deeper technical answer from Opus.
- **Grounded in your resume** - PDF, Markdown or text resume plus job
  description and notes are loaded once at session start; PDFs go to Claude
  as documents.
- **Keeps context** - both tiers remember the whole interview, so follow-ups
  make sense; a new question interrupts an answer still streaming; small talk
  is ignored.
- **Ask anytime** - type a question (answered directly, even without
  recording), or press Answer for what the interviewer just said.
- **Consultant role**, configurable quick/depth models (Haiku default for the
  quick tier), optional thinking and web search for the depth tier, English
  pinning when the language is set to English.
- **Interview workspace** - answers beside the live transcript.

### Speech-to-text

- **whisper-rs is the default engine** - in-process, no Python or server, small
  model download.
- **FluidAudio (macOS, Apple Silicon)** - Parakeet TDT on the Neural Engine via
  a Swift helper, with speaker labels; honors the transcription language and
  offers an English-only v2 model.

### Fixes

- Notes and auto-summaries no longer fail with `401 Unauthorized` when a CLI
  provider is selected.
- `tauri dev` no longer rebuilds in a loop while the FluidAudio helper compiles
  (`.taurignore`).

## [1.0.0] — Initial release

### Highlights

- **Live call companion** - real-time transcription of both sides of a call,
  with an overlay window hidden from screen sharing.
- **Contextual talking points** - retrieves related knowledge from your notes
  and suggests useful responses at key moments in the conversation.
- **Auto summaries** - periodic summaries (30 seconds to 10 minutes) with
  history and on-demand regeneration.
- **Flexible local AI backends** - LM Studio, llama-server (Gemma), Codex CLI,
  faster-whisper, Parakeet, and more for STT and LLM workloads.
- **Cross-platform** - Windows, macOS, and Linux builds via Tauri.

ContextCall continues work previously published as OpenCassava, itself built
on groundwork from OpenOats. This release marks the start of ContextCall as a
standalone project.
