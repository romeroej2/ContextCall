import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { normalizeShortcut, shortcutFromKeyboardEvent } from "../hotkeys";
import type { ApiKeys, AppSettings, CodexUsageSummary, LlamaServerStatus, MeetingTemplate, SttStatus } from "../types";
import { colors, typography, spacing } from "../theme";
import { PromptsView } from "./PromptsView";
import {
  appSettingsEqual,
  apiKeysEqual,
  shouldReindexKnowledgeBase,
  shouldReindexAfterApiKeySave,
} from "./settings/settingsHelpers";
import { createSettingsStyles } from "./settings/settingsStyles";
import { AiProvidersSection } from "./settings/AiProvidersSection";
import { EnginesSection } from "./settings/EnginesSection";
import { KnowledgeSection } from "./settings/KnowledgeSection";
import { AudioSection } from "./settings/AudioSection";
import { PrivacySection } from "./settings/PrivacySection";
import { useSettingsDisclosure } from "./settings/useSettingsDisclosure";
import { searchSettings } from "./settings/settingsSearch";

type Tab = "audio" | "engines" | "ai" | "prompts";

interface SettingsViewProps {
  settings?: AppSettings | null;
  onSettingsChange?: (settings: AppSettings) => void;
  onApiKeysSaved?: () => void;
  sttStatus?: SttStatus | null;
  onSetupStt?: () => void;
  isSettingUpStt?: boolean;
  compact?: boolean;
}

