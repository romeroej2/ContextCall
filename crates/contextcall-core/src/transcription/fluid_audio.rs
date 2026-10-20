//! FluidAudio sidecar (macOS only): Parakeet TDT on CoreML / Apple Neural Engine plus
//! WeSpeaker speaker embeddings for diarization.
//!
//! The helper is a Swift executable (`contextcall/src-tauri/helpers/fluidaudio`) bundled
//! next to the app binary. It speaks the same JSON-lines protocol as the Python workers,
//! except audio travels as base64 little-endian f32 instead of a JSON float array.
//! Models are downloaded and compiled by FluidAudio itself on first `ensure_model`.

use crate::process_control::ManagedChild;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const HELPER_NAME: &str = "contextcall-fluidaudio";
pub const DEFAULT_MODEL: &str = "v3";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FluidAudioConfig {
    pub helper_path: PathBuf,
    /// Directory for ContextCall's own setup stamps (models live in FluidAudio's cache).
    pub runtime_root: PathBuf,
    /// "v3" (Parakeet TDT 0.6B v3), "ultra" (v3 post-trained) or "v2" (English only).
    pub model: String,
    /// ISO 639-1 hint ("en", "es") that keeps v3 from switching languages; empty = auto.
    pub language: String,
    pub diarization_enabled: bool,
}

impl FluidAudioConfig {
    /// The helper ships next to the main executable (Tauri strips the target-triple
    /// suffix from externalBin entries in both `tauri dev` and bundled builds).
    pub fn default_helper_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(HELPER_NAME)))
            .unwrap_or_else(|| PathBuf::from(HELPER_NAME))
    }

    pub fn helper_available(&self) -> bool {
        cfg!(target_os = "macos") && self.helper_path.is_file()
    }

    pub fn model_stamp_path(&self) -> PathBuf {
        let model = self
            .model
            .replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        self.runtime_root.join(format!("model-{model}.stamp"))
    }

    pub fn diarizer_stamp_path(&self) -> PathBuf {
        self.runtime_root.join("diarizer.stamp")
    }

    pub fn model_ready(&self) -> bool {
        self.model_stamp_path().is_file()
    }

    pub fn diarizer_ready(&self) -> bool {
        self.diarizer_stamp_path().is_file()
    }
}

pub fn supported() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

