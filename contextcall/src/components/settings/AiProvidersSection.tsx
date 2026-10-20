// AI Providers settings section (LLM mode, language model, embeddings).
// Extracted from SettingsView.tsx (EP0007 / US0029).

import type { Dispatch, SetStateAction } from "react";
import { colors, typography, spacing } from "../../theme";
import type { ApiKeys, AppSettings, CodexUsageSummary, LlamaServerStatus } from "../../types";
import type { SettingsStyles } from "./settingsStyles";
import { AdvancedToggle } from "./AdvancedToggle";
import { LabeledTextField } from "./fields";
import { ClaudeCliCheck } from "./ClaudeCliCheck";

interface AiProvidersSectionProps {
  settings: AppSettings;
  apiKeys: ApiKeys;
  styles: SettingsStyles;
  isLocalMode: boolean;
  isGemmaLocalMode: boolean;
  isCodexCliMode: boolean;
  applyImmediateSettings: (update: Partial<AppSettings>) => Promise<boolean>;
  setSettings: Dispatch<SetStateAction<AppSettings | null>>;
  setApiKeys: Dispatch<SetStateAction<ApiKeys | null>>;
  commitSettingsDraft: () => Promise<boolean>;
  commitApiKeysDraft: () => Promise<boolean>;
  handleTextFieldCommitKeyDown: (
    event: React.KeyboardEvent<HTMLInputElement>,
    commit: () => Promise<unknown>,
  ) => void;
  llamaStatus: LlamaServerStatus | null;
  llamaSetupMessage: string | null;
  isSettingUpLlama: boolean;
  setupLlamaServer: () => Promise<void>;
  refreshLlamaStatus: () => Promise<void>;
  codexCliStatus: { ok: boolean; message: string } | null;
  setCodexCliStatus: Dispatch<SetStateAction<{ ok: boolean; message: string } | null>>;
  isCheckingCodexCli: boolean;
  checkCodexCli: () => Promise<void>;
  codexUsage: CodexUsageSummary | null;
  refreshCodexUsage: () => Promise<void>;
  showAdvanced: boolean;
  onToggleAdvanced: (value: boolean) => void;
}

