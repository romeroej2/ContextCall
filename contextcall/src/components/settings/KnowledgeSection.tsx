// Knowledge & Notes settings section (Obsidian vault + legacy KB folder).
// Extracted from SettingsView.tsx (EP0007 / US0029).

import type { Dispatch, SetStateAction } from "react";
import { colors, typography, spacing } from "../../theme";
import type { AppSettings, MeetingTemplate } from "../../types";
import type { SettingsStyles } from "./settingsStyles";

interface KnowledgeSectionProps {
  settings: AppSettings;
  templates: MeetingTemplate[];
  styles: SettingsStyles;
  kbFileCount: number;
  isIndexingKb: boolean;
  kbStatus: string | null;
  isObsidianActive: boolean;
  hasKbSource: boolean;
  applyImmediateSettings: (update: Partial<AppSettings>) => Promise<boolean>;
  chooseFolder: (key: "kbFolderPath" | "notesFolderPath" | "obsidianVaultPath") => Promise<void>;
  addObsidianIncludePath: () => Promise<void>;
  removeObsidianIncludePath: (relativePath: string) => Promise<void>;
  runKnowledgeBaseSyncNow: () => void;
  setKbFileCount: Dispatch<SetStateAction<number>>;
  setKbStatus: Dispatch<SetStateAction<string | null>>;
}

