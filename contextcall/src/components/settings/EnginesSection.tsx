// Engines (STT) settings section — transcription provider, model/device, prerequisites.
// Extracted from SettingsView.tsx (EP0007 / US0029); field markup via shared primitives (slice 3).

import type { Dispatch, SetStateAction } from "react";
import { colors, typography, spacing } from "../../theme";
import type { ApiKeys, AppSettings, LlamaServerStatus, SttStatus } from "../../types";
import type { SettingsStyles } from "./settingsStyles";
import { AdvancedToggle } from "./AdvancedToggle";
import { LabeledSelect } from "./fields";
import { resolveWhisperModel } from "./settingsHelpers";
import {
  transcriptionLocaleOptions,
  whisperModelOptions,
  sttProviderOptions,
  parakeetModelOptions,
  parakeetDeviceOptions,
  omniAsrModelOptions,
  omniAsrDeviceOptions,
  cohereTranscribeModelOptions,
  cohereTranscribeDeviceOptions,
  fasterWhisperModelOptions,
  fasterWhisperComputeTypeOptions,
  fasterWhisperDeviceOptions,
} from "./settingsOptions";

interface EnginesSectionProps {
  settings: AppSettings;
  apiKeys: ApiKeys;
  styles: SettingsStyles;
  applyImmediateSettings: (update: Partial<AppSettings>) => Promise<boolean>;
  handleTextFieldCommitKeyDown: (
    event: React.KeyboardEvent<HTMLInputElement>,
    commit: () => Promise<unknown>,
  ) => void;
  huggingFaceTokenDraft: string;
  setHuggingFaceTokenDraft: Dispatch<SetStateAction<string>>;
  saveHuggingFaceToken: () => Promise<void>;
  wsl2Status: { ok: boolean; message: string } | null;
  pythonStatus: { ok: boolean; message: string } | null;
  llamaStatus: LlamaServerStatus | null;
  isWindows: boolean;
  sttStatus: SttStatus | null;
  onSetupStt?: () => void;
  isSettingUpStt: boolean;
  showAdvanced: boolean;
  onToggleAdvanced: (value: boolean) => void;
}

