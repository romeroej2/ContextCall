use keyring::Entry;

const SERVICE: &str = "com.contextcall.app";
/// Legacy service namespace used before the app was renamed from "openoats".
/// Entries here are migrated to `SERVICE` on first `load()` and then removed.
const LEGACY_SERVICE: &str = "com.openoats.app";

pub struct KeyEntry {
    entry: Entry,
    /// Legacy-namespace entry for the same key, if a one-time migration applies.
    legacy: Option<Entry>,
}

impl KeyEntry {
    fn new(key: &str) -> Self {
        Self {
            entry: Entry::new(SERVICE, key).expect("keyring entry creation failed"),
            legacy: Entry::new(LEGACY_SERVICE, key).ok(),
        }
    }

    pub fn new_with_service(service: &str, key: &str) -> Self {
        Self {
            entry: Entry::new(service, key).expect("keyring entry creation failed"),
            legacy: None,
        }
    }

    /// Test-only constructor that wires an explicit legacy service so migration
    /// can be exercised without touching the real credential namespace.
    #[cfg(test)]
    fn new_with_legacy(service: &str, legacy_service: &str, key: &str) -> Self {
        Self {
            entry: Entry::new(service, key).expect("keyring entry creation failed"),
            legacy: Entry::new(legacy_service, key).ok(),
        }
    }

    pub fn open_router_api_key() -> Self {
        Self::new("openRouterApiKey")
    }
    pub fn voyage_api_key() -> Self {
        Self::new("voyageApiKey")
    }
    pub fn open_ai_llm_api_key() -> Self {
        Self::new("openAILLMApiKey")
    }
    pub fn open_ai_embed_api_key() -> Self {
        Self::new("openAIEmbedApiKey")
    }
    pub fn hugging_face_token() -> Self {
        Self::new("huggingFaceToken")
    }

    pub fn save(&self, value: &str) -> Result<(), keyring::Error> {
        self.entry.set_password(value)
    }

    pub fn save_or_delete(&self, value: &str) -> Result<(), keyring::Error> {
        if value.trim().is_empty() {
            match self.entry.delete_credential() {
                Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(err) => Err(err),
            }
        } else {
            self.entry.set_password(value)
        }
    }

    pub fn load(&self) -> Option<String> {
        if let Ok(value) = self.entry.get_password() {
            return Some(value);
        }
        // One-time migration: if the key only exists under the legacy service,
        // copy it forward to the canonical service and delete the legacy entry.
        if let Some(legacy) = &self.legacy {
            if let Ok(value) = legacy.get_password() {
                let _ = self.entry.set_password(&value);
                let _ = legacy.delete_credential();
                return Some(value);
            }
        }
        None
    }

    pub fn delete(&self) -> Result<(), keyring::Error> {
        self.entry.delete_credential()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_roundtrip() {
        let entry = KeyEntry::new_with_service("openoats-test", "test_api_key");
        entry.save("sk-test-value-123").unwrap();
        let loaded = entry.load().unwrap();
        assert_eq!(loaded, "sk-test-value-123");
        entry.delete().ok();
    }

    #[test]
    fn load_missing_key_returns_none() {
        let entry = KeyEntry::new_with_service("contextcall-test", "definitely_does_not_exist_xyz");
        let result = entry.load();
        assert!(result.is_none());
    }

    /// Probe whether the compiled keyring backend persists a credential across
    /// separate `Entry` instances. The default keyring v3 store (no platform
    /// feature enabled) is an in-process mock that does NOT — under it the
    /// cross-namespace migration cannot be exercised, so callers skip.
    #[cfg(test)]
    fn backend_persists_cross_instance() -> bool {
        const SVC: &str = "contextcall-persist-probe";
        const KEY: &str = "probe_key";
        let writer = KeyEntry::new_with_service(SVC, KEY);
        if writer.save("probe-value").is_err() {
            return false;
        }
        let reader = KeyEntry::new_with_service(SVC, KEY);
        let persists = reader.load().as_deref() == Some("probe-value");
        writer.delete().ok();
        persists
    }

    #[test]
    fn load_migrates_from_legacy_service() {
        if !backend_persists_cross_instance() {
            eprintln!(
                "[keychain] mock keyring backend (no platform feature) — \
                 skipping cross-namespace migration assertions"
            );
            return;
        }

        const NEW: &str = "contextcall-migrate-new-test";
        const OLD: &str = "contextcall-migrate-legacy-test";
        const KEY: &str = "migrate_key";

        // Seed a value only in the legacy namespace.
        let legacy = KeyEntry::new_with_service(OLD, KEY);
        legacy.save("sk-legacy-123").unwrap();

        // Canonical entry with legacy fallback should migrate it forward.
        let entry = KeyEntry::new_with_legacy(NEW, OLD, KEY);
        assert_eq!(entry.load().as_deref(), Some("sk-legacy-123"));

        // Value now lives under the new service...
        let plain_new = KeyEntry::new_with_service(NEW, KEY);
        assert_eq!(plain_new.load().as_deref(), Some("sk-legacy-123"));

        // ...and the legacy entry was deleted (full replace).
        let plain_old = KeyEntry::new_with_service(OLD, KEY);
        assert!(plain_old.load().is_none());

        plain_new.delete().ok();
    }

    #[test]
    fn load_migration_is_idempotent_and_noop_without_legacy() {
        const NEW: &str = "contextcall-noop-new-test";
        const OLD: &str = "contextcall-noop-legacy-test";
        const KEY: &str = "noop_key";

        // No legacy value present → load returns None, no panic.
        let entry = KeyEntry::new_with_legacy(NEW, OLD, KEY);
        assert!(entry.load().is_none());
        assert!(entry.load().is_none());
    }
}
