// JSON-lines worker for ContextCall, mirroring the protocol of parakeet_worker.py:
// one request object per stdin line, one {"ok": ..., "result"|"error": ...} per stdout line.
// Logs and progress go to stderr so stdout stays a clean response channel.
//
// Commands:
//   health
//   ensure_model   {model: "v3"|"ultra", asr: bool, diarization: bool}
//   transcribe     {samples_b64, language?} -> {text, words: [{text, start, end}]}
//                  (language: ISO 639-1, e.g. "en"; word times in seconds from the start)
//   speaker_id     {samples_b64}            -> {speaker_id: "speaker_N" | null, index: Int | null}
//                  (index: position of this embedding in the session history, for recluster)
//   recluster      {threshold?}             -> {speaker_ids: ["speaker_N" | null, ...]}
//                  (one label per history index, from clustering the whole session at once)
//   clear_speakers
//   shutdown
//
// samples_b64 is base64 of little-endian f32 PCM, 16 kHz mono.
import CoreML
import Foundation
import FluidAudio

let minSpeakerIdSamples = 16_000  // 1.0 s at 16 kHz, same floor as the Python worker
// Clips shorter than this give noisy embeddings: they may match a known speaker but
// never start a new one or move a speaker's centroid. Most spurious "Speaker N"
// labels came from short clips landing just outside every centroid.
let minNewSpeakerSeconds: Float = 2.0
// Max average cosine distance at which recluster joins two groups of embeddings.
let reclusterThreshold: Float = 0.45

var asrManager: AsrManager?
var asrVersionName: String?
var diarizer: DiarizerManager?
var speakers = SpeakerManager(minSpeechDuration: minNewSpeakerSeconds)
// Every embedding taken this session with its live label, so recluster can revisit
// early decisions once more of each voice has been heard.
var history: [(embedding: [Float], seconds: Float, liveId: String?)] = []
// Online speaker id -> name chosen by the last recluster.
var aliases: [String: String] = [:]

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
func speakerId(_ samples: [Float]) throws -> (id: String?, index: Int?) {
    guard samples.count >= minSpeakerIdSamples else { return (nil, nil) }
    guard let diarizer else {
        throw NSError(
            domain: "contextcall-fluidaudio", code: 3,
            userInfo: [NSLocalizedDescriptionKey: "diarizer not loaded; call ensure_model with diarization"])
    }
    let embedding = try diarizer.extractSpeakerEmbedding(from: samples)
    let seconds = Float(samples.count) / 16_000
    let speaker: Speaker?
    if seconds < minNewSpeakerSeconds {
        speaker = speakers.findSpeaker(with: embedding).id.flatMap { speakers.getSpeaker(for: $0) }
    } else {
        speaker = speakers.assignSpeaker(embedding, speechDuration: seconds)
    }
    let id = speaker.map { engineId($0.id) }
    history.append((embedding, seconds, id))
    return (id.map { aliases[$0] ?? $0 }, history.count - 1)
}

/// SpeakerManager ids are 1-based ("1", "2", …); the engine expects "speaker_<0-based>".
func engineId(_ id: String) -> String {
    if let n = Int(id) { return "speaker_\(n - 1)" }
    return "speaker_\(id)"
}

