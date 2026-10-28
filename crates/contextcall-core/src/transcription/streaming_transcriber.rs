use crate::transcription::vad::Vad;
use futures::Stream;
use std::sync::mpsc;
use std::sync::{atomic::AtomicBool, Arc};

pub type OnFinal = Box<dyn Fn(String, Option<String>) + Send + 'static>;
pub type OnVolatile = Box<dyn Fn(String) + Send + 'static>;
/// Corrected speaker IDs for earlier finals: (final's 0-based position among the
/// finals this transcriber tagged, new "speaker_N").
pub type OnSpeakerRelabel = Box<dyn Fn(Vec<(usize, String)>) + Send + 'static>;
pub type OnProgress = Arc<dyn Fn(SegmentProgress) + Send + Sync + 'static>;
pub type OnParakeetWorkerIdle =
    Box<dyn Fn(crate::transcription::parakeet::ParakeetWorker) + Send + 'static>;

#[derive(Clone, Copy)]
pub enum SegmentProgress {
    Captured,
    Processed,
}

#[derive(Clone)]
pub enum SttBackend {
    WhisperRs {
        model_path: String,
    },
    FasterWhisper(crate::transcription::faster_whisper::FasterWhisperConfig),
    Parakeet(crate::transcription::parakeet::ParakeetConfig),
    OmniAsr(crate::transcription::omni_asr::OmniAsrConfig),
    CohereTranscribe(crate::transcription::cohere_transcribe::CohereTranscribeConfig),
    /// Gemma 4 12B audio STT served by the managed llama-server instance.
    LlamaServer {
        base_url: String,
    },
    /// Parakeet TDT on CoreML / ANE via the FluidAudio sidecar (macOS).
    FluidAudio(crate::transcription::fluid_audio::FluidAudioConfig),
    Passthrough,
}

/// Per-backend VAD segmentation tuning. Whisper-class models transcribe short
/// hard-cut chunks fine; Gemma 4's audio encoder degrades badly on them, so the
/// llama-server backend uses longer segments and waits for fuller pauses.
#[derive(Clone, Copy)]
struct SegmentationParams {
    silence_end_chunks: usize,
    min_speech_samples: usize,
    flush_samples: usize,
}

impl SegmentationParams {
    fn for_backend(backend: &SttBackend) -> Self {
        match backend {
            SttBackend::LlamaServer { .. } => Self {
                // 1 s pause before closing a segment, 1 s minimum speech, and up
                // to 10 s per segment: Gemma needs natural, uncut phrases —
                // 3 s mid-word flush cuts produce fragments and corrupted text.
                silence_end_chunks: 10,
                min_speech_samples: 16_000,
                flush_samples: 160_000,
            },
            _ => Self {
                silence_end_chunks: Vad::SILENCE_END_CHUNKS,
                min_speech_samples: Vad::MIN_SPEECH_SAMPLES,
                flush_samples: Vad::FLUSH_SAMPLES,
            },
        }
    }
}

/// VAD output. `continues` marks a forced cut mid-speech (the flush cap), as
/// opposed to a segment closed by a pause.
struct Segment {
    samples: Vec<f32>,
    continues: bool,
}

impl Segment {
    fn ended(samples: Vec<f32>) -> Self {
        Self {
            samples,
            continues: false,
        }
    }
}

pub struct StreamingTranscriber {
    on_final: OnFinal,
    backend: SttBackend,
    language: String,
    on_volatile: Option<OnVolatile>,
    on_progress: Option<OnProgress>,
    stop_signal: Option<Arc<AtomicBool>>,
    cancel_on_stop: bool,
    parakeet_worker: Option<crate::transcription::parakeet::ParakeetWorker>,
    parakeet_worker_idle: Option<OnParakeetWorkerIdle>,
    omni_asr_worker: Option<crate::transcription::omni_asr::OmniAsrWorker>,
    cohere_transcribe_worker:
        Option<crate::transcription::cohere_transcribe::CohereTranscribeWorker>,
    diarization_enabled: bool,
    clear_speakers_on_start: bool,
    /// Speaker-embedding sidecar used for diarization when the STT backend has none.
    speaker_tagger: Option<crate::transcription::fluid_audio::FluidAudioConfig>,
    speaker_relabel: Option<OnSpeakerRelabel>,
}

/// Assigns speaker IDs to finalized segments for backends without built-in
/// diarization, using a diarization-only FluidAudio sidecar.
struct SpeakerTagger {
    worker: crate::transcription::fluid_audio::FluidAudioWorker,
    window: SpeakerWindow,
}

impl SpeakerTagger {
    fn spawn(
        config: &crate::transcription::fluid_audio::FluidAudioConfig,
        clear_speakers: bool,
    ) -> Option<Self> {
        let mut worker = match crate::transcription::fluid_audio::FluidAudioWorker::spawn(config) {
            Ok(worker) => worker,
            Err(e) => {
                log::warn!("[diarization] speaker tagger unavailable: {e}");
                return None;
            }
        };
        if let Err(e) = worker.ensure_model(false, true) {
            log::warn!("[diarization] speaker tagger failed to load: {e}");
            return None;
        }
        if clear_speakers {
            let _ = worker.clear_speakers();
        }
        Some(Self {
            worker,
            window: SpeakerWindow::default(),
        })
    }

