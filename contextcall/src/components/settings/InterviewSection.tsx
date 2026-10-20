// Interview mode settings: resume, role, and the fast/deep Claude tiers.

import { useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { colors, spacing, typography } from "../../theme";
import type { AppSettings } from "../../types";
import type { SettingsStyles } from "./settingsStyles";
import { ClaudeCliCheck } from "./ClaudeCliCheck";
import { FieldHint, LabeledSelect } from "./fields";

interface InterviewSectionProps {
  settings: AppSettings;
  styles: SettingsStyles;
  applyImmediateSettings: (update: Partial<AppSettings>) => Promise<boolean>;
  setSettings: Dispatch<SetStateAction<AppSettings | null>>;
  commitSettingsDraft: () => Promise<boolean>;
}

const BACKENDS = [
  { value: "claude-cli", label: "Claude CLI — your Claude login (recommended)" },
  { value: "anthropic-api", label: "Anthropic API — Console API key" },
] as const;

const FAST_MODELS = [
  { value: "haiku", label: "Haiku — fastest, ~0.8 s to first words (recommended)" },
  { value: "sonnet", label: "Sonnet — richer answers at ~1 s" },
  { value: "opus", label: "Opus — strongest, slower to finish" },
] as const;

const ROLES = [
  { value: "interview", label: "Interview — I'm the candidate" },
  { value: "consultant", label: "Consultant — expert assistant for any call" },
] as const;

const DEEP_MODELS = [
  { value: "opus", label: "Opus (recommended)" },
  { value: "sonnet", label: "Sonnet" },
] as const;

export function InterviewSection({
  settings,
  styles,
  applyImmediateSettings,
  setSettings,
  commitSettingsDraft,
}: InterviewSectionProps) {
  const [resumeStatus, setResumeStatus] = useState<{ ok: boolean; message: string } | null>(null);

  const checkResume = async (path: string) => {
    try {
      const message = await invoke<string>("check_interview_resume", { path });
      setResumeStatus({ ok: true, message });
    } catch (err) {
      setResumeStatus({ ok: false, message: String(err) });
    }
  };

  const chooseResume = async () => {
    const path = await invoke<string | null>("choose_resume_file");
    if (!path) return;
    await applyImmediateSettings({ interviewResumePath: path });
    await checkResume(path);
  };

  const consultant = settings.interviewRole === "consultant";
  const textareaStyle = { ...styles.inputStyle, minHeight: 90, resize: "vertical" as const, fontFamily: "inherit" };

  return (
    <div>
      <div style={styles.section}>
        <h4 style={styles.sectionTitle}>Interview Mode</h4>
        <p style={styles.sectionDescription}>
          Answers each question the other side asks live: a quick answer you can say right away, then the technical
          depth from Opus. You can also type any question (even without recording) and get a straight expert answer.
          Both remember the whole conversation, so follow-ups make sense.
        </p>
        <label style={styles.checkboxStyle}>
          <input
            type="checkbox"
            checked={settings.interviewModeEnabled}
            onChange={(e) => applyImmediateSettings({ interviewModeEnabled: e.target.checked })}
            style={styles.checkboxInput}
          />
          <span style={styles.checkboxLabel}>
            Enable interview mode
            <span style={{ display: "block", fontSize: typography.sm, color: colors.textMuted, marginTop: 2 }}>
              Replaces the regular suggestion cards while recording. Takes effect on the next session.
            </span>
          </span>
        </label>
        <LabeledSelect
          styles={styles}
          label="Copilot Role"
          value={settings.interviewRole}
          options={ROLES}
          onChange={(v) => applyImmediateSettings({ interviewRole: v })}
          hint="Consultant drops the candidate framing: answers are for client calls, workshops and technical discussions."
        />
        <label style={styles.checkboxStyle}>
          <input
            type="checkbox"
            checked={settings.interviewAutoAnswer}
            onChange={(e) => applyImmediateSettings({ interviewAutoAnswer: e.target.checked })}
            style={styles.checkboxInput}
          />
          <span style={styles.checkboxLabel}>
            Answer automatically when a question is detected
            <span style={{ display: "block", fontSize: typography.sm, color: colors.textMuted, marginTop: 2 }}>
              Off: answers only when you press Answer or type a question.
            </span>
          </span>
        </label>
      </div>

      <div style={styles.divider} />

      <div style={styles.section}>
        <h4 style={styles.sectionTitle}>{consultant ? "You and the Engagement" : "You and the Role"}</h4>
        <div style={styles.fieldWrap}>
          <label style={styles.labelStyle}>{consultant ? "Your Profile (optional)" : "Resume"}</label>
          <div style={{ display: "flex", gap: spacing[2], alignItems: "center" }}>
            <input
              type="text"
              value={settings.interviewResumePath ?? ""}
              onChange={(e) => {
                setResumeStatus(null);
                setSettings({ ...settings, interviewResumePath: e.target.value || null });
              }}
              onBlur={() => {
                void commitSettingsDraft();
                if (settings.interviewResumePath) void checkResume(settings.interviewResumePath);
              }}
              style={styles.inputStyle}
              placeholder="PDF, Markdown or text file"
            />
            <button style={styles.buttonSecondary} onClick={() => void chooseResume()}>
              Choose…
            </button>
          </div>
          {resumeStatus && (
            <div style={{ ...styles.statusBadge(resumeStatus.ok ? "success" : "error"), marginTop: spacing[2] }}>
              <span>{resumeStatus.message}</span>
            </div>
          )}
          <FieldHint>PDFs are sent to Claude as-is, so layout and tables are kept.</FieldHint>
        </div>
        <div style={styles.fieldWrap}>
          <label style={styles.labelStyle}>{consultant ? "Client / Engagement" : "Job Description"}</label>
          <textarea
            value={settings.interviewJobDescription}
            onChange={(e) => setSettings({ ...settings, interviewJobDescription: e.target.value })}
            onBlur={() => void commitSettingsDraft()}
            style={textareaStyle}
            placeholder={
              consultant
                ? "Who the client is, their stack, the goal of the engagement"
                : "Paste the job posting or describe the role and company"
            }
          />
        </div>
        <div style={styles.fieldWrap}>
          <label style={styles.labelStyle}>Notes for the Copilot</label>
          <textarea
            value={settings.interviewExtraContext}
            onChange={(e) => setSettings({ ...settings, interviewExtraContext: e.target.value })}
            onBlur={() => void commitSettingsDraft()}
            style={textareaStyle}
            placeholder="Stories to emphasize, salary expectations, things to avoid, preferred tech stack…"
          />
        </div>
      </div>

      <div style={styles.divider} />

      <div style={styles.section}>
        <h4 style={styles.sectionTitle}>Brain</h4>
        <LabeledSelect
          styles={styles}
          label="Connection"
          value={settings.interviewBackend}
          options={BACKENDS}
          onChange={(v) => applyImmediateSettings({ interviewBackend: v })}
          hint={
            settings.interviewBackend === "anthropic-api"
              ? "Uses the API key, base URL and auth mode from Settings → AI → Anthropic API. Thinking and web search below apply to the Claude CLI only."
              : undefined
          }
        />
        {settings.interviewBackend !== "anthropic-api" && (
          <ClaudeCliCheck
            settings={settings}
            styles={styles}
            setSettings={setSettings}
            commitSettingsDraft={commitSettingsDraft}
            model={settings.interviewFastModel}
          />
        )}
        <LabeledSelect
          styles={styles}
          label="Quick Answer Model"
          value={settings.interviewFastModel}
          options={FAST_MODELS}
          onChange={(v) => applyImmediateSettings({ interviewFastModel: v })}
          hint="Runs with thinking off so the first words arrive in about a second."
        />
        <label style={styles.checkboxStyle}>
          <input
            type="checkbox"
            checked={settings.interviewDeepEnabled}
            onChange={(e) => applyImmediateSettings({ interviewDeepEnabled: e.target.checked })}
            style={styles.checkboxInput}
          />
          <span style={styles.checkboxLabel}>Add a deeper answer (research tier)</span>
        </label>
        {settings.interviewDeepEnabled && (
          <>
            <LabeledSelect
              styles={styles}
              label="Depth Model"
              value={settings.interviewDeepModel}
              options={DEEP_MODELS}
              onChange={(v) => applyImmediateSettings({ interviewDeepModel: v })}
            />
            <label style={styles.checkboxStyle}>
              <input
                type="checkbox"
                checked={settings.interviewDeepThinking}
                onChange={(e) => applyImmediateSettings({ interviewDeepThinking: e.target.checked })}
                style={styles.checkboxInput}
              />
              <span style={styles.checkboxLabel}>
                Let it think first
                <span style={{ display: "block", fontSize: typography.sm, color: colors.textMuted, marginTop: 2 }}>
                  More accurate on hard problems; adds a few seconds before the depth answer starts.
                </span>
              </span>
            </label>
            <label style={styles.checkboxStyle}>
              <input
                type="checkbox"
                checked={settings.interviewDeepWebSearch}
                onChange={(e) => applyImmediateSettings({ interviewDeepWebSearch: e.target.checked })}
                style={styles.checkboxInput}
              />
              <span style={styles.checkboxLabel}>
                Allow web search
                <span style={{ display: "block", fontSize: typography.sm, color: colors.textMuted, marginTop: 2 }}>
                  For questions about the company or recent tech. Much slower (10 s+).
                </span>
              </span>
            </label>
          </>
        )}
      </div>
    </div>
  );
}
