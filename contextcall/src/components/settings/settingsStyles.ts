// Shared inline-style factory for the settings UI.
// Extracted from SettingsView.tsx (EP0007 / US0029) so section components
// can consume one consistent style set. Depends on `compact` (overlay vs full).

import type { CSSProperties } from "react";
import { colors, typography, spacing, radius } from "../../theme";

export function createSettingsStyles(compact: boolean) {
  return {
    container: {
      padding: compact ? spacing[3] : spacing[4],
      overflowY: "auto" as const,
      flex: 1,
      minHeight: 0,
      backgroundColor: colors.background,
    },
    header: {
      margin: `0 0 ${spacing[4]}px`,
      color: colors.text,
      fontSize: typography.lg,
      fontWeight: 600,
    },
    tabs: {
      display: "flex" as const,
      gap: spacing[1],
      flexWrap: "wrap" as const,
      marginBottom: spacing[4],
      borderBottom: `1px solid ${colors.border}`,
      paddingBottom: spacing[1],
    },
    tab: (isActive: boolean): CSSProperties => ({
      padding: compact ? `${spacing[2]}px` : `${spacing[2]}px ${spacing[3]}px`,
      background: isActive && compact ? `${colors.accent}10` : "transparent",
      border: isActive && compact ? `1px solid ${colors.accent}28` : "none",
      borderBottom: compact ? "none" : isActive ? `2px solid ${colors.accent}` : "2px solid transparent",
      borderRadius: compact ? radius.xl : 0,
      color: isActive ? colors.accent : colors.textSecondary,
      fontSize: typography.base,
      fontWeight: isActive ? 700 : 500,
      cursor: "pointer",
      transition: "all 0.2s",
    }),
    section: {
      marginBottom: spacing[4],
      background: colors.surface,
      border: `1px solid ${colors.border}`,
      borderRadius: radius.lg,
      // Generous inner padding so rounded corners never clip section text.
      padding: compact ? spacing[3] : spacing[5],
    },
    sectionTitle: {
      color: colors.textSecondary,
      fontSize: typography.xs,
      textTransform: "uppercase" as const,
      letterSpacing: "1.5px",
      margin: `0 0 ${spacing[2]}px`,
      fontWeight: 600,
    },
    sectionDescription: {
      color: colors.textMuted,
      fontSize: typography.sm,
      margin: `0 0 ${spacing[3]}px`,
      lineHeight: 1.5,
    },
    fieldWrap: {
      marginBottom: spacing[3],
    },
    labelStyle: {
      display: "block" as const,
      fontSize: typography.base,
      color: colors.textSecondary,
      marginBottom: spacing[1],
      fontWeight: 500,
    },
    inputStyle: {
      width: "100%",
      padding: `${spacing[2]}px`,
      background: colors.surface,
      color: colors.text,
      border: `1px solid ${colors.border}`,
      borderRadius: radius.sm,
      fontSize: typography.md,
      boxSizing: "border-box" as const,
      fontFamily: "inherit",
    },
    selectStyle: {
      width: "100%",
      padding: `${spacing[2]}px`,
      background: colors.surface,
      color: colors.text,
      border: `1px solid ${colors.border}`,
      borderRadius: radius.sm,
      fontSize: typography.md,
      cursor: "pointer",
    },
    checkboxStyle: {
      display: "flex",
      alignItems: "center",
      gap: spacing[2],
      cursor: "pointer",
    },
    checkboxInput: {
      width: 16,
      height: 16,
      accentColor: colors.accent,
    },
    checkboxLabel: {
      fontSize: typography.base,
      color: colors.text,
    },
    button: {
      padding: `${spacing[2]}px ${spacing[3]}px`,
      background: colors.accent,
      color: colors.textInverse,
      border: "none",
      borderRadius: radius.sm,
      fontSize: typography.base,
      cursor: "pointer",
      transition: "background 0.2s",
    },
    buttonSecondary: {
      padding: `${spacing[2]}px ${spacing[3]}px`,
      background: "transparent",
      color: colors.textSecondary,
      border: `1px solid ${colors.border}`,
      borderRadius: radius.sm,
      fontSize: typography.base,
      cursor: "pointer",
    },
    statusBadge: (type: "success" | "warning" | "error"): CSSProperties => ({
      display: "inline-flex",
      alignItems: "center",
      gap: spacing[1],
      padding: `${spacing[1]}px ${spacing[2]}px`,
      background: type === "success" ? `${colors.success}15` : type === "warning" ? `${colors.warning}15` : `${colors.error}15`,
      color: type === "success" ? colors.success : type === "warning" ? colors.warning : colors.error,
      borderRadius: radius.sm,
      fontSize: typography.sm,
    }),
    grid: {
      display: "grid",
      gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))",
      gap: spacing[3],
    },
    aiModeCard: (isSelected: boolean): CSSProperties => ({
      padding: spacing[3],
      background: isSelected ? `${colors.accent}10` : colors.surface,
      border: `1px solid ${isSelected ? colors.accent : colors.border}`,
      borderRadius: radius.lg,
      cursor: "pointer",
      transition: "all 0.2s",
    }),
    aiModeTitle: {
      fontSize: typography.md,
      fontWeight: 600,
      color: colors.text,
      marginBottom: spacing[1],
    },
    aiModeDesc: {
      fontSize: typography.sm,
      color: colors.textMuted,
      lineHeight: 1.4,
    },
    divider: {
      // Sections are now self-contained cards, so the old hairline between them
      // is redundant — collapse it to plain spacing.
      height: 0,
      background: "transparent",
      margin: `${spacing[1]}px 0`,
    },
  };
}

export type SettingsStyles = ReturnType<typeof createSettingsStyles>;