    fn tag(
        &mut self,
        samples: &[f32],
        continues: bool,
        relabel: Option<&OnSpeakerRelabel>,
    ) -> Option<String> {
        let worker = &mut self.worker;
        if self.window.recluster_due() {
            self.window.recluster(|| worker.recluster(), relabel);
        }
        self.window.tag(samples, continues, |audio| worker.speaker_id(audio))
    }

    fn finish(&mut self, relabel: Option<&OnSpeakerRelabel>) {
        let worker = &mut self.worker;
        self.window.recluster(|| worker.recluster(), relabel);
    }
}

/// Picks the audio used for a speaker embedding, keeps the last confirmed ID, and
/// remembers what each final was labelled so a later recluster can correct it.
///
/// VAD force-cuts long speech every few seconds (`continues`); those pieces are one
/// speaker's uninterrupted turn, so they're embedded together for a steadier
/// embedding. A pause ends the turn, and the next segment starts fresh instead of
/// borrowing the previous speaker's audio.
#[derive(Default)]
struct SpeakerWindow {
    turn: Vec<f32>,
    turn_open: bool,
    last_speaker_id: Option<String>,
    /// Per tagged final, in emit order: helper history index and the label shown.
    tagged: Vec<(Option<usize>, Option<String>)>,
    since_recluster: usize,
}

impl SpeakerWindow {
    /// The embedding model sees at most 10 s.
    const TURN_SAMPLES: usize = 10 * 16_000;
    const RECLUSTER_EVERY: usize = 8;

    fn tag<F>(&mut self, samples: &[f32], continues: bool, identify: F) -> Option<String>
    where
        F: FnOnce(&[f32]) -> Result<crate::transcription::fluid_audio::SpeakerMatch, String>,
    {
        if !self.turn_open {
            self.turn.clear();
        }
        self.turn_open = continues;
        self.turn.extend_from_slice(samples);
        if self.turn.len() > Self::TURN_SAMPLES {
            let excess = self.turn.len() - Self::TURN_SAMPLES;
            self.turn.drain(..excess);
        }
        let (id, index) = match identify(&self.turn) {
            Ok(found) => (found.speaker_id, found.index),
            Err(e) => {
                log::warn!("[diarization] speaker_id error: {e}");
                (None, None)
            }
        };
        if id.is_some() {
            self.last_speaker_id = id.clone();
        }
        let shown = self.last_speaker_id.clone();
        self.tagged.push((index, shown.clone()));
        self.since_recluster += 1;
        shown
    }

    fn recluster_due(&self) -> bool {
        self.since_recluster >= Self::RECLUSTER_EVERY
    }

    /// Applies a whole-session clustering to every final tagged so far and reports
    /// the ones whose label changed. Finals without an embedding follow the final
    /// before them, as they did live.
    fn recluster<F>(&mut self, recluster: F, relabel: Option<&OnSpeakerRelabel>)
    where
        F: FnOnce() -> Result<Vec<Option<String>>, String>,
    {
        if self.since_recluster == 0 {
            return;
        }
        self.since_recluster = 0;
        let labels = match recluster() {
            Ok(labels) => labels,
            Err(e) => {
                log::warn!("[diarization] recluster error: {e}");
                return;
            }
        };
        let mut changes = Vec::new();
        let mut previous: Option<String> = None;
        for (position, (index, shown)) in self.tagged.iter_mut().enumerate() {
            let label = match index.and_then(|i| labels.get(i).cloned().flatten()) {
                Some(label) => Some(label),
                None if index.is_none() => previous.clone().or_else(|| shown.clone()),
                None => shown.clone(),
            };
            if let Some(label) = label.as_ref().filter(|label| shown.as_ref() != Some(*label)) {
                changes.push((position, label.clone()));
                *shown = Some(label.clone());
            }
            previous = shown.clone();
        }
        self.last_speaker_id = previous.or(self.last_speaker_id.take());
        if !changes.is_empty() {
            log::info!("[diarization] recluster relabelled {} segment(s)", changes.len());
            if let Some(relabel) = relabel {
                relabel(changes);
            }
        }
    }
}

impl StreamingTranscriber {
    pub fn new(backend: SttBackend, language: String, on_final: OnFinal) -> Self {
        Self {
            on_final,
            backend,
            language,
            on_volatile: None,
            on_progress: None,
            stop_signal: None,
            cancel_on_stop: false,
            parakeet_worker: None,
            parakeet_worker_idle: None,
            omni_asr_worker: None,
            cohere_transcribe_worker: None,
            diarization_enabled: false,
            clear_speakers_on_start: false,
            speaker_tagger: None,
            speaker_relabel: None,
        }
    }

    /// Test-only: passthrough mode that skips actual transcription.
    pub fn new_passthrough(on_final: OnFinal) -> Self {
        Self {
            on_final,
            backend: SttBackend::Passthrough,
            language: "en".into(),
            on_volatile: None,
            on_progress: None,
            stop_signal: None,
            cancel_on_stop: false,
            parakeet_worker: None,
            parakeet_worker_idle: None,
            omni_asr_worker: None,
            cohere_transcribe_worker: None,
            diarization_enabled: false,
            clear_speakers_on_start: false,
            speaker_tagger: None,
            speaker_relabel: None,
        }
    }

    /// Attach a pre-warmed Parakeet worker so the model is already loaded when recording starts.
    pub fn with_parakeet_worker(
        mut self,
        worker: crate::transcription::parakeet::ParakeetWorker,
    ) -> Self {
        self.parakeet_worker = Some(worker);
        self
    }

