// Static index of settings for the search box. EP0007 / US0030.
// Maps each setting label to its tab and whether it lives in the advanced group,
// so search can jump to the right tab and auto-expand advanced when needed.

export type SettingTab = "audio" | "engines" | "ai" | "interview" | "prompts";

export interface SettingEntry {
  label: string;
  tab: SettingTab;
  advanced: boolean;
  keywords?: string;
}

export const SETTINGS_INDEX: SettingEntry[] = [
  { label: "Mic Input", tab: "audio", advanced: false, keywords: "microphone device input source" },
  { label: "System Audio", tab: "audio", advanced: false, keywords: "loopback output device source" },
  { label: "Mic Capture Mode", tab: "audio", advanced: false, keywords: "push to talk auto gate" },
  { label: "Push-to-Talk Shortcut", tab: "audio", advanced: false, keywords: "hotkey ptt shortcut" },
  { label: "Mic Voice Threshold", tab: "audio", advanced: false, keywords: "calibration sensitivity gate noise" },
  { label: "Hide from screen sharing", tab: "audio", advanced: false, keywords: "privacy content protection screenshare" },
  { label: "STT Provider", tab: "engines", advanced: false, keywords: "transcription backend whisper parakeet gemma cohere omni" },
  { label: "Language / Locale", tab: "engines", advanced: false, keywords: "language locale transcription" },
  { label: "Whisper Model", tab: "engines", advanced: true, keywords: "model whisper" },
  { label: "Device (CPU/CUDA)", tab: "engines", advanced: true, keywords: "device cpu cuda gpu rocm" },
  { label: "Compute Type", tab: "engines", advanced: true, keywords: "int8 float compute precision" },
  { label: "Speaker diarization", tab: "engines", advanced: true, keywords: "diarization speakers" },
  { label: "Echo cancellation", tab: "engines", advanced: true, keywords: "echo aec" },
  { label: "Hugging Face Token", tab: "engines", advanced: true, keywords: "hf token cohere gated" },
  { label: "AI Mode", tab: "ai", advanced: false, keywords: "local cloud gemma codex claude ollama mode" },
  { label: "Language Model", tab: "ai", advanced: false, keywords: "llm model openrouter ollama openai api key" },
  { label: "Embeddings", tab: "ai", advanced: true, keywords: "voyage embedding knowledge search" },
  { label: "Knowledge & Notes (Obsidian)", tab: "ai", advanced: false, keywords: "obsidian vault kb notes template" },
  { label: "Interview Mode", tab: "interview", advanced: false, keywords: "interview job resume cv copilot claude opus haiku answer questions consultant assistant role ask" },
  { label: "Resume", tab: "interview", advanced: false, keywords: "resume cv pdf candidate" },
  { label: "Job Description", tab: "interview", advanced: false, keywords: "role job posting company" },
  { label: "Suggestion Prompts", tab: "prompts", advanced: false, keywords: "suggestion prompt customize" },
];

export function searchSettings(query: string): SettingEntry[] {
  const n = query.trim().toLowerCase();
  if (!n) return [];
  return SETTINGS_INDEX.filter(
    (e) => e.label.toLowerCase().includes(n) || (e.keywords ?? "").toLowerCase().includes(n),
  );
}
