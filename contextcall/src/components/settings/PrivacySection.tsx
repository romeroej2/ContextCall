// Privacy settings section (hide from screen share).
// Extracted from SettingsView.tsx (EP0007 / US0029).

import { typography, colors } from "../../theme";
import type { AppSettings } from "../../types";
import type { SettingsStyles } from "./settingsStyles";

interface PrivacySectionProps {
  settings: AppSettings;
  styles: SettingsStyles;
  applyImmediateSettings: (update: Partial<AppSettings>) => Promise<boolean>;
}

export function PrivacySection({ settings, styles, applyImmediateSettings }: PrivacySectionProps) {
  return (
    <div style={styles.section}>
      <h4 style={styles.sectionTitle}>Privacy</h4>
      <label style={styles.checkboxStyle}>
        <input
          type="checkbox"
          checked={settings.hideFromScreenShare}
          onChange={(e) =>
            applyImmediateSettings({ hideFromScreenShare: e.target.checked })
          }
          style={styles.checkboxInput}
        />
        <span style={styles.checkboxLabel}>
          Hide from screen sharing
          <span style={{ display: "block", fontSize: typography.sm, color: colors.textMuted, marginTop: 2 }}>
            Makes the app invisible during screen recordings
          </span>
        </span>
      </label>
    </div>
  );
}
