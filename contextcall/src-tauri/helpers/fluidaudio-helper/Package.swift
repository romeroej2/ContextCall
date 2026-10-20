// swift-tools-version: 6.0
// FluidAudio sidecar: Parakeet TDT ASR + speaker embeddings on CoreML / Apple Neural Engine.
// Built by src-tauri/build.rs on macOS and bundled as a Tauri externalBin.
import PackageDescription

let package = Package(
    name: "contextcall-fluidaudio",
    platforms: [.macOS(.v14)],
    dependencies: [
        // Pinned to the revision benchmarked for ContextCall (2026-10-19 main).
        .package(
            url: "https://github.com/FluidInference/FluidAudio.git",
            revision: "61d41b3ca04833d1cb80632c82ba401f117a01ef"
        )
    ],
    targets: [
        .executableTarget(
            name: "contextcall-fluidaudio",
            dependencies: [.product(name: "FluidAudio", package: "FluidAudio")]
        )
    ]
)
