# Changelog

All notable changes to ContextCall are documented here.

Format: `## [version] - date`, newest first.

## [1.1.1] - 2026-10-26

### Fixes

- **Spanish transcription on the FluidAudio engine (macOS)** - long stretches
  of speech are cut every ~3 s for low latency, and Parakeet v3 drifted toward
  English at each cut, so Spanish came out as look-alike nonsense ("equipo de
  ciencia" for "equipo de cinco"). Each cut is now decoded with the preceding
  audio as context and stitched by word timing, so transcripts match an uncut
  decode while finals still arrive every ~3 s for interview mode.

### Docs

- README section for Interview & Consultant mode: setup, roles, and how the
  quick and depth answers work.

## [1.1.0] - 2026-10-20

This release turns ContextCall into a live interview copilot powered by
Claude, and adds a native, Neural Engine speech engine on Apple Silicon Macs.

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
- **FluidAudio voice engine (macOS, Apple Silicon)** - Parakeet TDT running on
  the Apple Neural Engine through a bundled Swift helper, so transcription is
  fast and on-device with no Python or server and little CPU/GPU load.
  - Multilingual v3 model by default, plus an English-only v2 model.
  - Speaker labels from on-device diarization.
  - Honors the transcription language, pinning English output for `en-*`
    locales so v3 doesn't drift between languages.
  - Models download on first use; select it in Settings → Engines.

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
