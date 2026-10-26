// JSON-lines worker for ContextCall, mirroring the protocol of parakeet_worker.py:
// one request object per stdin line, one {"ok": ..., "result"|"error": ...} per stdout line.
// Logs and progress go to stderr so stdout stays a clean response channel.
//
// Commands:
//   health
//   ensure_model   {model: "v3"|"ultra", asr: bool, diarization: bool}
//   transcribe     {samples_b64, language?} -> {text, words: [{text, start, end}]}
//                  (language: ISO 639-1, e.g. "en"; word times in seconds from the start)
//   speaker_id     {samples_b64}            -> {speaker_id: "speaker_N" | null}
//   clear_speakers
//   shutdown
//
// samples_b64 is base64 of little-endian f32 PCM, 16 kHz mono.
import CoreML
import Foundation
import FluidAudio

let minSpeakerIdSamples = 16_000  // 1.0 s at 16 kHz, same floor as the Python worker

var asrManager: AsrManager?
var asrVersionName: String?
var diarizer: DiarizerManager?
var speakers = SpeakerManager()

func log(_ message: String) {
    FileHandle.standardError.write(Data("[fluidaudio] \(message)\n".utf8))
}

func emit(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object) else { return }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

func ok(_ result: [String: Any]) { emit(["ok": true, "result": result]) }
func fail(_ message: String) { emit(["ok": false, "error": message]) }

func decodeSamples(_ payload: [String: Any]) throws -> [Float] {
    guard let b64 = payload["samples_b64"] as? String, let data = Data(base64Encoded: b64) else {
        throw NSError(
            domain: "contextcall-fluidaudio", code: 1,
            userInfo: [NSLocalizedDescriptionKey: "missing or invalid samples_b64"])
    }
    return data.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
}

func asrVersion(_ name: String) -> AsrModelVersion {
    switch name {
    case "ultra": return .ultra
    case "v2": return .v2
    default: return .v3
    }
}

let progressHandler: ProgressHandler = { progress in
    switch progress.phase {
    case .listing:
        log("progress listing")
    case .downloading(let done, let total):
        log("progress downloading \(done)/\(total) \(Int(progress.fractionCompleted * 100))%")
    case .compiling(let name):
        log("progress compiling \(name)")
    }
}

@MainActor
func ensureModel(_ payload: [String: Any]) async throws {
    let modelName = payload["model"] as? String ?? "v3"
    if payload["asr"] as? Bool ?? true, asrManager == nil || asrVersionName != modelName {
        let started = Date()
        let models = try await AsrModels.downloadAndLoad(
            version: asrVersion(modelName), progressHandler: progressHandler)
        let manager = AsrManager()
        try await manager.loadModels(models)
        asrManager = manager
        asrVersionName = modelName
        log("asr \(modelName) loaded in \(String(format: "%.1f", Date().timeIntervalSince(started)))s")
    }
    if payload["diarization"] as? Bool ?? false, diarizer == nil {
        let started = Date()
        let models = try await DiarizerModels.downloadIfNeeded(progressHandler: progressHandler)
        let manager = DiarizerManager()
        manager.initialize(models: models)
        diarizer = manager
        log("diarizer loaded in \(String(format: "%.1f", Date().timeIntervalSince(started)))s")
    }
}

@MainActor
func transcribe(_ samples: [Float], language: Language?) async throws -> ASRResult {
    guard let asr = asrManager else {
        throw NSError(
            domain: "contextcall-fluidaudio", code: 2,
            userInfo: [NSLocalizedDescriptionKey: "ASR model not loaded; call ensure_model first"])
    }
    var state = TdtDecoderState.make(decoderLayers: await asr.decoderLayerCount)
    // The hint filters decoder tokens to the language's script (plus an
    // English-token blocklist for "fr"). It does not separate Spanish from
    // English, which share a script, so segment length matters more there.
    return try await asr.transcribe(samples, decoderState: &state, language: language)
}

/// Groups sub-word tokens into words (a leading space starts a new word) so the
/// caller can stitch overlapping windows by time.
func words(_ timings: [TokenTiming]) -> [[String: Any]] {
    var words: [[String: Any]] = []
    for timing in timings {
        let piece = timing.token
        if piece.hasPrefix(" ") || words.isEmpty {
            let text = piece.trimmingCharacters(in: .whitespaces)
            if text.isEmpty { continue }
            words.append(["text": text, "start": timing.startTime, "end": timing.endTime])
        } else {
            words[words.count - 1]["text"] = (words[words.count - 1]["text"] as? String ?? "") + piece
            words[words.count - 1]["end"] = timing.endTime
        }
    }
    return words
}

@MainActor
func speakerId(_ samples: [Float]) throws -> String? {
    guard samples.count >= minSpeakerIdSamples else { return nil }
    guard let diarizer else {
        throw NSError(
            domain: "contextcall-fluidaudio", code: 3,
            userInfo: [NSLocalizedDescriptionKey: "diarizer not loaded; call ensure_model with diarization"])
    }
    let embedding = try diarizer.extractSpeakerEmbedding(from: samples)
    let duration = Float(samples.count) / 16_000
    guard let speaker = speakers.assignSpeaker(embedding, speechDuration: duration) else { return nil }
    // SpeakerManager ids are 1-based ("1", "2", …); the engine expects "speaker_<0-based>".
    if let n = Int(speaker.id) { return "speaker_\(n - 1)" }
    return "speaker_\(speaker.id)"
}

while let line = readLine(strippingNewline: true) {
    if line.trimmingCharacters(in: .whitespaces).isEmpty { continue }
    guard let data = line.data(using: .utf8),
        let payload = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
        let command = payload["command"] as? String
    else {
        fail("invalid request")
        continue
    }
    do {
        switch command {
        case "health":
            ok(["status": "ready"])
        case "ensure_model":
            try await ensureModel(payload)
            ok(["model": asrVersionName ?? "", "diarization": diarizer != nil])
        case "transcribe":
            let language = (payload["language"] as? String).flatMap(Language.init(rawValue:))
            let result = try await transcribe(try decodeSamples(payload), language: language)
            ok(["text": result.text, "words": words(result.tokenTimings ?? [])])
        case "speaker_id":
            if let id = try speakerId(try decodeSamples(payload)) {
                ok(["speaker_id": id])
            } else {
                ok(["speaker_id": NSNull()])
            }
        case "clear_speakers":
            speakers.reset()
            ok(["cleared": true])
        case "shutdown":
            ok(["status": "bye"])
            exit(0)
        default:
            fail("unknown command: \(command)")
        }
    } catch {
        fail(error.localizedDescription)
    }
}
