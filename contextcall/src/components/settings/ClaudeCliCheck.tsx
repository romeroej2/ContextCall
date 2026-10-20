// Claude CLI command field + smoke test, shared by the AI and Interview tabs.

import { useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { spacing } from "../../theme";
import type { AppSettings } from "../../types";
import type { SettingsStyles } from "./settingsStyles";
import { FieldHint } from "./fields";

interface ClaudeCliCheckProps {
  settings: AppSettings;
  styles: SettingsStyles;
  setSettings: Dispatch<SetStateAction<AppSettings | null>>;
  commitSettingsDraft: () => Promise<boolean>;
  /** Model the smoke test runs with. */
  model: string;
}

export function ClaudeCliCheck({ settings, styles, setSettings, commitSettingsDraft, model }: ClaudeCliCheckProps) {
  const [status, setStatus] = useState<{ ok: boolean; message: string } | null>(null);
  const [checking, setChecking] = useState(false);

  const check = async () => {
    setChecking(true);
    try {
      const message = await invoke<string>("check_claude_cli", { path: settings.claudeCliPath, model });
      setStatus({ ok: true, message });
    } catch (err) {
      setStatus({ ok: false, message: String(err) });
    } finally {
      setChecking(false);
    }
  };

  return (
    <div style={styles.fieldWrap}>
      <label style={styles.labelStyle}>Claude CLI Command</label>
      <div style={{ display: "flex", gap: spacing[2], alignItems: "flex-start" }}>
        <input
          type="text"
          value={settings.claudeCliPath}
          onChange={(e) => {
            setStatus(null);
            setSettings({ ...settings, claudeCliPath: e.target.value });
          }}
          onBlur={() => void commitSettingsDraft()}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.nativeEvent.isComposing) void commitSettingsDraft();
          }}
          style={styles.inputStyle}
          placeholder="claude"
        />
        <button style={styles.buttonSecondary} onClick={() => void check()} disabled={checking}>
          {checking ? "Checking..." : "Check"}
        </button>
      </div>
      {status && (
        <div style={{ ...styles.statusBadge(status.ok ? "success" : "error"), marginTop: spacing[2] }}>
          <span>{status.message}</span>
        </div>
      )}
      <FieldHint>
        Uses your own Claude Code login (run <code>claude</code> once in a terminal to sign in). ContextCall never reads
        or stores those credentials.
      </FieldHint>
    </div>
  );
}