    pub fn with_parakeet_worker_idle(mut self, on_idle: OnParakeetWorkerIdle) -> Self {
        self.parakeet_worker_idle = Some(on_idle);
        self
    }

    /// Attach a pre-warmed OmniASR worker so the model is already loaded when recording starts.
    pub fn with_omni_asr_worker(
        mut self,
        worker: crate::transcription::omni_asr::OmniAsrWorker,
    ) -> Self {
        self.omni_asr_worker = Some(worker);
        self
    }

    pub fn with_cohere_transcribe_worker(
        mut self,
        worker: crate::transcription::cohere_transcribe::CohereTranscribeWorker,
    ) -> Self {
        self.cohere_transcribe_worker = Some(worker);
        self
    }

    /// Builder: attach a volatile (in-progress) speech callback.
    pub fn with_volatile(mut self, on_volatile: OnVolatile) -> Self {
        self.on_volatile = Some(on_volatile);
        self
    }

    pub fn with_progress(mut self, on_progress: OnProgress) -> Self {
        self.on_progress = Some(on_progress);
        self
    }

    pub fn with_stop_signal(mut self, stop_signal: Arc<AtomicBool>) -> Self {
        self.stop_signal = Some(stop_signal);
        self
    }

    pub fn with_cancel_on_stop(mut self, enabled: bool) -> Self {
        self.cancel_on_stop = enabled;
        self
    }

    pub fn with_diarization(mut self, enabled: bool) -> Self {
        self.diarization_enabled = enabled;
        self
    }

    pub fn with_clear_speakers_on_start(mut self, enabled: bool) -> Self {
        self.clear_speakers_on_start = enabled;
        self
    }

    /// Receives corrected speaker IDs for earlier finals when the FluidAudio
    /// diarizer re-clusters the session. Positions count finals from this transcriber.
    pub fn with_speaker_relabel(mut self, on_relabel: OnSpeakerRelabel) -> Self {
        self.speaker_relabel = Some(on_relabel);
        self
    }

    /// Diarize backends that have no speaker ID of their own via the FluidAudio
    /// sidecar. Ignored unless diarization is enabled.
    pub fn with_speaker_tagger(
        mut self,
        config: Option<crate::transcription::fluid_audio::FluidAudioConfig>,
    ) -> Self {
        self.speaker_tagger = config;
        self
    }