export function SettingsView({
  settings: initialSettings = null,
  onSettingsChange,
  onApiKeysSaved,
  sttStatus = null,
  onSetupStt,
  isSettingUpStt = false,
  compact = false,
}: SettingsViewProps) {
  const [settings, setSettings] = useState<AppSettings | null>(initialSettings);
  const [apiKeys, setApiKeys] = useState<ApiKeys | null>(null);
  const [templates, setTemplates] = useState<MeetingTemplate[]>([]);
  const [activeTab, setActiveTab] = useState<Tab>("audio");
  const [settingsQuery, setSettingsQuery] = useState("");
  const { showAdvanced, setShowAdvanced } = useSettingsDisclosure();
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [kbFileCount, setKbFileCount] = useState<number>(0);
  const [isIndexingKb, setIsIndexingKb] = useState(false);
  const [kbStatus, setKbStatus] = useState<string | null>(null);
  const [isCalibrating, setIsCalibrating] = useState(false);
  const [calibrationCountdown, setCalibrationCountdown] = useState(0);
  const [calibrationLevel, setCalibrationLevel] = useState(0);
  const [calibrationError, setCalibrationError] = useState<string | null>(null);
  const [micDevices, setMicDevices] = useState<string[]>([]);
  const [systemAudioDevices, setSystemAudioDevices] = useState<string[]>([]);
  const [huggingFaceTokenDraft, setHuggingFaceTokenDraft] = useState("");
  const [isRecordingPushToTalkHotkey, setIsRecordingPushToTalkHotkey] = useState(false);
  const [codexCliStatus, setCodexCliStatus] = useState<{ ok: boolean; message: string } | null>(null);
  const [isCheckingCodexCli, setIsCheckingCodexCli] = useState(false);
  const [codexUsage, setCodexUsage] = useState<CodexUsageSummary | null>(null);
  const [llamaStatus, setLlamaStatus] = useState<LlamaServerStatus | null>(null);
  const [isSettingUpLlama, setIsSettingUpLlama] = useState(false);
  const [llamaSetupMessage, setLlamaSetupMessage] = useState<string | null>(null);
  const persistedSettingsRef = useRef<AppSettings | null>(initialSettings);
  const persistedApiKeysRef = useRef<ApiKeys | null>(null);
  const kbSyncTimeoutRef = useRef<number | null>(null);
  // Prerequisite check state
  const [wsl2Status, setWsl2Status] = useState<{ ok: boolean; message: string } | null>(null);
  const [pythonStatus, setPythonStatus] = useState<{ ok: boolean; message: string } | null>(null);
  const isWindows = navigator.userAgent.toLowerCase().includes("windows");
  const isObsidianActive = !!settings?.obsidianVaultPath;
  const hasKbSource = !!(settings?.obsidianVaultPath || settings?.kbFolderPath);


  const countKBFiles = useCallback(async (targetSettings: AppSettings | null = settings) => {
    try {
      if (!targetSettings) {
        setKbFileCount(0);
        return;
      }
      if (targetSettings.obsidianVaultPath) {
        const includeCount = new Set([
          ...targetSettings.obsidianKbIncludePaths.filter(Boolean),
          targetSettings.obsidianNotesFolder,
        ]).size;
        setKbFileCount(includeCount);
        return;
      }
      setKbFileCount(targetSettings.kbFolderPath ? 1 : 0);
    } catch {
      setKbFileCount(0);
    }
  }, [settings]);

  const syncKnowledgeBase = useCallback(async (targetSettings: AppSettings | null = persistedSettingsRef.current) => {
    if (!targetSettings || !(targetSettings.obsidianVaultPath || targetSettings.kbFolderPath)) {
      setKbStatus(null);
      setIsIndexingKb(false);
      return;
    }

    try {
      setIsIndexingKb(true);
      setKbStatus("Indexing knowledge base...");
      const addedChunks = await invoke<number>("index_kb");
      setKbStatus(
        addedChunks > 0
          ? `Knowledge base indexed - ${addedChunks} new or updated chunks`
          : "Knowledge base is ready"
      );
      setError(null);
    } catch (err) {
      setKbStatus("Knowledge base indexing failed");
      setError(String(err));
    } finally {
      setIsIndexingKb(false);
    }
  }, []);

  const scheduleKnowledgeBaseSync = useCallback(
    (targetSettings: AppSettings | null, immediate = false) => {
      if (kbSyncTimeoutRef.current !== null) {
        window.clearTimeout(kbSyncTimeoutRef.current);
        kbSyncTimeoutRef.current = null;
      }

      if (!targetSettings || !(targetSettings.obsidianVaultPath || targetSettings.kbFolderPath)) {
        setKbStatus(null);
        setIsIndexingKb(false);
        return;
      }

      kbSyncTimeoutRef.current = window.setTimeout(() => {
        kbSyncTimeoutRef.current = null;
        void syncKnowledgeBase(targetSettings);
      }, immediate ? 0 : 600);
    },
    [syncKnowledgeBase],
  );

  const runKnowledgeBaseSyncNow = useCallback(() => {
    scheduleKnowledgeBaseSync(persistedSettingsRef.current ?? settings, true);
  }, [scheduleKnowledgeBaseSync, settings]);

  const checkCodexCli = useCallback(async () => {
    if (!settings) {
      return;
    }
    setIsCheckingCodexCli(true);
    try {
      const message = await invoke<string>("check_codex_cli", { path: settings.codexCliPath });
      setCodexCliStatus({ ok: true, message });
      setError(null);
    } catch (err) {
      setCodexCliStatus({ ok: false, message: String(err) });
    } finally {
      setIsCheckingCodexCli(false);
    }
  }, [settings]);

  const refreshCodexUsage = useCallback(async () => {
    try {
      const summary = await invoke<CodexUsageSummary>("get_codex_usage_summary");
      setCodexUsage(summary);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const refreshLlamaStatus = useCallback(async () => {
    try {
      const status = await invoke<LlamaServerStatus>("get_llama_server_status");
      setLlamaStatus(status);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const setupLlamaServer = useCallback(async () => {
    setIsSettingUpLlama(true);
    setLlamaSetupMessage("Starting Gemma 4 (llama-server) setup...");
    try {
      await invoke("setup_llama_server");
      setLlamaSetupMessage(null);
      setError(null);
    } catch (err) {
      setLlamaSetupMessage(null);
      setError(String(err));
    } finally {
      setIsSettingUpLlama(false);
      void refreshLlamaStatus();
    }
  }, [refreshLlamaStatus]);

  // Live setup progress for the AI-tab Gemma setup button.
  useEffect(() => {
    if (!isSettingUpLlama) return;
    const unlisten = listen<{ stage: string; message: string }>("stt-setup-status", (e) => {
      setLlamaSetupMessage(e.payload.message);
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [isSettingUpLlama]);

  const flashSaved = useCallback(() => {
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  }, []);

  const persistSettings = useCallback(
    async (
      updated: AppSettings,
      options?: {
        reindexKnowledgeBase?: boolean;
      },
    ) => {
      try {
        await invoke("save_settings", { newSettings: updated });
        persistedSettingsRef.current = updated;
        onSettingsChange?.(updated);
        setError(null);
        flashSaved();
        if (options?.reindexKnowledgeBase) {
          scheduleKnowledgeBaseSync(updated);
        }
        return true;
      } catch (err) {
        setError(String(err));
        return false;
      }
    },
    [flashSaved, onSettingsChange, scheduleKnowledgeBaseSync],
  );

  const commitSettingsDraft = useCallback(async () => {
    if (!settings) {
      return false;
    }

    const previous = persistedSettingsRef.current;
    if (appSettingsEqual(settings, previous)) {
      return true;
    }

    return persistSettings(settings, {
      reindexKnowledgeBase: previous ? shouldReindexKnowledgeBase(previous, settings) : false,
    });
  }, [persistSettings, settings]);

  const applyImmediateSettings = useCallback(
    async (update: Partial<AppSettings>) => {
      if (!settings) {
        return false;
      }

      const previousDraft = settings;
      const previousPersisted = persistedSettingsRef.current ?? settings;
      const nextDraft = { ...previousDraft, ...update };
      const nextPersisted = { ...previousPersisted, ...update };
      setSettings(nextDraft);

      const saved = await persistSettings(nextPersisted, {
        reindexKnowledgeBase: shouldReindexKnowledgeBase(previousPersisted, nextPersisted),
      });

      if (!saved) {
        setSettings(previousDraft);
      }

      return saved;
    },
    [persistSettings, settings],
  );

  const persistApiKeys = useCallback(
    async (
      updated: ApiKeys,
      options?: {
        reindexKnowledgeBase?: boolean;
      },
    ) => {
      try {
        await invoke("save_api_keys", { newKeys: updated });
        persistedApiKeysRef.current = updated;
        onApiKeysSaved?.();
        setError(null);
        flashSaved();
        if (options?.reindexKnowledgeBase) {
          scheduleKnowledgeBaseSync(persistedSettingsRef.current);
        }
        return true;
      } catch (err) {
        setError(String(err));
        return false;
      }
    },
    [flashSaved, onApiKeysSaved, scheduleKnowledgeBaseSync],
  );

  const commitApiKeysDraft = useCallback(async () => {
    if (!apiKeys) {
      return false;
    }

    const previous = persistedApiKeysRef.current;
    if (apiKeysEqual(apiKeys, previous)) {
      return true;
    }

    return persistApiKeys(apiKeys, {
      reindexKnowledgeBase: previous
        ? shouldReindexAfterApiKeySave(persistedSettingsRef.current, previous, apiKeys)
        : false,
    });
  }, [apiKeys, persistApiKeys]);

  const handleTextFieldCommitKeyDown = useCallback(
    (
      event: React.KeyboardEvent<HTMLInputElement>,
      commit: () => Promise<unknown>,
    ) => {
      if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) {
        return;
      }

      event.preventDefault();
      void commit();
    },
    [],
  );

  useEffect(() => {
    invoke<ApiKeys>("get_api_keys")
      .then((keys) => {
        setApiKeys(keys);
        persistedApiKeysRef.current = keys;
        setHuggingFaceTokenDraft(keys.huggingFaceToken || "");
      })
      .catch((err) => setError(String(err)));
  }, []);

  useEffect(() => {
    invoke<MeetingTemplate[]>("list_templates")
      .then(setTemplates)
      .catch((err) => setError(String(err)));
  }, []);

  useEffect(() => {
    Promise.allSettled([
      invoke<string[]>("list_mic_devices"),
      invoke<string[]>("list_sys_audio_devices"),
    ]).then(([micsResult, systemResult]) => {
      if (micsResult.status === "fulfilled") {
        setMicDevices(micsResult.value);
      }
      if (systemResult.status === "fulfilled") {
        setSystemAudioDevices(systemResult.value);
      }
    });
  }, []);

  // Check prerequisites when the Advanced tab becomes active or the provider changes
  useEffect(() => {
    if (!settings || activeTab !== "engines") return;
    const provider = settings.sttProvider;
    if (provider === "omni-asr" && isWindows) {
      invoke<string>("check_wsl2")
        .then(() => setWsl2Status({ ok: true, message: "WSL2 is available and Python 3 is installed inside it." }))
        .catch((err: string) => setWsl2Status({ ok: false, message: err }));
    } else {
      setWsl2Status(null);
    }
    if (
      provider === "faster-whisper" ||
      provider === "parakeet" ||
      provider === "cohere-transcribe"
    ) {
      invoke<string>("check_python")
        .then((cmd) => setPythonStatus({ ok: true, message: `Python 3 found (${cmd}).` }))
        .catch((err: string) => setPythonStatus({ ok: false, message: err }));
    } else {
      setPythonStatus(null);
    }
  }, [settings, activeTab, isWindows]);

  useEffect(() => {
    if (activeTab === "ai" && settings?.llmProvider === "codex-cli") {
      void refreshCodexUsage();
    }
  }, [activeTab, refreshCodexUsage, settings?.llmProvider]);

  useEffect(() => {
    const llamaSelected =
      settings?.llmProvider === "llama-server" || settings?.sttProvider === "llama-server";
    if ((activeTab === "ai" || activeTab === "engines") && llamaSelected) {
      void refreshLlamaStatus();
    }
  }, [activeTab, refreshLlamaStatus, settings?.llmProvider, settings?.sttProvider]);

  useEffect(() => {
    if (initialSettings) {
      setSettings(initialSettings);
      persistedSettingsRef.current = initialSettings;
      setKbFileCount(
        initialSettings.obsidianVaultPath
          ? new Set([
              ...initialSettings.obsidianKbIncludePaths.filter(Boolean),
              initialSettings.obsidianNotesFolder,
            ]).size
          : initialSettings.kbFolderPath
            ? 1
            : 0,
      );
      return;
    }

    invoke<AppSettings>("get_settings")
      .then((loadedSettings) => {
        setSettings(loadedSettings);
        persistedSettingsRef.current = loadedSettings;
        setKbFileCount(
          loadedSettings.obsidianVaultPath
            ? new Set([
                ...loadedSettings.obsidianKbIncludePaths.filter(Boolean),
                loadedSettings.obsidianNotesFolder,
              ]).size
            : loadedSettings.kbFolderPath
              ? 1
              : 0,
        );
      })
      .catch((err) => setError(String(err)));
  }, [initialSettings]);

  useEffect(() => {
    if (settings?.micCaptureMode === "push-to-talk") {
      return;
    }

    setIsRecordingPushToTalkHotkey(false);
  }, [settings?.micCaptureMode]);

  useEffect(() => {
    return () => {
      if (kbSyncTimeoutRef.current !== null) {
        window.clearTimeout(kbSyncTimeoutRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!isRecordingPushToTalkHotkey || !settings) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();

      if (
        event.key === "Escape" &&
        !event.ctrlKey &&
        !event.altKey &&
        !event.shiftKey &&
        !event.metaKey
      ) {
        setIsRecordingPushToTalkHotkey(false);
        return;
      }

      const captured = shortcutFromKeyboardEvent(event);
      if (!captured) {
        return;
      }

      setIsRecordingPushToTalkHotkey(false);
      void applyImmediateSettings({
        pushToTalkHotkey: normalizeShortcut(captured) || "Space",
      });
    };

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [applyImmediateSettings, isRecordingPushToTalkHotkey, settings]);

  const startCalibration = async () => {
    if (!settings) return;
    setIsCalibrating(true);
    setCalibrationError(null);
    setCalibrationLevel(0);

    const unlisten = await listen<{ level: number }>("calibration-audio-level", (e) => {
      setCalibrationLevel(e.payload.level);
    });

    try {
      await invoke("start_calibration_preview");

      for (let i = 3; i >= 1; i--) {
        setCalibrationCountdown(i);
        await new Promise((r) => setTimeout(r, 1000));
      }
      setCalibrationCountdown(0);

      const rms = await invoke<number>("calibrate_mic_threshold");
      await invoke("stop_calibration_preview").catch(() => {});
      setSettings((current) => (current ? { ...current, micCalibrationRms: rms } : current));
      const persistedSettings = persistedSettingsRef.current ?? settings;
      await persistSettings({ ...persistedSettings, micCalibrationRms: rms });
    } catch (err) {
      setCalibrationError(String(err));
      await invoke("stop_calibration_preview").catch(() => {});
    } finally {
      unlisten();
      setIsCalibrating(false);
      setCalibrationLevel(0);
    }
  };

  const saveHuggingFaceToken = async () => {
    if (!apiKeys) return;
    const updated = { ...apiKeys, huggingFaceToken: huggingFaceTokenDraft };
    setApiKeys(updated);
    const previous = persistedApiKeysRef.current ?? apiKeys;
    const saved = await persistApiKeys(updated);
    if (!saved) {
      setApiKeys(previous);
    }
  };

  const chooseFolder = async (
    key: "kbFolderPath" | "notesFolderPath" | "obsidianVaultPath",
  ) => {
    try {
      const selected = await invoke<string | null>("choose_folder");
      if (selected && settings) {
        const updated = { [key]: selected } as Partial<AppSettings>;
        const saved = await applyImmediateSettings(updated);
        if (key === "kbFolderPath" || key === "obsidianVaultPath") {
          void countKBFiles(saved ? { ...settings, ...updated } : settings);
        }
      }
    } catch (err) {
      console.error("Failed to choose folder:", err);
    }
  };

  const addObsidianIncludePath = async () => {
    if (!settings?.obsidianVaultPath) {
      return;
    }
    try {
      const relativePath = await invoke<string>("choose_obsidian_include_folder", {
        vaultPath: settings.obsidianVaultPath,
      });
      if (!relativePath) {
        return;
      }
      const nextIncludePaths = Array.from(
        new Set([...settings.obsidianKbIncludePaths, relativePath]),
      );
      const nextSettings = { ...settings, obsidianKbIncludePaths: nextIncludePaths };
      const saved = await applyImmediateSettings({
        obsidianKbIncludePaths: nextIncludePaths,
      });
      if (saved) {
        void countKBFiles(nextSettings);
      }
    } catch (err) {
      if (String(err).includes("No folder selected")) {
        return;
      }
      setError(String(err));
    }
  };

  const removeObsidianIncludePath = async (relativePath: string) => {
    if (!settings) {
      return;
    }
    const nextIncludePaths = settings.obsidianKbIncludePaths.filter((value) => value !== relativePath);
    const nextSettings = { ...settings, obsidianKbIncludePaths: nextIncludePaths };
    const saved = await applyImmediateSettings({
      obsidianKbIncludePaths: nextIncludePaths,
    });
    if (saved) {
      void countKBFiles(nextSettings);
    }
  };

  if (!settings || !apiKeys) {
    return (
      <div style={{ padding: spacing[4], color: colors.textMuted }}>
        <div style={{ display: "flex", alignItems: "center", gap: spacing[2] }}>
          <span>Loading settings...</span>
        </div>
      </div>
    );
  }

  const isLocalUrl = (url?: string) => !url || /^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?/.test(url);
  const isCodexCliMode = settings.llmProvider === "codex-cli";
  const isGemmaLocalMode = settings.llmProvider === "llama-server";
  const isLocalMode =
    !isCodexCliMode &&
    !isGemmaLocalMode &&
    ((settings.llmProvider === "ollama" && settings.embeddingProvider === "ollama") ||
      (settings.llmProvider === "openai" && settings.embeddingProvider === "openai" &&
        isLocalUrl(settings.openAiLlmBaseUrl) && isLocalUrl(settings.openAiEmbedBaseUrl)));

  const styles = createSettingsStyles(compact);

  return (
    <div style={styles.container}>
      {!compact && <h3 style={styles.header}>Settings</h3>}

      <div style={{ ...styles.fieldWrap, marginBottom: spacing[3] }}>
        <input
          type="text"
          value={settingsQuery}
          onChange={(e) => setSettingsQuery(e.target.value)}
          placeholder="Search settings..."
          style={styles.inputStyle}
        />
      </div>

      {settingsQuery.trim() ? (
        <div style={styles.section}>
          {searchSettings(settingsQuery).length === 0 ? (
            <span style={{ fontSize: typography.sm, color: colors.textMuted }}>No matching settings.</span>
          ) : (
            searchSettings(settingsQuery).map((entry) => (
              <button
                key={`${entry.tab}:${entry.label}`}
                style={{ ...styles.buttonSecondary, display: "block", width: "100%", textAlign: "left", marginBottom: spacing[1] }}
                onClick={() => {
                  setActiveTab(entry.tab);
                  if (entry.advanced) setShowAdvanced(true);
                  setSettingsQuery("");
                }}
              >
                {entry.label}
                <span style={{ color: colors.textMuted, marginLeft: spacing[2] }}>
                  {entry.tab}{entry.advanced ? " · advanced" : ""}
                </span>
              </button>
            ))
          )}
        </div>
      ) : (
        <>
      {/* Tabs */}
      <div style={styles.tabs}>
        <button
          style={styles.tab(activeTab === "audio")}
          onClick={() => setActiveTab("audio")}
        >
          Audio
        </button>
        <button
          style={styles.tab(activeTab === "engines")}
          onClick={() => setActiveTab("engines")}
        >
          Engines
        </button>
        <button
          style={styles.tab(activeTab === "ai")}
          onClick={() => setActiveTab("ai")}
        >
          AI
        </button>
        <button
          style={styles.tab(activeTab === "prompts")}
          onClick={() => setActiveTab("prompts")}
        >
          Prompts
        </button>
      </div>

      {/* Audio Tab */}
      {activeTab === "audio" && (
        <div>
          <AudioSection
            settings={settings}
            styles={styles}
            micDevices={micDevices}
            systemAudioDevices={systemAudioDevices}
            applyImmediateSettings={applyImmediateSettings}
            isRecordingPushToTalkHotkey={isRecordingPushToTalkHotkey}
            setIsRecordingPushToTalkHotkey={setIsRecordingPushToTalkHotkey}
            isCalibrating={isCalibrating}
            calibrationCountdown={calibrationCountdown}
            calibrationLevel={calibrationLevel}
            calibrationError={calibrationError}
            startCalibration={startCalibration}
          />

          <div style={styles.divider} />

          <PrivacySection
            settings={settings}
            styles={styles}
            applyImmediateSettings={applyImmediateSettings}
          />
        </div>
      )}

      {/* AI Tab */}
      {activeTab === "ai" && (
        <div>
          <AiProvidersSection
            settings={settings}
            apiKeys={apiKeys}
            styles={styles}
            isLocalMode={isLocalMode}
            isGemmaLocalMode={isGemmaLocalMode}
            isCodexCliMode={isCodexCliMode}
            applyImmediateSettings={applyImmediateSettings}
            setSettings={setSettings}
            setApiKeys={setApiKeys}
            commitSettingsDraft={commitSettingsDraft}
            commitApiKeysDraft={commitApiKeysDraft}
            handleTextFieldCommitKeyDown={handleTextFieldCommitKeyDown}
            llamaStatus={llamaStatus}
            llamaSetupMessage={llamaSetupMessage}
            isSettingUpLlama={isSettingUpLlama}
            setupLlamaServer={setupLlamaServer}
            refreshLlamaStatus={refreshLlamaStatus}
            codexCliStatus={codexCliStatus}
            setCodexCliStatus={setCodexCliStatus}
            isCheckingCodexCli={isCheckingCodexCli}
            checkCodexCli={checkCodexCli}
            codexUsage={codexUsage}
            refreshCodexUsage={refreshCodexUsage}
            showAdvanced={showAdvanced}
            onToggleAdvanced={setShowAdvanced}
          />

          <div style={styles.divider} />

          <KnowledgeSection
            settings={settings}
            templates={templates}
            styles={styles}
            kbFileCount={kbFileCount}
            isIndexingKb={isIndexingKb}
            kbStatus={kbStatus}
            isObsidianActive={isObsidianActive}
            hasKbSource={hasKbSource}
            applyImmediateSettings={applyImmediateSettings}
            chooseFolder={chooseFolder}
            addObsidianIncludePath={addObsidianIncludePath}
            removeObsidianIncludePath={removeObsidianIncludePath}
            runKnowledgeBaseSyncNow={runKnowledgeBaseSyncNow}
            setKbFileCount={setKbFileCount}
            setKbStatus={setKbStatus}
          />
        </div>
      )}

      {/* Engines Tab */}
      {activeTab === "engines" && (
        <EnginesSection
          settings={settings}
          apiKeys={apiKeys}
          styles={styles}
          applyImmediateSettings={applyImmediateSettings}
          handleTextFieldCommitKeyDown={handleTextFieldCommitKeyDown}
          huggingFaceTokenDraft={huggingFaceTokenDraft}
          setHuggingFaceTokenDraft={setHuggingFaceTokenDraft}
          saveHuggingFaceToken={saveHuggingFaceToken}
          wsl2Status={wsl2Status}
          pythonStatus={pythonStatus}
          llamaStatus={llamaStatus}
          isWindows={isWindows}
          sttStatus={sttStatus}
          onSetupStt={onSetupStt}
          isSettingUpStt={isSettingUpStt}
          showAdvanced={showAdvanced}
          onToggleAdvanced={setShowAdvanced}
        />
      )}

      {/* Prompts Tab */}
      {activeTab === "prompts" && <PromptsView />}
        </>
      )}

      {/* Status Messages */}
      {error && (
        <div style={{ ...styles.statusBadge("error"), marginTop: spacing[4] }}>
          <span>⚠️</span>
          <span>{error}</span>
        </div>
      )}
      {saved && (
        <div style={{ ...styles.statusBadge("success"), marginTop: spacing[4] }}>
          <span>✓</span>
          <span>Saved</span>
        </div>
      )}
    </div>
  );
}