export function EnginesSection({
  settings,
  apiKeys,
  styles,
  applyImmediateSettings,
  handleTextFieldCommitKeyDown,
  huggingFaceTokenDraft,
  setHuggingFaceTokenDraft,
  saveHuggingFaceToken,
  wsl2Status,
  pythonStatus,
  llamaStatus,
  isWindows,
  sttStatus,
  onSetupStt,
  isSettingUpStt,
  showAdvanced,
  onToggleAdvanced,
}: EnginesSectionProps) {
  const localeHint =
    settings.sttProvider === "faster-whisper"
      ? `ContextCall will use faster-whisper ${settings.fasterWhisperModel}.`
      : settings.sttProvider === "parakeet"
      ? `ContextCall will use Parakeet v3 (${settings.parakeetModel}). Language is auto-detected from audio.`
      : settings.sttProvider === "omni-asr"
      ? `ContextCall will use Omni-ASR (${settings.omniAsrModel}).`
      : settings.sttProvider === "cohere-transcribe"
      ? `ContextCall will use Cohere Transcribe (${settings.cohereTranscribeModel}) with the selected explicit locale. Unsupported or auto locales fall back to whisper-rs.`
      : `ContextCall will download and use ${resolveWhisperModel(settings.transcriptionLocale, settings.whisperModel)}. Choose Auto Detect for mixed-language conversations.`;

  const bannerStyle = (ok: boolean): React.CSSProperties => ({
    marginTop: spacing[2],
    padding: spacing[3],
    background: ok ? `${colors.success}12` : `${colors.warning}12`,
    border: `1px solid ${ok ? colors.success : colors.warning}`,
    borderRadius: 6,
  });

  return (
    <div>
      {/* Transcription Section */}
      <div style={styles.section}>
        <h4 style={styles.sectionTitle}>Transcription</h4>

        <LabeledSelect
          styles={styles}
          label="STT Provider"
          value={settings.sttProvider}
          options={sttProviderOptions}
          onChange={(v) => applyImmediateSettings({ sttProvider: v })}
          hint={sttProviderOptions.find((o) => o.value === settings.sttProvider)?.description}
        />

        <LabeledSelect
          styles={styles}
          label="Language / Locale"
          value={settings.transcriptionLocale}
          options={transcriptionLocaleOptions}
          onChange={(v) => applyImmediateSettings({ transcriptionLocale: v })}
          hint={localeHint}
        />

        <AdvancedToggle checked={showAdvanced} onChange={onToggleAdvanced} styles={styles} />
        {showAdvanced && (
          <>
            {settings.sttProvider === "whisper-rs" ? (
              <LabeledSelect
                styles={styles}
                label="Whisper Model"
                value={settings.whisperModel}
                options={whisperModelOptions}
                onChange={(v) => applyImmediateSettings({ whisperModel: v })}
                hint={
                  <>
                    {whisperModelOptions.find((o) => o.value === settings.whisperModel)?.description}. Effective model: `{resolveWhisperModel(settings.transcriptionLocale, settings.whisperModel)}`.
                  </>
                }
              />
            ) : settings.sttProvider === "faster-whisper" ? (
              <>
                <LabeledSelect
                  styles={styles}
                  label="faster-whisper Model"
                  value={settings.fasterWhisperModel}
                  options={fasterWhisperModelOptions}
                  onChange={(v) => applyImmediateSettings({ fasterWhisperModel: v })}
                />
                <div style={styles.grid}>
                  <LabeledSelect
                    styles={styles}
                    label="Device"
                    value={settings.fasterWhisperDevice}
                    options={fasterWhisperDeviceOptions}
                    onChange={(v) => applyImmediateSettings({ fasterWhisperDevice: v })}
                  />
                  <LabeledSelect
                    styles={styles}
                    label="Compute Type"
                    value={settings.fasterWhisperComputeType}
                    options={fasterWhisperComputeTypeOptions}
                    onChange={(v) => applyImmediateSettings({ fasterWhisperComputeType: v })}
                  />
                </div>
              </>
            ) : settings.sttProvider === "parakeet" ? (
              <>
                <LabeledSelect
                  styles={styles}
                  label="Parakeet Model"
                  value={settings.parakeetModel}
                  options={parakeetModelOptions}
                  onChange={(v) => applyImmediateSettings({ parakeetModel: v })}
                  hint={parakeetModelOptions.find((o) => o.value === settings.parakeetModel)?.description}
                />
                <LabeledSelect
                  styles={styles}
                  label="Device"
                  value={settings.parakeetDevice}
                  options={parakeetDeviceOptions}
                  onChange={(v) => applyImmediateSettings({ parakeetDevice: v })}
                />
                <div style={styles.fieldWrap}>
                  <label style={styles.labelStyle}>Speaker diarization</label>
                  <div style={{ display: "flex", alignItems: "center", gap: spacing[2] }}>
                    <input
                      type="checkbox"
                      id="diarization-toggle"
                      checked={settings.diarizationEnabled ?? true}
                      onChange={(e) => applyImmediateSettings({ diarizationEnabled: e.target.checked })}
                    />
                    <label htmlFor="diarization-toggle" style={{ fontSize: typography.sm, color: colors.text, cursor: "pointer" }}>
                      Enabled
                    </label>
                  </div>
                  <span style={{ fontSize: typography.sm, color: colors.textMuted, marginTop: 4, display: "block" }}>
                    Automatically identify different speakers in call audio
                  </span>
                </div>
                <div style={styles.fieldWrap}>
                  <label style={styles.labelStyle}>Echo cancellation</label>
                  <div style={{ display: "flex", alignItems: "center", gap: spacing[2] }}>
                    <input
                      type="checkbox"
                      id="echo-cancellation-toggle"
                      checked={settings.echoCancellationEnabled ?? true}
                      onChange={(e) => applyImmediateSettings({ echoCancellationEnabled: e.target.checked })}
                    />
                    <label htmlFor="echo-cancellation-toggle" style={{ fontSize: typography.sm, color: colors.text, cursor: "pointer" }}>
                      Enabled
                    </label>
                  </div>
                  <span style={{ fontSize: typography.sm, color: colors.textMuted, marginTop: 4, display: "block" }}>
                    Reduce speaker playback leaking back into the microphone when you are using speakers
                  </span>
                </div>
              </>
            ) : settings.sttProvider === "omni-asr" ? (
              <>
                <LabeledSelect
                  styles={styles}
                  label="Omni-ASR Model"
                  value={settings.omniAsrModel}
                  options={omniAsrModelOptions}
                  onChange={(v) => applyImmediateSettings({ omniAsrModel: v })}
                  hint={omniAsrModelOptions.find((o) => o.value === settings.omniAsrModel)?.description}
                />
                <LabeledSelect
                  styles={styles}
                  label="Device"
                  value={settings.omniAsrDevice}
                  options={omniAsrDeviceOptions}
                  onChange={(v) => applyImmediateSettings({ omniAsrDevice: v })}
                />
              </>
            ) : settings.sttProvider === "cohere-transcribe" ? (
              <>
                <LabeledSelect
                  styles={styles}
                  label="Cohere Model"
                  value={settings.cohereTranscribeModel}
                  options={cohereTranscribeModelOptions}
                  onChange={(v) => applyImmediateSettings({ cohereTranscribeModel: v })}
                  hint={cohereTranscribeModelOptions.find((o) => o.value === settings.cohereTranscribeModel)?.description}
                />
                <LabeledSelect
                  styles={styles}
                  label="Device"
                  value={settings.cohereTranscribeDevice}
                  options={cohereTranscribeDeviceOptions}
                  onChange={(v) => applyImmediateSettings({ cohereTranscribeDevice: v })}
                  hint={
                    settings.cohereTranscribeDevice === "rocm-windows"
                      ? "Experimental AMD ROCm path on native Windows. Currently intended for Radeon 7900-class GPUs with Python 3.12."
                      : settings.cohereTranscribeDevice === "wsl-rocm"
                      ? "Uses ROCm through WSL on Windows. This is the more conservative AMD GPU path."
                      : "Auto and CPU remain the most compatible options if GPU runtime setup fails."
                  }
                />
                <div style={styles.fieldWrap}>
                  <label style={styles.labelStyle}>Hugging Face Token</label>
                  <input
                    type="password"
                    value={huggingFaceTokenDraft}
                    onChange={(e) => setHuggingFaceTokenDraft(e.target.value)}
                    onBlur={() => {
                      if (huggingFaceTokenDraft !== (apiKeys.huggingFaceToken || "")) {
                        void saveHuggingFaceToken();
                      }
                    }}
                    onKeyDown={(e) => handleTextFieldCommitKeyDown(e, saveHuggingFaceToken)}
                    style={styles.inputStyle}
                    placeholder="hf_..."
                  />
                  <div style={{ display: "flex", alignItems: "center", gap: spacing[2], marginTop: spacing[1] }}>
                    <button style={styles.button} onClick={() => void saveHuggingFaceToken()}>
                      Save token
                    </button>
                    <span style={{ fontSize: typography.sm, color: colors.textMuted }}>
                      Required for gated local model download.
                    </span>
                  </div>
                </div>
                <div style={styles.fieldWrap}>
                  <label style={styles.labelStyle}>Speaker diarization</label>
                  <span style={{ fontSize: typography.sm, color: colors.textMuted, display: "block" }}>
                    Cohere Transcribe does not provide diarization in this integration.
                  </span>
                </div>
              </>
            ) : null}

            {/* WSL2 prerequisite banner — Windows + omni-asr only */}
            {wsl2Status && (
              <div style={bannerStyle(wsl2Status.ok)}>
                <div style={{ display: "flex", alignItems: "flex-start", gap: spacing[2] }}>
                  <span style={{ fontSize: 18 }}>{wsl2Status.ok ? "✅" : "⚠️"}</span>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: typography.base, fontWeight: 600, color: colors.text, marginBottom: 4 }}>
                      {wsl2Status.ok ? "WSL2 Ready" : "WSL2 Required for Omni-ASR on Windows"}
                    </div>
                    <div style={{ fontSize: typography.sm, color: colors.textMuted, lineHeight: 1.5 }}>
                      {wsl2Status.message}
                    </div>
                    {!wsl2Status.ok && (
                      <div style={{ marginTop: spacing[2], display: "flex", flexDirection: "column", gap: spacing[1] }}>
                        <div style={{ fontSize: typography.sm, color: colors.text, fontWeight: 500 }}>Setup steps:</div>
                        <ol style={{ margin: 0, paddingLeft: 20, fontSize: typography.sm, color: colors.textMuted, lineHeight: 1.8 }}>
                          <li>Open PowerShell as Administrator and run: <code style={{ background: colors.surface, padding: "1px 4px", borderRadius: 3 }}>wsl --install</code></li>
                          <li>After reboot, open the WSL terminal (Ubuntu) and run:<br/>
                            <code style={{ background: colors.surface, padding: "1px 4px", borderRadius: 3 }}>sudo apt update && sudo apt install -y python3 python3-venv python3-pip</code><br/>
                            <span style={{ fontSize: typography.xs, color: colors.textMuted }}>On Ubuntu 24.04 with Python 3.12, use: <code style={{ background: colors.surface, padding: "1px 4px", borderRadius: 3 }}>python3.12-venv</code> instead of <code style={{ background: colors.surface, padding: "1px 4px", borderRadius: 3 }}>python3-venv</code></span>
                          </li>
                          <li>Then click <strong>Set up Omni-ASR</strong> below.</li>
                        </ol>
                        <a
                          href="https://learn.microsoft.com/en-us/windows/wsl/install"
                          target="_blank"
                          rel="noopener noreferrer"
                          style={{ fontSize: typography.sm, color: colors.accent, marginTop: spacing[1] }}
                        >
                          📖 WSL2 Installation Guide →
                        </a>
                      </div>
                    )}
                  </div>
                </div>
              </div>
            )}

            {/* Python prerequisite banner — faster-whisper and parakeet */}
            {pythonStatus && (
              <div style={bannerStyle(pythonStatus.ok)}>
                <div style={{ display: "flex", alignItems: "flex-start", gap: spacing[2] }}>
                  <span style={{ fontSize: 18 }}>{pythonStatus.ok ? "✅" : "⚠️"}</span>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: typography.base, fontWeight: 600, color: colors.text, marginBottom: 4 }}>
                      {pythonStatus.ok ? "Python 3 Found" : "Python 3 Required"}
                    </div>
                    <div style={{ fontSize: typography.sm, color: colors.textMuted, lineHeight: 1.5 }}>
                      {pythonStatus.message}
                    </div>
                    {!pythonStatus.ok && (
                      <div style={{ marginTop: spacing[2] }}>
                        <a
                          href="https://python.org/downloads/"
                          target="_blank"
                          rel="noopener noreferrer"
                          style={{ fontSize: typography.sm, color: colors.accent }}
                        >
                          📖 Download Python 3 →
                        </a>
                        {isWindows && (
                          <span style={{ fontSize: typography.sm, color: colors.textMuted, marginLeft: spacing[2] }}>
                            (check "Add Python to PATH" during installation)
                          </span>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              </div>
            )}

            {settings.sttProvider === "llama-server" && (
              <div style={bannerStyle(!!llamaStatus?.ready)}>
                <div style={{ display: "flex", alignItems: "flex-start", gap: spacing[2] }}>
                  <span style={{ fontSize: 18 }}>{llamaStatus?.ready ? "✅" : "⬇️"}</span>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: typography.base, fontWeight: 600, color: colors.text, marginBottom: 4 }}>
                      {llamaStatus?.ready ? "Gemma 4 (llama-server) Ready" : "One-Click Setup Available"}
                    </div>
                    <div style={{ fontSize: typography.sm, color: colors.textMuted, lineHeight: 1.5 }}>
                      {llamaStatus?.message ??
                        "Downloads the llama.cpp runtime plus Gemma 4 12B Q4_K_M and its audio projector (~7.6 GB total). Existing LM Studio or Hugging Face model files are reused automatically."}
                    </div>
                    {llamaStatus?.modelSource && llamaStatus.modelSource !== "managed" && (
                      <div style={{ fontSize: typography.sm, color: colors.textMuted, marginTop: 4 }}>
                        Reusing model files from {llamaStatus.modelSource}.
                      </div>
                    )}
                  </div>
                </div>
              </div>
            )}

            {settings.sttProvider === "cohere-transcribe" && (
              <div style={bannerStyle(!!apiKeys.huggingFaceToken)}>
                <div style={{ display: "flex", alignItems: "flex-start", gap: spacing[2] }}>
                  <span style={{ fontSize: 18 }}>{apiKeys.huggingFaceToken ? "✅" : "⚠️"}</span>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: typography.base, fontWeight: 600, color: colors.text, marginBottom: 4 }}>
                      {apiKeys.huggingFaceToken ? "Hugging Face Token Saved" : "Hugging Face Token Required"}
                    </div>
                    <div style={{ fontSize: typography.sm, color: colors.textMuted, lineHeight: 1.5 }}>
                      {apiKeys.huggingFaceToken
                        ? "ContextCall can use this token during local Cohere Transcribe setup."
                        : "Save a Hugging Face token here and make sure your account has accepted access to CohereLabs/cohere-transcribe-03-2026."}
                    </div>
                  </div>
                </div>
              </div>
            )}

            {sttStatus && (
              <div style={{ marginTop: spacing[2], display: "flex", flexDirection: "column", gap: spacing[2] }}>
                <span style={styles.statusBadge(sttStatus.ready ? (sttStatus.usingFallback ? "warning" : "success") : "error")}>
                  <span>{sttStatus.message}</span>
                </span>
                <span style={{ fontSize: typography.sm, color: colors.textMuted }}>
                  Active backend: `{sttStatus.effectiveProvider}` using `{sttStatus.effectiveModel}`.
                </span>
                {(settings.sttProvider === "faster-whisper" || settings.sttProvider === "parakeet" || settings.sttProvider === "omni-asr" || settings.sttProvider === "cohere-transcribe" || settings.sttProvider === "llama-server") && (
                  <button style={styles.button} onClick={() => onSetupStt?.()} disabled={isSettingUpStt}>
                    {isSettingUpStt
                      ? `Setting up ${settings.sttProvider}...`
                      : sttStatus.selectedProviderReady
                      ? `Reinstall ${settings.sttProvider}`
                      : `Set up ${settings.sttProvider}`}
                  </button>
                )}
              </div>
            )}
          </>
        )}
      </div>

      <div style={styles.divider} />

      {/* Reset Section */}
      <div style={styles.section}>
        <h4 style={styles.sectionTitle}>Reset</h4>
        <button
          style={{ ...styles.buttonSecondary, color: colors.error }}
          onClick={() => {
            if (confirm("Reset all settings to defaults? This cannot be undone.")) {
              // Reset logic would go here
            }
          }}
        >
          Reset to Defaults
        </button>
      </div>
    </div>
  );
}
