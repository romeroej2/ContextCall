// Audio & Capture settings section (devices, capture mode, PTT, mic calibration).
// Extracted from SettingsView.tsx (EP0007 / US0029).

import type { Dispatch, SetStateAction } from "react";
import { colors, typography, spacing } from "../../theme";
import { normalizeShortcut } from "../../hotkeys";
import type { AppSettings } from "../../types";
import { WaveformVisualizer } from "../WaveformVisualizer";
import type { SettingsStyles } from "./settingsStyles";

interface AudioSectionProps {
  settings: AppSettings;
  styles: SettingsStyles;
  micDevices: string[];
  systemAudioDevices: string[];
  applyImmediateSettings: (update: Partial<AppSettings>) => Promise<boolean>;
  isRecordingPushToTalkHotkey: boolean;
  setIsRecordingPushToTalkHotkey: Dispatch<SetStateAction<boolean>>;
  isCalibrating: boolean;
  calibrationCountdown: number;
  calibrationLevel: number;
  calibrationError: string | null;
  startCalibration: () => Promise<void>;
}

export function AudioSection({
  settings,
  styles,
  micDevices,
  systemAudioDevices,
  applyImmediateSettings,
  isRecordingPushToTalkHotkey,
  setIsRecordingPushToTalkHotkey,
  isCalibrating,
  calibrationCountdown,
  calibrationLevel,
  calibrationError,
  startCalibration,
}: AudioSectionProps) {
  return (
    <div style={styles.section}>
      <h4 style={styles.sectionTitle}>Audio &amp; Capture</h4>
      <p style={styles.sectionDescription}>
        Choose the microphone and system audio sources ContextCall listens to, then
        calibrate the mic gate if you need more aggressive filtering of room noise.
      </p>

      <div style={styles.grid}>
        <div style={styles.fieldWrap}>
          <label style={styles.labelStyle}>Mic Input</label>
          <select
            value={settings.inputDeviceName || "default"}
            onChange={(e) =>
              applyImmediateSettings({
                inputDeviceName: e.target.value === "default" ? null : e.target.value,
              })
            }
            style={styles.selectStyle}
          >
            <option value="default">Default microphone</option>
            {micDevices.map((device) => (
              <option key={device} value={device}>
                {device}
              </option>
            ))}
          </select>
        </div>

        <div style={styles.fieldWrap}>
          <label style={styles.labelStyle}>System Audio</label>
          <select
            value={settings.systemAudioDeviceName || "default"}
            onChange={(e) =>
              applyImmediateSettings({
                systemAudioDeviceName:
                  e.target.value === "default" ? null : e.target.value,
              })
            }
            style={styles.selectStyle}
          >
            <option value="default">Default system source</option>
            {systemAudioDevices.map((device) => (
              <option key={device} value={device}>
                {device}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div style={{ ...styles.fieldWrap, marginTop: spacing[3] }}>
        <label style={styles.labelStyle}>Mic Capture Mode</label>
        <select
          value={settings.micCaptureMode ?? "auto"}
          onChange={(e) =>
            applyImmediateSettings({
              micCaptureMode: e.target.value as "auto" | "push-to-talk",
            })
          }
          style={styles.selectStyle}
        >
          <option value="auto">Auto</option>
          <option value="push-to-talk">Push to talk</option>
        </select>
        <span style={{ fontSize: typography.sm, color: colors.textMuted, marginTop: 4, display: "block" }}>
          Auto keeps the current always-listening behavior. Push to talk only sends mic audio
          to transcription while you hold the live trigger.
        </span>
      </div>

      {settings.micCaptureMode === "push-to-talk" && (
        <div style={{ ...styles.fieldWrap, marginTop: spacing[3] }}>
          <label style={styles.labelStyle}>Push-to-Talk Shortcut</label>
          <div style={{ display: "flex", flexWrap: "wrap", gap: spacing[2] }}>
            <button
              style={styles.button}
              onClick={() =>
                setIsRecordingPushToTalkHotkey((previous) => !previous)
              }
            >
              {isRecordingPushToTalkHotkey
                ? "Press keys..."
                : `Record Shortcut (${normalizeShortcut(settings.pushToTalkHotkey) || "Space"})`}
            </button>
            <button
              style={styles.buttonSecondary}
              onClick={() =>
                applyImmediateSettings({ pushToTalkHotkey: "Space" })
              }
            >
              Reset to Space
            </button>
          </div>
          <span
            style={{
              fontSize: typography.sm,
              color: isRecordingPushToTalkHotkey ? colors.text : colors.textMuted,
              marginTop: 4,
              display: "block",
            }}
          >
            {isRecordingPushToTalkHotkey
              ? "Press a key or combination now. Escape cancels."
              : `Current shortcut: ${normalizeShortcut(settings.pushToTalkHotkey) || "Space"}. The shortcut is registered globally while live transcription is running.`}
          </span>
        </div>
      )}

      <div style={{ ...styles.fieldWrap, marginTop: spacing[3] }}>
        <label style={styles.labelStyle}>Mic Voice Threshold</label>
        {isCalibrating ? (
          <div style={{ display: "flex", flexDirection: "column", gap: spacing[1] }}>
            <WaveformVisualizer level={calibrationLevel} isActive={true} />
            <span style={{ fontSize: typography.sm, color: colors.textMuted }}>
              {calibrationCountdown > 0 ? `Speak normally... ${calibrationCountdown}` : "Processing..."}
            </span>
          </div>
        ) : (
          <>
            {settings.micCalibrationRms == null ? (
              <span style={{ fontSize: typography.sm, color: colors.textMuted }}>
                Not calibrated - gate is disabled
              </span>
            ) : (
              <>
                <span style={{ fontSize: typography.sm, color: colors.text }}>
                  Calibrated: {((settings.micCalibrationRms ?? 0) * 1000).toFixed(1)}
                </span>
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: spacing[2],
                    marginTop: spacing[1],
                    flexWrap: "wrap",
                  }}
                >
                  <label style={{ fontSize: typography.sm, color: colors.text }}>
                    Sensitivity
                  </label>
                  <input
                    type="range"
                    min={0.1}
                    max={0.8}
                    step={0.05}
                    value={settings.micThresholdMultiplier ?? 0.6}
                    onChange={(e) =>
                      applyImmediateSettings({
                        micThresholdMultiplier: parseFloat(e.target.value),
                      })
                    }
                    style={{ width: 160 }}
                  />
                  <span style={{ fontSize: typography.sm, color: colors.textMuted }}>
                    {((settings.micThresholdMultiplier ?? 0.6) * 100).toFixed(0)}%
                  </span>
                </div>
              </>
            )}
            {calibrationError && (
              <span
                style={{
                  fontSize: typography.sm,
                  color: colors.error,
                  marginTop: spacing[1],
                  display: "block",
                }}
              >
                {calibrationError}
              </span>
            )}
            <button
              style={{ ...styles.button, marginTop: spacing[1] }}
              onClick={startCalibration}
              disabled={isCalibrating}
            >
              {settings.micCalibrationRms == null ? "Calibrate" : "Recalibrate"}
            </button>
            <span
              style={{
                fontSize: typography.sm,
                color: colors.textMuted,
                marginTop: 4,
                display: "block",
              }}
            >
              Audio below this level will be silenced. Recalibrate if you change microphones
              or your meeting setup.
            </span>
          </>
        )}
      </div>
    </div>
  );
}
