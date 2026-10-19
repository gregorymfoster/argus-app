# 1. Rust engine with a web UI in a Tauri shell

Status: accepted (2026-10-17)

## Context

Argus has to be as fast as possible on large RAW batches, and its UI will mostly be built by AI. Greg's two priorities, in order: maximum performance, then the most beautiful UI we can get with AI doing the building. It targets the Mac only for now (Greg, 2026-10-17: mobile is out of scope until later) and must be easy to distribute.

## Decision

All image work lives in a Rust engine: RAW decode, the edit pipeline, previews, export, the photo catalog, and on-device ML (quality scoring, faces). The engine renders on the GPU through wgpu, which uses Metal on Apple hardware.

The UI is TypeScript and React running in a Tauri 2 shell. Tauri can also target iOS if mobile comes back.

The edit pipeline is written once as WGSL shaders. Native wgpu runs them for batch work and export. WebGPU in the webview runs the same shaders for live slider previews, so dragging a slider never waits on a round trip to the engine.

## Why

The engine is where performance comes from, and Rust gives us native speed and safe parallelism across a batch of 100 photos. Models are much better at producing polished web UI than SwiftUI or AppKit, because there is far more training data and a much larger component ecosystem, so a web front end serves the second goal best. Figma and Linear use this same split (a fast core with a web UI), and so do Tauri apps generally.

## Considered and rejected

Pure SwiftUI with Core Image: the best Apple integration, but AI-built SwiftUI looks plainer, and it locks the engine to Apple.

SwiftUI shell with a Rust core: native feel with a fast engine, but we'd write the UI twice if we ever leave Apple, and AI is weaker at it.

Electron: heavier binaries and memory than Tauri, and no iOS.

## Consequences

We give up Apple's built-in RAW decoder and Vision. RAW decoding needs a Rust decoder (rawler) or LibRaw through FFI, and face and quality models run through ONNX Runtime, which uses the CoreML backend on Apple hardware. Sony ARW support must be verified early. Moving full-resolution pixels between the engine and the webview is the main performance risk, so previews travel as display-sized GPU textures or tiles over a custom protocol, never as full-resolution buffers over JSON IPC.