    pub async fn run<S>(self, stream: S)
    where
        S: Stream<Item = Vec<f32>> + Send + 'static,
    {
        use futures::StreamExt;

        let (seg_tx, seg_rx) = mpsc::sync_channel::<Segment>(30);
        let on_final = self.on_final;
        let on_volatile = self.on_volatile;
        let on_progress = self.on_progress;
        let progress_for_backend = on_progress.clone();
        let language = self.language.clone();
        let backend = self.backend.clone();
        let stop_signal = self.stop_signal;
        let cancel_on_stop = self.cancel_on_stop;
        let prewarmed_parakeet = self.parakeet_worker;
        let on_parakeet_worker_idle = self.parakeet_worker_idle;
        let prewarmed_omni_asr = self.omni_asr_worker;
        let prewarmed_cohere = self.cohere_transcribe_worker;

        // Run Whisper on a blocking thread-pool thread so that joining it is
        // async-friendly. Using std::thread::join() inside an async fn would
        // block a tokio worker, causing handle.await in stop_transcription to
        // return before the drain completes and letting is_running flip to
        // false while segments are still being transcribed.
        let diarization_enabled = self.diarization_enabled;
        let clear_speakers_on_start = self.clear_speakers_on_start;
        let speaker_tagger_config = self.speaker_tagger;
        let speaker_relabel = self.speaker_relabel;

        let whisper_task = tokio::task::spawn_blocking(move || {
            let backend_diarizes = matches!(
                backend,
                SttBackend::Parakeet(_) | SttBackend::FluidAudio(_) | SttBackend::Passthrough
            );
            let mut speaker_tagger = match speaker_tagger_config {
                Some(config) if diarization_enabled && !backend_diarizes => {
                    SpeakerTagger::spawn(&config, clear_speakers_on_start)
                }
                _ => None,
            };
            let mut tag_speaker = |samples: &[f32], continues: bool| {
                speaker_tagger
                    .as_mut()
                    .and_then(|t| t.tag(samples, continues, speaker_relabel.as_ref()))
            };
            match backend {
                SttBackend::WhisperRs { model_path } => {
                    match crate::transcription::whisper::WhisperManager::new(&model_path, &language)
                    {
                        Ok(manager) => {
                            let mut state = match manager.create_state() {
                                Ok(s) => s,
                                Err(e) => {
                                    log::error!("whisper state: {e}");
                                    return;
                                }
                            };
                            for Segment { samples, continues } in seg_rx.iter() {
                                let text =
                                    crate::transcription::whisper::WhisperManager::transcribe(
                                        &mut state, &samples, &language,
                                    );
                                if let Some(ref on_progress) = progress_for_backend {
                                    on_progress(SegmentProgress::Processed);
                                }
                                if !text.is_empty() {
                                    log::info!(
                                        "[transcriber] {}",
                                        text.chars().take(80).collect::<String>()
                                    );
                                    on_final(text, tag_speaker(&samples, continues));
                                }
                            }
                        }
                        Err(e) => log::error!("Failed to load whisper model: {e}"),
                    }
                }
                SttBackend::FasterWhisper(config) => {
                    match crate::transcription::faster_whisper::FasterWhisperWorker::spawn(&config)
                    {
                        Ok(mut worker) => {
                            for Segment { samples, continues } in seg_rx.iter() {
                                match worker.transcribe(&samples, &language) {
                                    Ok(text) if !text.is_empty() => {
                                        if let Some(ref on_progress) = progress_for_backend {
                                            on_progress(SegmentProgress::Processed);
                                        }
                                        log::info!(
                                            "[transcriber] {}",
                                            text.chars().take(80).collect::<String>()
                                        );
                                        on_final(text, tag_speaker(&samples, continues));
                                    }
                                    Ok(_) => {
                                        if let Some(ref on_progress) = progress_for_backend {
                                            on_progress(SegmentProgress::Processed);
                                        }
                                    }
                                    Err(e) => log::error!("faster-whisper transcribe error: {e}"),
                                }
                            }
                        }
                        Err(e) => log::error!("Failed to launch faster-whisper worker: {e}"),
                    }
                }
                SttBackend::Parakeet(config) => {
                    // Acquire a ready worker: use the pre-warmed one if healthy, otherwise
                    // spawn fresh. The pre-warmed Python process can die between warmup and
                    // recording start (e.g. OS memory pressure), so we verify it with
                    // ensure_model and fall back to a new spawn on failure.
                    let worker_result = if let Some(mut w) = prewarmed_parakeet {
                        match w.ensure_model(config.diarization_enabled) {
                            Ok(_) => {
                                log::info!("[parakeet] using pre-warmed worker");
                                Ok(w)
                            }
                            Err(e) => {
                                log::warn!(
                                    "[parakeet] pre-warmed worker unhealthy ({e}), spawning fresh"
                                );
                                crate::transcription::parakeet::ParakeetWorker::spawn(&config)
                            }
                        }
                    } else {
                        crate::transcription::parakeet::ParakeetWorker::spawn(&config)
                    };
                    match worker_result {
                        Ok(mut worker) => {
                            // Eagerly load the model now (no-op if pre-warmed, otherwise loads
                            // in parallel with audio capture so it's ready for the first segment).
                            if let Err(e) = worker.ensure_model(config.diarization_enabled) {
                                log::error!("parakeet ensure_model failed: {e}");
                                return;
                            }
                            if clear_speakers_on_start {
                                if let Err(e) = worker.clear_speakers() {
                                    log::warn!("[diarization] clear_speakers failed: {e}");
                                }
                            }
                            // Rolling buffer of recent speech for speaker ID.
                            // TitaNet needs 3–4 s of audio for stable embeddings;
                            // using only the current short VAD segment causes every
                            // segment to get a new speaker ID.
                            const DIARIZATION_BUF_SAMPLES: usize = 4 * 16_000; // 4 s at 16 kHz
                            let mut diarization_buf: Vec<f32> = Vec::new();
                            // Last confirmed speaker ID — used as fallback when the buffer
                            // is too short for a reliable embedding (avoids "Speaker A").
                            let mut last_speaker_id: Option<String> = None;

                            for Segment { samples, .. } in seg_rx.iter() {
                                if diarization_enabled {
                                    diarization_buf.extend_from_slice(&samples);
                                    if diarization_buf.len() > DIARIZATION_BUF_SAMPLES {
                                        let excess =
                                            diarization_buf.len() - DIARIZATION_BUF_SAMPLES;
                                        diarization_buf.drain(..excess);
                                    }
                                }
                                match worker.transcribe(&samples) {
                                    Ok(text) if !text.is_empty() => {
                                        if let Some(ref on_progress) = progress_for_backend {
                                            on_progress(SegmentProgress::Processed);
                                        }
                                        log::info!(
                                            "[transcriber] {}",
                                            text.chars().take(80).collect::<String>()
                                        );
                                        let speaker_id = if diarization_enabled {
                                            match worker.speaker_id(&diarization_buf) {
                                                Ok(Some(id)) => {
                                                    last_speaker_id = Some(id.clone());
                                                    Some(id)
                                                }
                                                Ok(None) => last_speaker_id.clone(),
                                                Err(e) => {
                                                    log::warn!(
                                                        "[diarization] speaker_id error: {e}"
                                                    );
                                                    last_speaker_id.clone()
                                                }
                                            }
                                        } else {
                                            None
                                        };
                                        on_final(text, speaker_id);
                                    }
                                    Ok(_) => {
                                        if let Some(ref on_progress) = progress_for_backend {
                                            on_progress(SegmentProgress::Processed);
                                        }
                                    }
                                    Err(e) => log::error!("parakeet transcribe error: {e}"),
                                }
                            }
                            let worker_is_healthy = worker.health().is_ok();
                            if worker_is_healthy {
                                if let Some(on_idle) = on_parakeet_worker_idle {
                                    on_idle(worker);
                                }
                            } else {
                                log::warn!("[parakeet] worker unhealthy after session; dropping");
                            }
                        }
                        Err(e) => log::error!("Failed to launch parakeet worker: {e}"),
                    }
                }
                SttBackend::OmniAsr(config) => {
                    // Acquire a ready worker: use the pre-warmed one if healthy, otherwise
                    // spawn fresh. The pre-warmed Python process can die between warmup and
                    // recording start, so we verify it with ensure_model first.
                    let worker_result = if let Some(mut w) = prewarmed_omni_asr {
                        match w.ensure_model() {
                            Ok(_) => {
                                log::info!("[omni-asr] using pre-warmed worker");
                                Ok(w)
                            }
                            Err(e) => {
                                log::warn!(
                                    "[omni-asr] pre-warmed worker unhealthy ({e}), spawning fresh"
                                );
                                crate::transcription::omni_asr::OmniAsrWorker::spawn(&config)
                            }
                        }
                    } else {
                        crate::transcription::omni_asr::OmniAsrWorker::spawn(&config)
                    };
                    match worker_result {
                        Ok(mut worker) => {
                            // No-op if pre-warmed; otherwise loads the 1+ GB checkpoint before
                            // the first segment arrives so transcription isn't delayed.
                            if let Err(e) = worker.ensure_model() {
                                log::error!("omni-asr ensure_model failed: {e}");
                            }
                            for Segment { samples, continues } in seg_rx.iter() {
                                match worker.transcribe(&samples, &language) {
                                    Ok(text) if !text.is_empty() => {
                                        if let Some(ref on_progress) = progress_for_backend {
                                            on_progress(SegmentProgress::Processed);
                                        }
                                        log::info!(
                                            "[transcriber] {}",
                                            text.chars().take(80).collect::<String>()
                                        );
                                        on_final(text, tag_speaker(&samples, continues));
                                    }
                                    Ok(_) => {
                                        if let Some(ref on_progress) = progress_for_backend {
                                            on_progress(SegmentProgress::Processed);
                                        }
                                    }
                                    Err(e) => log::error!("omni-asr transcribe error: {e}"),
                                }
                            }
                        }
                        Err(e) => log::error!("Failed to launch omni-asr worker: {e}"),
                    }
                }
                SttBackend::CohereTranscribe(config) => {
                    let worker_result = if let Some(mut w) = prewarmed_cohere {
                        match w.ensure_model() {
                            Ok(_) => {
                                log::info!("[cohere-transcribe] using pre-warmed worker");
                                Ok(w)
                            }
                            Err(e) => {
                                log::warn!(
                                    "[cohere-transcribe] pre-warmed worker unhealthy ({e}), spawning fresh"
                                );
                                crate::transcription::cohere_transcribe::CohereTranscribeWorker::spawn(&config)
                            }
                        }
                    } else {
                        crate::transcription::cohere_transcribe::CohereTranscribeWorker::spawn(
                            &config,
                        )
                    };
                    match worker_result {
                        Ok(mut worker) => {
                            if let Err(e) = worker.ensure_model() {
                                log::error!("cohere-transcribe ensure_model failed: {e}");
                                return;
                            }
                            for Segment { samples, continues } in seg_rx.iter() {
                                match worker.transcribe(&samples, &language) {
                                    Ok(text) if !text.is_empty() => {
                                        if let Some(ref on_progress) = progress_for_backend {
                                            on_progress(SegmentProgress::Processed);
                                        }
                                        log::info!(
                                            "[transcriber] {}",
                                            text.chars().take(80).collect::<String>()
                                        );
                                        on_final(text, tag_speaker(&samples, continues));
                                    }
                                    Ok(_) => {
                                        if let Some(ref on_progress) = progress_for_backend {
                                            on_progress(SegmentProgress::Processed);
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("cohere-transcribe transcribe error: {e}")
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to launch cohere-transcribe worker: {e}")
                        }
                    }
                }
                SttBackend::LlamaServer { base_url } => {
                    // The engine starts the server in the background; wait here so
                    // segments captured during model load are buffered, not dropped.
                    if let Err(e) = crate::llama_server::wait_healthy_blocking(
                        &base_url,
                        std::time::Duration::from_secs(240),
                    ) {
                        log::error!("llama-server STT unavailable: {e}");
                        return;
                    }
                    for Segment { samples, continues } in seg_rx.iter() {
                        match crate::llama_server::transcribe_segment_blocking(
                            &base_url, &samples, &language,
                        ) {
                            Ok(text) if !text.is_empty() => {
                                if let Some(ref on_progress) = progress_for_backend {
                                    on_progress(SegmentProgress::Processed);
                                }
                                log::info!(
                                    "[transcriber] {}",
                                    text.chars().take(80).collect::<String>()
                                );
                                on_final(text, tag_speaker(&samples, continues));
                            }
                            Ok(_) => {
                                if let Some(ref on_progress) = progress_for_backend {
                                    on_progress(SegmentProgress::Processed);
                                }
                            }
                            Err(e) => log::error!("llama-server transcribe error: {e}"),
                        }
                    }
                }
                SttBackend::FluidAudio(config) => {
                    let mut worker =
                        match crate::transcription::fluid_audio::FluidAudioWorker::spawn(&config) {
                            Ok(worker) => worker,
                            Err(e) => {
                                log::error!("Failed to launch FluidAudio helper: {e}");
                                return;
                            }
                        };
                    let diarize = diarization_enabled && config.diarization_enabled;
                    if let Err(e) = worker.ensure_model(true, diarize) {
                        log::error!("FluidAudio ensure_model failed: {e}");
                        return;
                    }
                    if diarize && clear_speakers_on_start {
                        if let Err(e) = worker.clear_speakers() {
                            log::warn!("[diarization] clear_speakers failed: {e}");
                        }
                    }
                    let mut window = SpeakerWindow::default();
                    let mut stitcher = crate::transcription::fluid_audio::SeamStitcher::default();
                    for Segment { samples, continues } in seg_rx.iter() {
                        let audio = stitcher.window(&samples);
                        let result = worker
                            .transcribe_words(&audio)
                            .map(|words| stitcher.accept(audio, &words, continues));
                        match result {
                            Ok(text) if !text.is_empty() => {
                                if let Some(ref on_progress) = progress_for_backend {
                                    on_progress(SegmentProgress::Processed);
                                }
                                log::info!(
                                    "[transcriber] {}",
                                    text.chars().take(80).collect::<String>()
                                );
                                let speaker_id = if diarize {
                                    if window.recluster_due() {
                                        window.recluster(
                                            || worker.recluster(),
                                            speaker_relabel.as_ref(),
                                        );
                                    }
                                    window.tag(&samples, continues, |audio| {
                                        worker.speaker_id(audio)
                                    })
                                } else {
                                    None
                                };
                                on_final(text, speaker_id);
                            }
                            Ok(_) => {
                                if let Some(ref on_progress) = progress_for_backend {
                                    on_progress(SegmentProgress::Processed);
                                }
                            }
                            Err(e) => log::error!("FluidAudio transcribe error: {e}"),
                        }
                    }
                    if diarize {
                        window.recluster(|| worker.recluster(), speaker_relabel.as_ref());
                    }
                }
                SttBackend::Passthrough => for _ in seg_rx.iter() {},
            }
            if let Some(tagger) = speaker_tagger.as_mut() {
                tagger.finish(speaker_relabel.as_ref());
            }
        });

        let seg_params = SegmentationParams::for_backend(&self.backend);
        let mut vad = Vad::new();
        let mut vad_buf: Vec<f32> = Vec::new();
        let mut pending_speech_buf: Vec<f32> = Vec::new();
        let mut speech_buf: Vec<f32> = Vec::new();
        let mut speaking = false;
        let mut active_start_count = 0usize;
        let mut silence_count = 0usize;
        let mut last_volatile_at = std::time::Instant::now();
        let volatile_interval = std::time::Duration::from_millis(500);
        // Fallback: count samples processed while speaking to fire volatile
        // even when wall-clock elapsed is near-zero (e.g. in fast tests).
        let volatile_sample_interval: usize = 8_000; // 500 ms at 16 kHz
        let mut speaking_samples_since_volatile: usize = 0;

        let mut stream = Box::pin(stream);
        // Stop is signalled by the upstream sender being dropped (mic capture
        // sets tx_opt=None when stop_requested fires), which causes
        // stream.next() to return None.  We no longer break early on the stop
        // signal; instead we let the stream drain fully so every captured
        // chunk reaches VAD and Whisper before the pipeline shuts down.
        while let Some(samples) = stream.next().await {
            if cancel_on_stop
                && stop_signal
                    .as_ref()
                    .map(|signal| signal.load(std::sync::atomic::Ordering::Relaxed))
                    .unwrap_or(false)
            {
                break;
            }
            vad_buf.extend_from_slice(&samples);

            while vad_buf.len() >= Vad::CHUNK_SIZE {
                let chunk: Vec<f32> = vad_buf.drain(..Vad::CHUNK_SIZE).collect();
                let active = vad.process_chunk(&chunk);

                if active {
                    silence_count = 0;
                    if speaking {
                        speech_buf.extend_from_slice(&chunk);
                    } else {
                        active_start_count += 1;
                        pending_speech_buf.extend_from_slice(&chunk);
                        if active_start_count >= Vad::MIN_START_CHUNKS {
                            speaking = true;
                            speech_buf.append(&mut pending_speech_buf);
                        }
                    }
                } else if speaking {
                    silence_count += 1;
                    speech_buf.extend_from_slice(&chunk);

                    if silence_count >= seg_params.silence_end_chunks {
                        speaking = false;
                        silence_count = 0;
                        active_start_count = 0;
                        pending_speech_buf.clear();
                        if speech_buf.len() > seg_params.min_speech_samples {
                            match seg_tx.try_send(Segment::ended(std::mem::take(&mut speech_buf))) {
                                Ok(_) => {
                                    if let Some(ref on_progress) = on_progress {
                                        on_progress(SegmentProgress::Captured);
                                    }
                                }
                                Err(e) => {
                                    log::warn!("[vad] seg dropped (end-of-speech): {e}");
                                }
                            }
                        } else {
                            speech_buf.clear();
                        }
                    }
                } else {
                    active_start_count = 0;
                    pending_speech_buf.clear();
                }

                if speaking && speech_buf.len() >= seg_params.flush_samples {
                    let segment = Segment {
                        samples: std::mem::take(&mut speech_buf),
                        continues: true,
                    };
                    match seg_tx.try_send(segment) {
                        Ok(_) => {
                            if let Some(ref on_progress) = on_progress {
                                on_progress(SegmentProgress::Captured);
                            }
                        }
                        Err(e) => {
                            log::warn!("[vad] seg dropped (flush): {e}");
                        }
                    }
                }

                // Emit volatile text while speech is active
                if speaking {
                    speaking_samples_since_volatile += Vad::CHUNK_SIZE;
                    if let Some(ref on_vol) = on_volatile {
                        let time_elapsed = last_volatile_at.elapsed() >= volatile_interval;
                        let samples_elapsed =
                            speaking_samples_since_volatile >= volatile_sample_interval;
                        if time_elapsed || samples_elapsed {
                            on_vol("...".to_string());
                            last_volatile_at = std::time::Instant::now();
                            speaking_samples_since_volatile = 0;
                        }
                    }
                } else {
                    speaking_samples_since_volatile = 0;
                }
            }
        }

        // Flush remainder
        if speech_buf.len() > seg_params.min_speech_samples {
            if seg_tx.send(Segment::ended(speech_buf)).is_ok() {
                if let Some(ref on_progress) = on_progress {
                    on_progress(SegmentProgress::Captured);
                }
            }
        }

        drop(seg_tx);
        let _ = whisper_task.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::fluid_audio::SpeakerMatch;
    use futures::stream;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn silence_produces_no_transcription() {
        let (tx, rx) = std::sync::mpsc::channel();
        let on_final = move |text: String, _speaker_id: Option<String>| {
            tx.send(text).ok();
        };
        let transcriber = StreamingTranscriber::new_passthrough(Box::new(on_final));
        let silence: Vec<Vec<f32>> = (0..30).map(|_| vec![0.0f32; 1600]).collect();
        transcriber.run(stream::iter(silence)).await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn parakeet_passthrough_speaker_id_is_none() {
        // Passthrough backend always passes None as speaker_id
        let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
        let on_final = Box::new(move |_text: String, speaker_id: Option<String>| {
            tx.send(speaker_id).ok();
        });
        // Passthrough mode is used in tests — it won't produce transcriptions
        // This test just validates the closure type compiles correctly.
        // Actual diarization path is tested via integration.
        drop(on_final);
        drop(rx);
    }

    #[tokio::test]
    async fn volatile_fires_while_speaking() {
        use std::sync::{Arc, Mutex};
        let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_clone = Arc::clone(&calls);
        let on_final = Box::new(|_text: String, _speaker_id: Option<String>| {});
        let on_volatile = Box::new(move |text: String| {
            calls_clone.lock().unwrap().push(text);
        });
        let transcriber =
            StreamingTranscriber::new_passthrough(on_final).with_volatile(on_volatile);

        // 2 seconds of continuous speech-level audio (non-silence) at 16kHz
        let speech_chunk: Vec<f32> = (0..1600).map(|i| (i as f32 / 100.0).sin() * 0.5).collect();
        let chunks: Vec<Vec<f32>> = (0..200).map(|_| speech_chunk.clone()).collect();
        transcriber.run(futures::stream::iter(chunks)).await;
        assert!(
            !calls.lock().unwrap().is_empty(),
            "volatile should have fired at least once"
        );
    }

    #[tokio::test]
    async fn short_noise_burst_does_not_capture_segment() {
        let captured = Arc::new(AtomicUsize::new(0));
        let captured_clone = Arc::clone(&captured);
        let on_final = Box::new(|_text: String, _speaker_id: Option<String>| {});
        let on_progress = Arc::new(move |progress: SegmentProgress| {
            if matches!(progress, SegmentProgress::Captured) {
                captured_clone.fetch_add(1, Ordering::Relaxed);
            }
        });

        let transcriber =
            StreamingTranscriber::new_passthrough(on_final).with_progress(on_progress);
        let speech_chunk: Vec<f32> = (0..1600).map(|i| (i as f32 / 100.0).sin() * 0.5).collect();
        let silence_chunk = vec![0.0f32; 1600];
        let chunks = vec![
            speech_chunk,
            silence_chunk.clone(),
            silence_chunk.clone(),
            silence_chunk.clone(),
            silence_chunk.clone(),
            silence_chunk.clone(),
            silence_chunk.clone(),
            silence_chunk.clone(),
            silence_chunk,
        ];

        transcriber.run(futures::stream::iter(chunks)).await;
        assert_eq!(captured.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn sustained_speech_still_captures_segment() {
        let captured = Arc::new(AtomicUsize::new(0));
        let captured_clone = Arc::clone(&captured);
        let on_final = Box::new(|_text: String, _speaker_id: Option<String>| {});
        let on_progress = Arc::new(move |progress: SegmentProgress| {
            if matches!(progress, SegmentProgress::Captured) {
                captured_clone.fetch_add(1, Ordering::Relaxed);
            }
        });

        let transcriber =
            StreamingTranscriber::new_passthrough(on_final).with_progress(on_progress);
        let speech_chunk: Vec<f32> = (0..1600).map(|i| (i as f32 / 100.0).sin() * 0.5).collect();
        let silence_chunk = vec![0.0f32; 1600];
        let mut chunks = vec![speech_chunk.clone(); 6];
        chunks.extend(vec![silence_chunk; 8]);

        transcriber.run(futures::stream::iter(chunks)).await;
        assert_eq!(captured.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn stop_signal_can_cancel_stream_when_enabled() {
        let stop = Arc::new(AtomicBool::new(true));
        let captured = Arc::new(AtomicUsize::new(0));
        let captured_clone = Arc::clone(&captured);
        let on_progress = Arc::new(move |progress: SegmentProgress| {
            if matches!(progress, SegmentProgress::Captured) {
                captured_clone.fetch_add(1, Ordering::Relaxed);
            }
        });
        let transcriber = StreamingTranscriber::new_passthrough(Box::new(|_, _| {}))
            .with_progress(on_progress)
            .with_stop_signal(stop)
            .with_cancel_on_stop(true);
        let speech_chunk: Vec<f32> = (0..1600).map(|i| (i as f32 / 100.0).sin() * 0.5).collect();
        let chunks: Vec<Vec<f32>> = (0..100).map(|_| speech_chunk.clone()).collect();

        transcriber.run(stream::iter(chunks)).await;

        assert_eq!(captured.load(Ordering::Relaxed), 0);
    }

    fn found(id: &str, index: usize) -> Result<SpeakerMatch, String> {
        Ok(SpeakerMatch {
            speaker_id: Some(id.into()),
            index: Some(index),
        })
    }

    #[test]
    fn speaker_window_embeds_a_whole_turn_but_not_the_previous_one() {
        let mut window = SpeakerWindow::default();
        let mut seen = Vec::new();
        // One speaker's turn force-cut twice, then a pause, then a new turn.
        for continues in [true, true, false, false] {
            window.tag(&vec![0.1; 48_000], continues, |audio| {
                seen.push(audio.len());
                Ok(SpeakerMatch::default())
            });
        }
        assert_eq!(seen, [48_000, 96_000, 144_000, 48_000]);
    }

    #[test]
    fn speaker_window_caps_a_turn_at_the_embedding_window() {
        let mut window = SpeakerWindow::default();
        let mut seen = 0;
        for _ in 0..5 {
            window.tag(&vec![0.1; 48_000], true, |audio| {
                seen = audio.len();
                Ok(SpeakerMatch::default())
            });
        }
        assert_eq!(seen, SpeakerWindow::TURN_SAMPLES);
    }

    #[test]
    fn speaker_window_falls_back_to_last_confirmed_speaker() {
        let mut window = SpeakerWindow::default();
        assert_eq!(window.tag(&[0.0; 10], false, |_| Ok(SpeakerMatch::default())), None);
        assert_eq!(
            window.tag(&[0.0; 10], false, |_| found("speaker_2", 0)),
            Some("speaker_2".into())
        );
        assert_eq!(
            window.tag(&[0.0; 10], false, |_| Ok(SpeakerMatch::default())),
            Some("speaker_2".into())
        );
        assert_eq!(
            window.tag(&[0.0; 10], false, |_| Err("boom".into())),
            Some("speaker_2".into())
        );
    }

    #[test]
    fn speaker_window_recluster_reports_only_changed_finals() {
        let mut window = SpeakerWindow::default();
        window.tag(&[0.0; 10], false, |_| found("speaker_0", 0));
        window.tag(&[0.0; 10], false, |_| found("speaker_3", 1));
        // Too short to embed: shown with the previous label, follows it on recluster.
        window.tag(&[0.0; 10], false, |_| Ok(SpeakerMatch::default()));
        window.tag(&[0.0; 10], false, |_| found("speaker_1", 2));

        let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&changes);
        let relabel: OnSpeakerRelabel = Box::new(move |c| sink.lock().unwrap().extend(c));
        window.recluster(
            || {
                Ok(vec![
                    Some("speaker_0".into()),
                    Some("speaker_0".into()),
                    Some("speaker_1".into()),
                ])
            },
            Some(&relabel),
        );
        assert_eq!(
            *changes.lock().unwrap(),
            [(1, "speaker_0".to_string()), (2, "speaker_0".to_string())]
        );
        assert!(!window.recluster_due());
        // Nothing new since: no second pass.
        window.recluster(|| panic!("should not recluster"), Some(&relabel));
    }

    /// Plays raw 16 kHz f32 PCM through VAD + FluidAudio in real time, printing each
    /// final with its arrival time:
    /// CONTEXTCALL_FLUIDAUDIO_HELPER=<helper> CONTEXTCALL_FLUIDAUDIO_PCM=<file.f32> \
    ///   CONTEXTCALL_FLUIDAUDIO_LANG=es cargo test -p contextcall-core fluid_audio_live -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn fluid_audio_live_stitching() {
        let (Some(helper), Some(pcm)) = (
            std::env::var_os("CONTEXTCALL_FLUIDAUDIO_HELPER"),
            std::env::var_os("CONTEXTCALL_FLUIDAUDIO_PCM"),
        ) else {
            return;
        };
        let bytes = std::fs::read(pcm).unwrap();
        let samples: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let language = std::env::var("CONTEXTCALL_FLUIDAUDIO_LANG").unwrap_or_default();
        let config = crate::transcription::fluid_audio::FluidAudioConfig {
            helper_path: helper.into(),
            runtime_root: tempfile::tempdir().unwrap().keep(),
            model: crate::transcription::fluid_audio::DEFAULT_MODEL.into(),
            language: language.clone(),
            diarization_enabled: false,
        };
        let started = std::time::Instant::now();
        let on_final = Box::new(move |text: String, _: Option<String>| {
            println!("[{:5.1}s] {text}", started.elapsed().as_secs_f64());
        });
        let chunks: Vec<Vec<f32>> = samples.chunks(1600).map(|c| c.to_vec()).collect();
        let paced = stream::unfold(chunks.into_iter(), |mut it| async move {
            let chunk = it.next()?;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Some((chunk, it))
        });
        StreamingTranscriber::new(SttBackend::FluidAudio(config), language, on_final)
            .run(paced)
            .await;
    }
}
