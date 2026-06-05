// First-run guided setup (EP0007 / US0033).
// Minimal essentials — mic + STT engine — then start. Everything else defers
// to smart defaults (US0031). Shown once; completion persists via
// AppSettings.hasCompletedOnboarding.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { colors, typography, spacing, radius } from "../theme";
import type { AppSettings } from "../types";
import { sttProviderOptions } from "./settings/settingsOptions";

interface FirstRunOnboardingProps {
  settings: AppSettings;
  /** Finish with the chosen mic + engine applied. */
  onComplete: (updated: AppSettings) => void;
  /** Skip — keep smart defaults, just mark onboarding done. */
  onSkip: (updated: AppSettings) => void;
}

export function FirstRunOnboarding({ settings, onComplete, onSkip }: FirstRunOnboardingProps) {
  const [micDevices, setMicDevices] = useState<string[]>([]);
  const [mic, setMic] = useState<string>(settings.inputDeviceName ?? "default");
  const [provider, setProvider] = useState<string>(settings.sttProvider);

  useEffect(() => {
    invoke<string[]>("list_mic_devices")
      .then(setMicDevices)
      .catch(() => setMicDevices([]));
  }, []);

  const finish = (withChoices: boolean) => {
    const updated: AppSettings = {
      ...settings,
      hasCompletedOnboarding: true,
      ...(withChoices
        ? { inputDeviceName: mic === "default" ? null : mic, sttProvider: provider }
        : {}),
    };
    (withChoices ? onComplete : onSkip)(updated);
  };

  const overlay: React.CSSProperties = {
    position: "fixed",
    inset: 0,
    background: colors.background,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    padding: spacing[5],
    zIndex: 1000,
  };
  const card: React.CSSProperties = {
    width: "100%",
    maxWidth: 460,
    background: colors.surface,
    border: `1px solid ${colors.border}`,
    borderRadius: radius.lg,
    padding: spacing[6],
    display: "flex",
    flexDirection: "column",
    gap: spacing[4],
  };
  const label: React.CSSProperties = {
    display: "block",
    fontSize: typography.base,
    color: colors.textSecondary,
    marginBottom: spacing[1],
    fontWeight: 500,
  };
  const select: React.CSSProperties = {
    width: "100%",
    padding: `${spacing[2]}px`,
    background: colors.surface,
    color: colors.text,
    border: `1px solid ${colors.border}`,
    borderRadius: radius.sm,
    fontSize: typography.md,
    cursor: "pointer",
  };
  const primaryBtn: React.CSSProperties = {
    flex: 1,
    padding: `${spacing[3]}px`,
    background: colors.accent,
    color: colors.textInverse,
    border: "none",
    borderRadius: radius.md,
    fontSize: typography.lg,
    fontWeight: 700,
    cursor: "pointer",
  };
  const skipBtn: React.CSSProperties = {
    padding: `${spacing[3]}px ${spacing[4]}px`,
    background: "transparent",
    color: colors.textSecondary,
    border: `1px solid ${colors.border}`,
    borderRadius: radius.md,
    fontSize: typography.base,
    cursor: "pointer",
  };

  const activeProvider = sttProviderOptions.find((o) => o.value === provider);

  return (
    <div style={overlay}>
      <div style={card}>
        <div>
          <h2 style={{ margin: 0, color: colors.text, fontSize: typography["2xl"], fontWeight: 700 }}>
            Welcome to ContextCall
          </h2>
          <p style={{ margin: `${spacing[1]}px 0 0`, color: colors.textMuted, fontSize: typography.md, lineHeight: 1.5 }}>
            Two quick choices and you are ready for your first call. Everything else uses sensible
            defaults you can change later in Settings.
          </p>
        </div>

        <div>
          <label style={label}>Microphone</label>
          <select style={select} value={mic} onChange={(e) => setMic(e.target.value)}>
            <option value="default">Default microphone</option>
            {micDevices.map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        </div>

        <div>
          <label style={label}>Transcription engine</label>
          <select style={select} value={provider} onChange={(e) => setProvider(e.target.value)}>
            {sttProviderOptions.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          {activeProvider && (
            <span style={{ display: "block", marginTop: 4, fontSize: typography.sm, color: colors.textMuted, lineHeight: 1.4 }}>
              {activeProvider.description}
            </span>
          )}
        </div>

        <div style={{ display: "flex", gap: spacing[2], alignItems: "center" }}>
          <button style={primaryBtn} onClick={() => finish(true)}>
            Start using ContextCall
          </button>
          <button style={skipBtn} onClick={() => finish(false)}>
            Skip
          </button>
        </div>
        <span style={{ fontSize: typography.sm, color: colors.textMuted, textAlign: "center" }}>
          Local-first by default — your audio stays on this device.
        </span>
      </div>
    </div>
  );
}