export function KnowledgeSection({
  settings,
  templates,
  styles,
  kbFileCount,
  isIndexingKb,
  kbStatus,
  isObsidianActive,
  hasKbSource,
  applyImmediateSettings,
  chooseFolder,
  addObsidianIncludePath,
  removeObsidianIncludePath,
  runKnowledgeBaseSyncNow,
  setKbFileCount,
  setKbStatus,
}: KnowledgeSectionProps) {
  return (
    <>
      {/* Obsidian Section */}
      <div style={styles.section}>
        <h4 style={styles.sectionTitle}>Knowledge &amp; Notes</h4>
        <p style={styles.sectionDescription}>
          Connect a local Obsidian vault to power suggestions and keep your meeting knowledge in one place. ContextCall still works without Obsidian for transcription, summaries, and notes.
        </p>
        <div style={styles.fieldWrap}>
          <label style={styles.labelStyle}>Vault</label>
          <div style={{ display: "flex", gap: spacing[2] }}>
            <input
              type="text"
              value={settings.obsidianVaultPath || ""}
              readOnly
              style={{ ...styles.inputStyle, flex: 1 }}
              placeholder="Choose a vault..."
            />
            <button
              style={styles.buttonSecondary}
              onClick={() => chooseFolder("obsidianVaultPath")}
            >
              {settings.obsidianVaultPath ? "Change..." : "Choose..."}
            </button>
            {settings.obsidianVaultPath && (
              <button
                style={{ ...styles.buttonSecondary, color: colors.error }}
                onClick={() => {
                  setKbFileCount(0);
                  setKbStatus(null);
                  void applyImmediateSettings({
                    obsidianVaultPath: null,
                    obsidianKbIncludePaths: [],
                  });
                }}
              >
                Clear
              </button>
            )}
          </div>
        </div>

        {settings.obsidianVaultPath && (
          <>
            <div style={styles.fieldWrap}>
              <label style={styles.labelStyle}>Knowledge Folders</label>
              <div style={{ display: "flex", flexDirection: "column", gap: spacing[2] }}>
                {settings.obsidianKbIncludePaths.length === 0 && (
                  <span style={{ fontSize: typography.sm, color: colors.textMuted }}>
                    No extra folders added yet. ContextCall will still index `{settings.obsidianNotesFolder}` for generated notes.
                  </span>
                )}
                {settings.obsidianKbIncludePaths.map((relativePath) => (
                  <div
                    key={relativePath}
                    style={{ display: "flex", alignItems: "center", gap: spacing[2] }}
                  >
                    <input
                      type="text"
                      value={relativePath}
                      readOnly
                      style={{ ...styles.inputStyle, flex: 1 }}
                    />
                    <button
                      style={{ ...styles.buttonSecondary, color: colors.error }}
                      onClick={() => void removeObsidianIncludePath(relativePath)}
                    >
                      Remove
                    </button>
                  </div>
                ))}
                <div style={{ display: "flex", gap: spacing[2], alignItems: "center" }}>
                  <button
                    style={styles.buttonSecondary}
                    onClick={() => void addObsidianIncludePath()}
                  >
                    Add Folder...
                  </button>
                  <span style={{ fontSize: typography.sm, color: colors.textMuted }}>
                    Choose the folders inside your vault that should be searched for suggestions.
                  </span>
                </div>
              </div>
            </div>

            <div style={styles.fieldWrap}>
              <label style={styles.labelStyle}>Default Notes Template</label>
              <select
                value={settings.defaultNotesTemplateId || ""}
                onChange={(e) =>
                  applyImmediateSettings({
                    defaultNotesTemplateId: e.target.value || null,
                  })
                }
                style={styles.selectStyle}
              >
                <option value="">Summary (default)</option>
                {templates.map((template) => (
                  <option key={template.id} value={template.id}>
                    {template.name}
                  </option>
                ))}
              </select>
            </div>

            <div
              style={{
                marginTop: spacing[2],
                display: "flex",
                alignItems: "center",
                gap: spacing[2],
                flexWrap: "wrap",
              }}
            >
              <span style={styles.statusBadge("success")}>
                <span>
                  {isIndexingKb
                    ? "Indexing vault..."
                    : kbStatus || `Vault connected - ${kbFileCount > 0 ? `${kbFileCount} folders indexed` : "ready"}`}
                </span>
              </span>
              <button
                style={styles.buttonSecondary}
                onClick={runKnowledgeBaseSyncNow}
                disabled={isIndexingKb}
              >
                Sync now
              </button>
            </div>

            <p style={{ ...styles.sectionDescription, marginTop: spacing[3], marginBottom: 0 }}>
              ContextCall publishes canonical notes to `{settings.obsidianNotesFolder}` and keeps transcript companions in `{settings.obsidianTranscriptsFolder}`. Generated notes are indexed automatically; transcripts are not.
            </p>
          </>
        )}
      </div>

      {!settings.obsidianVaultPath && settings.kbFolderPath && (
        <p style={{ ...styles.sectionDescription, marginBottom: spacing[4] }}>
          A legacy folder-based knowledge base is still saved in your settings for compatibility, but the main workflow now centers on an Obsidian vault.
        </p>
      )}

      <div style={{ ...styles.divider, display: "none" }} />


      {/* Knowledge Base Section */}
      <div style={{ ...styles.section, display: "none" }}>
        <h4 style={styles.sectionTitle}>Knowledge Base</h4>
        <p style={styles.sectionDescription}>
          Legacy folder source for suggestions. When an Obsidian vault is connected above, this path is kept only for backwards compatibility and is ignored.
        </p>
        <div style={styles.fieldWrap}>
          <label style={styles.labelStyle}>Legacy KB Folder</label>
          <div style={{ display: "flex", gap: spacing[2], alignItems: "center" }}>
            <input
              type="text"
              value={settings.kbFolderPath || ""}
              readOnly
              style={{ ...styles.inputStyle, flex: 1 }}
              placeholder="No folder selected..."
            />
            <button
              style={styles.buttonSecondary}
              onClick={() => chooseFolder("kbFolderPath")}
              disabled={isObsidianActive}
            >
              {settings.kbFolderPath ? "Change..." : "Choose..."}
            </button>
            {settings.kbFolderPath && (
              <button
                style={{ ...styles.buttonSecondary, color: colors.error }}
                disabled={isObsidianActive}
                onClick={() => {
                  setKbFileCount(0);
                  setKbStatus(null);
                  void applyImmediateSettings({ kbFolderPath: "" });
                }}
              >
                Clear
              </button>
            )}
          </div>
          {hasKbSource && (
            <div style={{ marginTop: spacing[2] }}>
              <span style={styles.statusBadge("success")}>
                <span>⚡</span>
                <span>
                  {isIndexingKb
                    ? "Indexing knowledge base..."
                    : kbStatus || `KB Connected ${kbFileCount > 0 ? `· ${kbFileCount} files` : ""}`}
                </span>
              </span>
            </div>
          )}
        </div>
      </div>
    </>
  );
}
