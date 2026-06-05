// "Show advanced" toggle shown at the start of a section's advanced group.
// EP0007 / US0030.

import { colors, typography, spacing } from "../../theme";
import type { SettingsStyles } from "./settingsStyles";

interface AdvancedToggleProps {
  checked: boolean;
  onChange: (value: boolean) => void;
  styles: SettingsStyles;
}

export function AdvancedToggle({ checked, onChange, styles }: AdvancedToggleProps) {
  return (
    <label style={{ ...styles.checkboxStyle, marginTop: spacing[2], marginBottom: spacing[3] }}>
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        style={styles.checkboxInput}
      />
      <span style={{ fontSize: typography.sm, color: colors.textSecondary, fontWeight: 600 }}>
        Show advanced
      </span>
    </label>
  );
}
