// Static select-option data for the settings UI.
// Extracted from SettingsView.tsx (EP0007 / US0029) to shrink the monolith.

export const transcriptionLocaleOptions = [
  { value: "auto", label: "Auto Detect" },
  { value: "en-US", label: "English (US)" },
  { value: "en-GB", label: "English (UK)" },
  { value: "es-ES", label: "Spanish (Spain)" },
  { value: "es-CO", label: "Spanish (Colombia)" },
  { value: "es-MX", label: "Spanish (Mexico)" },
  { value: "fr-FR", label: "French" },
  { value: "de-DE", label: "German" },
  { value: "pt-BR", label: "Portuguese (Brazil)" },
  { value: "pt-PT", label: "Portuguese (Portugal)" },
  { value: "it-IT", label: "Italian" },
  { value: "bg-BG", label: "Bulgarian" },
  { value: "hr-HR", label: "Croatian" },
  { value: "cs-CZ", label: "Czech" },
  { value: "da-DK", label: "Danish" },
  { value: "nl-NL", label: "Dutch" },
  { value: "et-EE", label: "Estonian" },
  { value: "fi-FI", label: "Finnish" },
  { value: "el-GR", label: "Greek" },
  { value: "hu-HU", label: "Hungarian" },
  { value: "lv-LV", label: "Latvian" },
  { value: "lt-LT", label: "Lithuanian" },
  { value: "mt-MT", label: "Maltese" },
  { value: "pl-PL", label: "Polish" },
  { value: "ro-RO", label: "Romanian" },
  { value: "ru-RU", label: "Russian" },
  { value: "sk-SK", label: "Slovak" },
  { value: "sl-SI", label: "Slovenian" },
  { value: "sv-SE", label: "Swedish" },
  { value: "uk-UA", label: "Ukrainian" },
];

export const whisperModelOptions = [
  { value: "auto", label: "Auto", description: "Base multilingual with automatic language detection for English and Spanish" },
  { value: "tiny", label: "Tiny", description: "Fastest, lowest accuracy" },
  { value: "base", label: "Base", description: "Lightweight multilingual model" },
  { value: "small", label: "Small", description: "Recommended multilingual balance with automatic language detection" },
  { value: "medium", label: "Medium", description: "Higher accuracy, more CPU/RAM" },
  { value: "large-v3-turbo", label: "Large v3 Turbo", description: "Best local multilingual accuracy, heaviest option" },
];

export const sttProviderOptions = [
  { value: "whisper-rs", label: "whisper-rs (Recommended)", description: "Default. In-process local transcription with ggml Whisper models. No extra dependencies — just a small model download." },
  {
    value: "llama-server",
    label: "Gemma 4 12B (llama-server)",
    description:
      "Local Gemma 4 12B multimodal audio transcription via a bundled llama.cpp server. No Python required - one-click setup downloads everything (~7.6 GB) and reuses LM Studio models when found. The same server also powers the local AI agent.",
  },
  { value: "faster-whisper", label: "faster-whisper", description: "Worker-based backend with app-managed Python runtime. Requires Python 3 on your system." },
  {
    value: "fluidaudio",
    label: "Parakeet on Neural Engine (macOS)",
    description:
      "NVIDIA Parakeet TDT v3 compiled for the Apple Neural Engine via FluidAudio. 25 languages with auto-detect, fastest local option on Apple Silicon. No Python — one-click ~470 MB model download. Includes speaker diarization.",
  },
  { value: "parakeet", label: "parakeet", description: "NVIDIA Parakeet TDT v3 — multilingual, 25 languages, automatic language detection. Requires Python 3 and ~3 GB disk." },
  { value: "omni-asr", label: "Omni-ASR", description: "facebookresearch/omnilingual-asr — supports 1,600+ languages. Requires Python 3 (Linux/macOS) or WSL2 + Python 3 (Windows)." },
  {
    value: "cohere-transcribe",
    label: "Cohere Transcribe",
    description:
      "Local CohereLabs/cohere-transcribe-03-2026 via Transformers. Requires Python 3, a Hugging Face token, and a supported explicit locale.",
  },
];

export const fluidAudioModelOptions = [
  { value: "v3", label: "Parakeet TDT 0.6B v3 (Recommended)", description: "NVIDIA's multilingual model: 25 European languages with automatic language detection." },
  { value: "ultra", label: "Parakeet Ultra", description: "Full-precision post-training of v3 with the same languages; separate ~470 MB download." },
  { value: "v2", label: "Parakeet TDT 0.6B v2 (English only)", description: "English-only model — never switches languages. Use it for English-only calls." },
];

export const parakeetModelOptions = [
  { value: "nvidia/parakeet-tdt-0.6b-v3", label: "Parakeet TDT 0.6B v3 (Recommended)", description: "600M params, 25 languages, best speed/accuracy balance." },
  { value: "nvidia/parakeet-tdt_ctc-1.1b", label: "Parakeet TDT 1.1B v2", description: "1.1B params, English-focused, highest accuracy." },
];

export const parakeetDeviceOptions = [
  { value: "auto", label: "Auto" },
  { value: "cpu", label: "CPU" },
  { value: "cuda", label: "CUDA (NVIDIA GPU)" },
];

export const omniAsrModelOptions = [
  { value: "omniASR_LLM_Unlimited_300M_v2", label: "omniASR LLM Unlimited 300M v2 (Fast)", description: "Fastest unlimited-length model." },
  { value: "omniASR_LLM_Unlimited_1B_v2",   label: "omniASR LLM Unlimited 1B v2",          description: "Balanced speed and accuracy." },
  { value: "omniASR_LLM_Unlimited_3B_v2",   label: "omniASR LLM Unlimited 3B v2",          description: "High accuracy." },
  { value: "omniASR_LLM_Unlimited_7B_v2",   label: "omniASR LLM Unlimited 7B v2 (Best)",   description: "Highest accuracy, requires more VRAM." },
];

export const omniAsrDeviceOptions = [
  { value: "auto", label: "Auto" },
  { value: "cpu", label: "CPU" },
  { value: "cuda", label: "CUDA" },
];

export const cohereTranscribeModelOptions = [
  {
    value: "CohereLabs/cohere-transcribe-03-2026",
    label: "Cohere Transcribe 03-2026",
    description:
      "Local Cohere multilingual speech-to-text model. Requires gated Hugging Face access.",
  },
];

export const cohereTranscribeDeviceOptions = [
  { value: "auto", label: "Auto" },
  { value: "cpu", label: "CPU" },
  { value: "cuda", label: "CUDA" },
  { value: "rocm-windows", label: "ROCm (Windows Experimental)" },
  { value: "wsl-rocm", label: "ROCm via WSL" },
];

export const fasterWhisperModelOptions = [
  { value: "tiny", label: "Tiny" },
  { value: "base", label: "Base" },
  { value: "small", label: "Small" },
  { value: "medium", label: "Medium" },
  { value: "large-v3", label: "Large v3" },
];

export const fasterWhisperComputeTypeOptions = [
  { value: "default", label: "Default" },
  { value: "int8", label: "INT8" },
  { value: "float16", label: "Float16" },
  { value: "float32", label: "Float32" },
];

export const fasterWhisperDeviceOptions = [
  { value: "auto", label: "Auto" },
  { value: "cpu", label: "CPU" },
  { value: "cuda", label: "CUDA" },
];