/// Average-linkage agglomerative clustering over the session's embeddings. Long clips
/// form the clusters; short ones join the nearest. Clusters keep the live label they
/// overlap most so bubbles only change where the live pass got it wrong, and later
/// live labels follow the same naming through `aliases`.
@MainActor
func recluster(threshold: Float) -> [String?] {
    let anchors = history.indices.filter { history[$0].seconds >= minNewSpeakerSeconds }
    let n = anchors.count
    // Pairwise distances between clusters, updated in place (Lance-Williams) so a
    // long session stays O(n²) memory and a few hundred ms per pass.
    var dist = [Float](repeating: 0, count: n * n)
    for a in 0..<n {
        for b in (a + 1)..<n {
            let d = SpeakerUtilities.cosineDistance(
                history[anchors[a]].embedding, history[anchors[b]].embedding)
            dist[a * n + b] = d
            dist[b * n + a] = d
        }
    }
    var members: [[Int]?] = anchors.map { [$0] }
    while true {
        var best: (a: Int, b: Int, d: Float) = (0, 0, .infinity)
        for a in 0..<n where members[a] != nil {
            for b in (a + 1)..<n where members[b] != nil && dist[a * n + b] < best.d {
                best = (a, b, dist[a * n + b])
            }
        }
        guard best.d <= threshold, let sa = members[best.a]?.count, let sb = members[best.b]?.count
        else { break }
        for k in 0..<n where members[k] != nil && k != best.a && k != best.b {
            let d = (Float(sa) * dist[k * n + best.a] + Float(sb) * dist[k * n + best.b]) / Float(sa + sb)
            dist[k * n + best.a] = d
            dist[best.a * n + k] = d
        }
        members[best.a]! += members[best.b]!
        members[best.b] = nil
    }
    let clusters = members.compactMap { $0 }

    var cluster = [Int?](repeating: nil, count: history.count)
    for (c, group) in clusters.enumerated() { for i in group { cluster[i] = c } }
    let centroids = clusters.map { group -> [Float] in
        var sum = [Float](repeating: 0, count: history[group[0]].embedding.count)
        for i in group {
            let e = history[i].embedding
            let norm = max(sqrt(e.reduce(0) { $0 + $1 * $1 }), 1e-9)
            for d in sum.indices { sum[d] += e[d] / norm }
        }
        return sum
    }
    for i in history.indices where cluster[i] == nil && !clusters.isEmpty {
        cluster[i] = centroids.indices.min {
            SpeakerUtilities.cosineDistance(history[i].embedding, centroids[$0])
                < SpeakerUtilities.cosineDistance(history[i].embedding, centroids[$1])
        }
    }

    // Name clusters after the live label holding the most speech in them, biggest first.
    var overlap: [Int: [String: Float]] = [:]
    var seconds = [Float](repeating: 0, count: clusters.count)
    for (i, entry) in history.enumerated() {
        guard let c = cluster[i] else { continue }
        seconds[c] += entry.seconds
        if let live = entry.liveId { overlap[c, default: [:]][live, default: 0] += entry.seconds }
    }
    var names: [Int: String] = [:]
    var taken = Set<String>()
    var next = (history.compactMap { $0.liveId.flatMap { Int($0.dropFirst("speaker_".count)) } }.max() ?? -1) + 1
    for c in clusters.indices.sorted(by: { seconds[$0] > seconds[$1] }) {
        if let live = overlap[c]?.filter({ !taken.contains($0.key) }).max(by: { $0.value < $1.value })?.key {
            names[c] = live
        } else {
            names[c] = "speaker_\(next)"
            next += 1
        }
        taken.insert(names[c]!)
    }

    // Live ids from the online pass map to the cluster that holds most of their speech.
    var votes: [String: [String: Float]] = [:]
    for (i, entry) in history.enumerated() {
        guard let live = entry.liveId, let name = cluster[i].flatMap({ names[$0] }) else { continue }
        votes[live, default: [:]][name, default: 0] += entry.seconds
    }
    aliases = votes.compactMapValues { $0.max(by: { $0.value < $1.value })?.key }
    return cluster.map { $0.flatMap { names[$0] } }
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
            let (id, index) = try speakerId(try decodeSamples(payload))
            ok(["speaker_id": id ?? NSNull(), "index": index ?? NSNull()])
        case "recluster":
            let threshold = (payload["threshold"] as? Double).map(Float.init) ?? reclusterThreshold
            ok(["speaker_ids": recluster(threshold: threshold).map { $0 ?? NSNull() as Any }])
        case "clear_speakers":
            speakers.reset()
            history.removeAll()
            aliases.removeAll()
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
