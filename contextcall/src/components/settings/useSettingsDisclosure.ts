// Per-session "Show advanced" disclosure state, persisted to localStorage.
// EP0007 / US0030.

import { useCallback, useState } from "react";

const KEY = "cc.settings.showAdvanced";

export function useSettingsDisclosure() {
  const [showAdvanced, setShowAdvancedState] = useState<boolean>(() => {
    try {
      return localStorage.getItem(KEY) === "1";
    } catch {
      return false;
    }
  });

  const setShowAdvanced = useCallback((value: boolean) => {
    setShowAdvancedState(value);
    try {
      localStorage.setItem(KEY, value ? "1" : "0");
    } catch {
      /* localStorage unavailable — keep in-memory only */
    }
  }, []);

  return { showAdvanced, setShowAdvanced };
}