export function AiProvidersSection({
  settings,
  apiKeys,
  styles,
  isLocalMode,
  isGemmaLocalMode,
  isCodexCliMode,
  applyImmediateSettings,
  setSettings,
  setApiKeys,
  commitSettingsDraft,
  commitApiKeysDraft,
  handleTextFieldCommitKeyDown,
  llamaStatus,
  llamaSetupMessage,
  isSettingUpLlama,
  setupLlamaServer,
  refreshLlamaStatus,
  codexCliStatus,
  setCodexCliStatus,
  isCheckingCodexCli,
  checkCodexCli,
  codexUsage,
  refreshCodexUsage,
  showAdvanced,
  onToggleAdvanced,
}: AiProvidersSectionProps) {
  const isClaudeCliMode = settings.llmProvider === "claude-cli";
  return (
    <div>
      {/* Mode Selection */}
      <div style={styles.section}>
        <h4 style={styles.sectionTitle}>AI Mode</h4>
        <p style={styles.sectionDescription}>
          Choose how ContextCall processes your data.
        </p>
        <div style={{ display: "flex", flexDirection: "column", gap: spacing[3] }}>
          <div
            style={styles.aiModeCard(isLocalMode)}
            onClick={() =>
              applyImmediateSettings({
                llmProvider: "ollama",
                embeddingProvider: "ollama",
              })
            }
          >
            <div style={styles.aiModeTitle}>🔒 Local Mode</div>
            <div style={styles.aiModeDesc}>
              Everything runs on your machine. Requires Ollama running locally. Maximum privacy, no data leaves your device.
            </div>
          </div>
          <div
            style={styles.aiModeCard(isGemmaLocalMode)}
            onClick={() =>
              applyImmediateSettings({
                llmProvider: "llama-server",
              })
            }
          >
            <div style={styles.aiModeTitle}>⚡ Gemma Local (built-in)</div>
            <div style={styles.aiModeDesc}>
              Self-hosted Gemma 4 12B via a bundled llama.cpp server — no Ollama, no Python, no API keys. One-click setup; can also transcribe speech. Embeddings stay on the provider configured below.
            </div>
          </div>
          <div
            style={styles.aiModeCard(isCodexCliMode)}
            onClick={() =>
              applyImmediateSettings({
                llmProvider: "codex-cli",
              })
            }
          >
            <div style={styles.aiModeTitle}>Codex CLI</div>
            <div style={styles.aiModeDesc}>
              Uses your local Codex CLI login for suggestions and notes. Embeddings stay on the provider configured below.
            </div>
          </div>
          <div
            style={styles.aiModeCard(isClaudeCliMode)}
            onClick={() =>
              applyImmediateSettings({
                llmProvider: "claude-cli",
              })
            }
          >
            <div style={styles.aiModeTitle}>Claude CLI</div>
            <div style={styles.aiModeDesc}>
              Uses your local Claude Code login. Live suggestions share one conversation per session, so each one builds on the last. Embeddings stay on the provider configured below.
            </div>
          </div>
          <div
            style={styles.aiModeCard(!isLocalMode && !isCodexCliMode && !isClaudeCliMode && !isGemmaLocalMode)}
            onClick={() =>
              applyImmediateSettings({
                llmProvider: "openrouter",
                embeddingProvider: "voyage",
              })
            }
          >
            <div style={styles.aiModeTitle}>☁️ Cloud Mode</div>
            <div style={styles.aiModeDesc}>
              Uses cloud providers for best quality. Requires API keys. Transcription stays local, only text snippets are sent to cloud.
            </div>
          </div>
        </div>
      </div>

      <div style={styles.divider} />

      {/* LLM Provider Settings */}
      <div style={styles.section}>
        <h4 style={styles.sectionTitle}>Language Model</h4>

        {settings.llmProvider === "llama-server" ? (
          <>
            <div style={styles.fieldWrap}>
              <div style={{ display: "flex", justifyContent: "space-between", gap: spacing[2], alignItems: "center" }}>
                <label style={styles.labelStyle}>Gemma 4 12B (llama-server)</label>
                <button style={styles.buttonSecondary} onClick={() => void refreshLlamaStatus()}>
                  Refresh
                </button>
              </div>
              {llamaStatus && (
                <div
                  style={{
                    ...styles.statusBadge(llamaStatus.running ? "success" : llamaStatus.ready ? "warning" : "error"),
                    display: "flex",
                    flexDirection: "column",
                    alignItems: "flex-start",
                    gap: spacing[1],
                  }}
                >
                  <span>{llamaStatus.message}</span>
                  {llamaStatus.modelPath && (
                    <span style={{ fontSize: typography.sm }}>
                      Model: {llamaStatus.modelPath}
                      {llamaStatus.modelSource && llamaStatus.modelSource !== "managed"
                        ? ` (reused from ${llamaStatus.modelSource})`
                        : ""}
                    </span>
                  )}
                </div>
              )}
              {llamaSetupMessage && (
                <div style={{ ...styles.statusBadge("warning"), marginTop: spacing[2] }}>
                  <span>{llamaSetupMessage}</span>
                </div>
              )}
              <button
                style={{ ...styles.button, marginTop: spacing[2] }}
                onClick={() => void setupLlamaServer()}
                disabled={isSettingUpLlama}
              >
                {isSettingUpLlama
                  ? "Setting up Gemma 4..."
                  : llamaStatus?.ready
                  ? "Start / verify server"
                  : "Set up Gemma 4 (downloads what's missing)"}
              </button>
              <span style={{ fontSize: typography.sm, color: colors.textMuted, marginTop: 4, display: "block" }}>
                Runs Gemma 4 12B Q4_K_M with its audio projector on llama.cpp. Existing LM Studio or Hugging Face model files are reused automatically; nothing leaves your machine.
              </span>
            </div>
            <div style={styles.fieldWrap}>
              <label style={styles.labelStyle}>Server Port</label>
              <input
                type="number"
                value={settings.llamaServerPort}
                onChange={(e) =>
                  setSettings({ ...settings, llamaServerPort: Number(e.target.value) || 8765 })
                }
                onBlur={() => void commitSettingsDraft()}
                onKeyDown={(e) => handleTextFieldCommitKeyDown(e, commitSettingsDraft)}
                style={styles.inputStyle}
                placeholder="8765"
              />
            </div>
          </>
        ) : isClaudeCliMode ? (
          <>
            <ClaudeCliCheck
              settings={settings}
              styles={styles}
              setSettings={setSettings}
              commitSettingsDraft={commitSettingsDraft}
              model={settings.claudeCliModel}
            />
            <LabeledTextField
              styles={styles}
              label="Model"
              value={settings.claudeCliModel}
              onChange={(v) => setSettings({ ...settings, claudeCliModel: v })}
              onCommit={commitSettingsDraft}
              placeholder="sonnet"
              hint="Alias (sonnet, opus, haiku) or a full model id."
            />
          </>
        ) : settings.llmProvider === "codex-cli" ? (
          <>
            <div style={styles.fieldWrap}>
              <label style={styles.labelStyle}>Codex CLI Command</label>
              <div style={{ display: "flex", gap: spacing[2], alignItems: "flex-start" }}>
                <input
                  type="text"
                  value={settings.codexCliPath}
                  onChange={(e) => {
                    setCodexCliStatus(null);
                    setSettings({ ...settings, codexCliPath: e.target.value });
                  }}
                  onBlur={() => void commitSettingsDraft()}
                  onKeyDown={(e) => handleTextFieldCommitKeyDown(e, commitSettingsDraft)}
                  style={styles.inputStyle}
                  placeholder="codex"
                />
                <button
                  style={styles.buttonSecondary}
                  onClick={() => void checkCodexCli()}
                  disabled={isCheckingCodexCli}
                >
                  {isCheckingCodexCli ? "Checking..." : "Check"}
                </button>
              </div>
              {codexCliStatus && (
                <div
                  style={{
                    ...styles.statusBadge(codexCliStatus.ok ? "success" : "error"),
                    marginTop: spacing[2],
                  }}
                >
                  <span>{codexCliStatus.message}</span>
                </div>
              )}
            </div>
            <LabeledTextField
              styles={styles}
              label="Model Override (optional)"
              value={settings.codexCliModel}
              onChange={(v) => setSettings({ ...settings, codexCliModel: v })}
              onCommit={commitSettingsDraft}
              placeholder="Leave blank for Codex CLI default"
            />
            <div style={styles.fieldWrap}>
              <label style={styles.labelStyle}>Fallback Provider</label>
              <select
                value={settings.codexCliFallbackProvider}
                onChange={(e) =>
                  applyImmediateSettings({ codexCliFallbackProvider: e.target.value })
                }
                style={styles.selectStyle}
              >
                <option value="disabled">Disabled - show Codex errors</option>
                <option value="openai">OpenAI-compatible</option>
                <option value="ollama">Ollama</option>
                <option value="openrouter">OpenRouter</option>
              </select>
              <span style={{ fontSize: typography.sm, color: colors.textMuted, marginTop: 4, display: "block" }}>
                Fallback is only used after a Codex CLI failure, timeout, or unusable response.
              </span>
            </div>
            <div style={styles.fieldWrap}>
              <div style={{ display: "flex", justifyContent: "space-between", gap: spacing[2], alignItems: "center" }}>
                <label style={styles.labelStyle}>Usage</label>
                <button style={styles.buttonSecondary} onClick={() => void refreshCodexUsage()}>
                  Refresh
                </button>
              </div>
              <div style={{ ...styles.statusBadge("success"), display: "flex", flexDirection: "column", alignItems: "flex-start", gap: spacing[1] }}>
                <span>
                  Requests: {(codexUsage?.totalRequests ?? 0).toLocaleString()} total, {(codexUsage?.successfulRequests ?? 0).toLocaleString()} successful, {(codexUsage?.fallbackRequests ?? 0).toLocaleString()} fallback
                </span>
                <span>
                  Tokens: {(codexUsage?.totalTokens ?? 0).toLocaleString()} total ({(codexUsage?.inputTokens ?? 0).toLocaleString()} input, {(codexUsage?.outputTokens ?? 0).toLocaleString()} output)
                </span>
                <span>
                  Cost: {codexUsage?.estimatedMeteredCostUsd == null ? "included in ChatGPT/Codex plan" : `$${codexUsage.estimatedMeteredCostUsd.toFixed(4)}`}
                </span>
                {(codexUsage?.unknownTokenRequests ?? 0) > 0 && (
                  <span>
                    Token counts unavailable for {codexUsage?.unknownTokenRequests.toLocaleString()} request(s).
                  </span>
                )}
              </div>
              <span style={{ fontSize: typography.sm, color: colors.textMuted, marginTop: 4, display: "block" }}>
                Ledger: {codexUsage?.ledgerPath ?? "No usage recorded yet."}
              </span>
            </div>
          </>
        ) : settings.llmProvider === "openrouter" ? (
          <>
            <LabeledTextField
              styles={styles}
              label="OpenRouter API Key"
              type="password"
              value={apiKeys.openRouterApiKey}
              onChange={(v) => setApiKeys({ ...apiKeys, openRouterApiKey: v })}
              onCommit={commitApiKeysDraft}
              placeholder="sk-or-..."
            />
            <LabeledTextField
              styles={styles}
              label="Model"
              value={settings.selectedModel}
              onChange={(v) => setSettings({ ...settings, selectedModel: v })}
              onCommit={commitSettingsDraft}
              placeholder="e.g. google/gemini-2.5-flash-preview"
              hint="Popular: google/gemini-2.5-flash, anthropic/claude-3.5-sonnet, openai/gpt-4o"
            />
          </>
        ) : settings.llmProvider === "ollama" ? (
          <>
            <LabeledTextField
              styles={styles}
              label="Ollama Base URL"
              value={settings.ollamaBaseUrl}
              onChange={(v) => setSettings({ ...settings, ollamaBaseUrl: v })}
              onCommit={commitSettingsDraft}
              placeholder="http://127.0.0.1:11434"
            />
            <LabeledTextField
              styles={styles}
              label="Model"
              value={settings.ollamaLlmModel}
              onChange={(v) => setSettings({ ...settings, ollamaLlmModel: v })}
              onCommit={commitSettingsDraft}
              placeholder="e.g. qwen3:8b, llama3.2:3b"
            />
          </>
        ) : (
          <>
            <LabeledTextField
              styles={styles}
              label="Base URL"
              value={settings.openAiLlmBaseUrl}
              onChange={(v) => setSettings({ ...settings, openAiLlmBaseUrl: v })}
              onCommit={commitSettingsDraft}
              placeholder="http://127.0.0.1:1234"
            />
            <LabeledTextField
              styles={styles}
              label="API Key (optional)"
              type="password"
              value={apiKeys.openAiLlmApiKey}
              onChange={(v) => setApiKeys({ ...apiKeys, openAiLlmApiKey: v })}
              onCommit={commitApiKeysDraft}
            />
            <LabeledTextField
              styles={styles}
              label="Model"
              value={settings.selectedModel}
              onChange={(v) => setSettings({ ...settings, selectedModel: v })}
              onCommit={commitSettingsDraft}
              placeholder="e.g. gpt-4o-mini"
            />
          </>
        )}
      </div>

      <AdvancedToggle checked={showAdvanced} onChange={onToggleAdvanced} styles={styles} />
      {showAdvanced && (
        <>
      <div style={styles.divider} />

      {/* Embedding Provider Settings */}
      <div style={styles.section}>
        <h4 style={styles.sectionTitle}>Embeddings</h4>
        <p style={styles.sectionDescription}>
          Used for knowledge base search.
        </p>

        {settings.embeddingProvider === "voyage" ? (
          <>
            <LabeledTextField
              styles={styles}
              label="Voyage AI API Key"
              type="password"
              value={apiKeys.voyageApiKey}
              onChange={(v) => setApiKeys({ ...apiKeys, voyageApiKey: v })}
              onCommit={commitApiKeysDraft}
              placeholder="pa-..."
            />
            <div style={{ ...styles.statusBadge("warning"), marginTop: spacing[2] }}>
              <span>⚡</span>
              <span>Uses voyage-3-lite model</span>
            </div>
          </>
        ) : settings.embeddingProvider === "ollama" ? (
          <>
            <LabeledTextField
              styles={styles}
              label="Ollama Base URL"
              value={settings.ollamaBaseUrl}
              onChange={(v) => setSettings({ ...settings, ollamaBaseUrl: v })}
              onCommit={commitSettingsDraft}
              placeholder="http://127.0.0.1:11434"
            />
            <LabeledTextField
              styles={styles}
              label="Embedding Model"
              value={settings.ollamaEmbedModel}
              onChange={(v) => setSettings({ ...settings, ollamaEmbedModel: v })}
              onCommit={commitSettingsDraft}
              placeholder="e.g. nomic-embed-text"
            />
          </>
        ) : (
          <>
            <LabeledTextField
              styles={styles}
              label="Base URL"
              value={settings.openAiEmbedBaseUrl}
              onChange={(v) => setSettings({ ...settings, openAiEmbedBaseUrl: v })}
              onCommit={commitSettingsDraft}
              placeholder="http://127.0.0.1:8080"
            />
            <LabeledTextField
              styles={styles}
              label="API Key (optional)"
              type="password"
              value={apiKeys.openAiEmbedApiKey}
              onChange={(v) => setApiKeys({ ...apiKeys, openAiEmbedApiKey: v })}
              onCommit={commitApiKeysDraft}
            />
            <LabeledTextField
              styles={styles}
              label="Model"
              value={settings.openAiEmbedModel}
              onChange={(v) => setSettings({ ...settings, openAiEmbedModel: v })}
              onCommit={commitSettingsDraft}
              placeholder="e.g. text-embedding-3-small"
            />
          </>
        )}
      </div>
        </>
      )}
    </div>
  );
}
