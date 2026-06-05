// Shared field primitives for settings sections (EP0007 / US0029 slice 3).
// Collapse the repeated label+control+hint markup so section files stay small.

import type { ReactNode } from "react";
import { colors, typography } from "../../theme";
import type { SettingsStyles } from "./settingsStyles";

export function FieldHint({ children }: { children: ReactNode }) {
  return (
    <span style={{ fontSize: typography.sm, color: colors.textMuted, marginTop: 4, display: "block" }}>
      {children}
    </span>
  );
}

interface Option {
  value: string;
  label: string;
  description?: string;
}

interface LabeledSelectProps {
  styles: SettingsStyles;
  label: string;
  value: string;
  options: readonly Option[];
  onChange: (value: string) => void;
  hint?: ReactNode;
}

export function LabeledSelect({ styles, label, value, options, onChange, hint }: LabeledSelectProps) {
  return (
    <div style={styles.fieldWrap}>
      <label style={styles.labelStyle}>{label}</label>
      <select value={value} onChange={(e) => onChange(e.target.value)} style={styles.selectStyle}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      {hint != null && <FieldHint>{hint}</FieldHint>}
    </div>
  );
}

interface LabeledTextFieldProps {
  styles: SettingsStyles;
  label: string;
  value: string;
  onChange: (value: string) => void;
  /** Called on blur and on Enter (matches the old handleTextFieldCommitKeyDown). */
  onCommit?: () => Promise<unknown> | void;
  type?: "text" | "password" | "number";
  placeholder?: string;
  hint?: ReactNode;
}

export function LabeledTextField({
  styles,
  label,
  value,
  onChange,
  onCommit,
  type = "text",
  placeholder,
  hint,
}: LabeledTextFieldProps) {
  return (
    <div style={styles.fieldWrap}>
      <label style={styles.labelStyle}>{label}</label>
      <input
        type={type}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        onBlur={onCommit ? () => void onCommit() : undefined}
        onKeyDown={
          onCommit
            ? (e) => {
                if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
                e.preventDefault();
                void onCommit();
              }
            : undefined
        }
        style={styles.inputStyle}
      />
      {hint != null && <FieldHint>{hint}</FieldHint>}
    </div>
  );
}