/// Downloads (first run) and compiles the CoreML models, then records stamps so status
/// checks don't need to spawn the helper. First-time ANE compilation takes minutes.
pub fn ensure_model<F>(config: &FluidAudioConfig, asr: bool, on_line: F) -> Result<(), String>
where
    F: Fn(&str) + Send + 'static,
{
    std::fs::create_dir_all(&config.runtime_root)
        .map_err(|e| format!("Failed to create FluidAudio runtime dir: {e}"))?;
    let mut worker = FluidAudioWorker::spawn_with_log(config, on_line)?;
    worker.ensure_model(asr, config.diarization_enabled)?;
    if asr {
        std::fs::write(config.model_stamp_path(), b"ok").map_err(|e| e.to_string())?;
    }
    if config.diarization_enabled {
        std::fs::write(config.diarizer_stamp_path(), b"ok").map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub struct FluidAudioWorker {
    child: ManagedChild,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_tail: Arc<Mutex<String>>,
    model: String,
    language: String,
}

impl FluidAudioWorker {
    pub fn spawn(config: &FluidAudioConfig) -> Result<Self, String> {
        Self::spawn_with_log(config, |_| {})
    }

    pub fn spawn_with_log<F>(config: &FluidAudioConfig, on_line: F) -> Result<Self, String>
    where
        F: Fn(&str) + Send + 'static,
    {
        if !config.helper_available() {
            return Err(format!(
                "FluidAudio helper not found at {}.",
                config.helper_path.display()
            ));
        }
        let mut command = Command::new(&config.helper_path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = ManagedChild::spawn(&mut command, "fluidaudio worker")
            .map_err(|e| format!("Failed to launch FluidAudio helper: {e}"))?;
        let stdin = child
            .take_stdin()
            .ok_or_else(|| "Failed to open stdin for FluidAudio helper.".to_string())?;
        let stdout = child
            .take_stdout()
            .ok_or_else(|| "Failed to open stdout for FluidAudio helper.".to_string())?;
        let stderr = child
            .take_stderr()
            .ok_or_else(|| "Failed to open stderr for FluidAudio helper.".to_string())?;
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        pump_stderr(stderr, Arc::clone(&stderr_tail), on_line);
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr_tail,
            model: config.model.clone(),
            language: config.language.trim().to_ascii_lowercase(),
        })
    }

    pub fn health(&mut self) -> Result<(), String> {
        self.send_request(json!({ "command": "health" }))
            .map(|_| ())
    }

    pub fn ensure_model(&mut self, asr: bool, diarization: bool) -> Result<(), String> {
        self.send_request(json!({
            "command": "ensure_model",
            "model": self.model,
            "asr": asr,
            "diarization": diarization,
        }))
        .map(|_| ())
    }

    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String, String> {
        let mut payload = json!({
            "command": "transcribe",
            "samples_b64": encode_samples(samples),
        });
        if !self.language.is_empty() && self.language != "auto" {
            payload["language"] = self.language.clone().into();
        }
        let response = self.send_request(payload)?;
        Ok(response["text"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string())
    }

    /// Returns "speaker_N" (0-based) or None when the audio is too short to embed.
    pub fn speaker_id(&mut self, samples: &[f32]) -> Result<Option<String>, String> {
        let response = self.send_request(json!({
            "command": "speaker_id",
            "samples_b64": encode_samples(samples),
        }))?;
        Ok(response["speaker_id"].as_str().map(|s| s.to_string()))
    }

    pub fn clear_speakers(&mut self) -> Result<(), String> {
        self.send_request(json!({ "command": "clear_speakers" }))
            .map(|_| ())
    }

    pub fn shutdown(&mut self) -> Result<(), String> {
        let _ = self
            .stdin
            .write_all(b"{\"command\":\"shutdown\"}\n")
            .and_then(|_| self.stdin.flush());
        match self
            .child
            .wait_with_timeout(Duration::from_secs(2))
            .map_err(|e| format!("Failed waiting for FluidAudio helper shutdown: {e}"))?
        {
            Some(_) => Ok(()),
            None => self
                .child
                .terminate_tree()
                .map_err(|e| format!("Failed to force-stop FluidAudio helper: {e}")),
        }
    }

    fn send_request(&mut self, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let line = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("Failed to write to FluidAudio helper: {e}"))?;

        let json = loop {
            let mut response = String::new();
            self.stdout
                .read_line(&mut response)
                .map_err(|e| format!("Failed to read FluidAudio helper response: {e}"))?;
            if response.is_empty() {
                let stderr = self
                    .stderr_tail
                    .lock()
                    .map(|v| v.trim().to_string())
                    .unwrap_or_default();
                return Err(format!("FluidAudio helper exited unexpectedly. {stderr}"));
            }
            let trimmed = response.trim();
            if trimmed.starts_with('{') {
                break serde_json::from_str::<serde_json::Value>(trimmed)
                    .map_err(|e| format!("Invalid FluidAudio helper response: {e}"))?;
            }
            if !trimmed.is_empty() {
                log::debug!("[fluidaudio][stdout] {trimmed}");
            }
        };
        if json["ok"].as_bool().unwrap_or(false) {
            Ok(json["result"].clone())
        } else {
            Err(json["error"]
                .as_str()
                .unwrap_or("Unknown FluidAudio helper error.")
                .to_string())
        }
    }
}

impl Drop for FluidAudioWorker {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn encode_samples(samples: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn pump_stderr<F>(mut stderr: impl Read + Send + 'static, tail: Arc<Mutex<String>>, on_line: F)
where
    F: Fn(&str) + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(&mut stderr);
        let mut line = String::new();
        while reader.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
            let trimmed = line.trim_end();
            if !trimmed.is_empty() {
                log::info!("{trimmed}");
                on_line(trimmed);
                if let Ok(mut t) = tail.lock() {
                    t.push_str(trimmed);
                    t.push('\n');
                    if t.len() > 4096 {
                        let cut = t.len() - 4096;
                        let cut = (cut..t.len()).find(|i| t.is_char_boundary(*i)).unwrap_or(0);
                        t.drain(..cut);
                    }
                }
            }
            line.clear();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_samples_is_little_endian_f32() {
        let encoded = encode_samples(&[1.0, -0.5]);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(bytes.len(), 8);
        assert_eq!(f32::from_le_bytes(bytes[0..4].try_into().unwrap()), 1.0);
        assert_eq!(f32::from_le_bytes(bytes[4..8].try_into().unwrap()), -0.5);
    }

    #[test]
    fn stamp_paths_are_per_model() {
        let config = FluidAudioConfig {
            helper_path: PathBuf::from("/nonexistent/contextcall-fluidaudio"),
            runtime_root: PathBuf::from("/tmp/fa"),
            model: "v3".into(),
            language: String::new(),
            diarization_enabled: true,
        };
        assert_eq!(
            config.model_stamp_path(),
            PathBuf::from("/tmp/fa/model-v3.stamp")
        );
        assert!(!config.helper_available());
        assert!(!config.model_ready());
    }

    /// Runs against a real helper build:
    /// CONTEXTCALL_FLUIDAUDIO_HELPER=<path> cargo test -p contextcall-core fluid_audio -- --ignored
    #[test]
    #[ignore]
    fn real_helper_transcribes_and_identifies_speakers() {
        let Some(helper) = std::env::var_os("CONTEXTCALL_FLUIDAUDIO_HELPER") else {
            return;
        };
        let runtime_root = tempfile::tempdir().unwrap();
        let config = FluidAudioConfig {
            helper_path: PathBuf::from(helper),
            runtime_root: runtime_root.path().to_path_buf(),
            model: DEFAULT_MODEL.into(),
            language: String::new(),
            diarization_enabled: true,
        };
        ensure_model(&config, true, |_| {}).unwrap();
        assert!(config.model_ready() && config.diarizer_ready());

        let mut worker = FluidAudioWorker::spawn(&config).unwrap();
        worker.health().unwrap();
        worker.ensure_model(true, true).unwrap();
        let silence = vec![0.0f32; 32_000];
        assert!(worker.transcribe(&silence).unwrap().is_empty());
        assert_eq!(worker.speaker_id(&silence[..8_000]).unwrap(), None);
        worker.clear_speakers().unwrap();
    }
}
