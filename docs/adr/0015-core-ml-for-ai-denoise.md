# 0015. AI Denoise runs through Core ML on the Mac, with tract as the reference

Status: accepted (2026-10-21, Greg picked "Build the Core ML path" on the AI Denoise thread).

## Context

AI Denoise (SCUNet, Apache-2.0) runs in 256 px tiles over the full-size demosaic. A 61 MP A7R V frame is about 1,250 tiles at 157 GFLOPs each. On Greg's M4 Max the CPU path through tract takes 0.18 s a tile at 8 threads, so 3.7 minutes a frame, and 16 threads is slower (4.2 minutes). Lightroom's Denoise is far quicker on the same Mac because it runs on the GPU and Neural Engine. A Shoot of 30 high-ISO frames would take two hours on the CPU and hold every performance core while it ran.

ADR 0013 put every model behind one runtime, tract on the CPU, so moving a model to other hardware should touch only the runtime. That still holds for the small Analyzers, which take milliseconds. Denoise is the first model where the CPU is the bottleneck a person feels.

## Decision

Denoise gets a second runtime on macOS, Core ML, and keeps tract.

- `engine/src/coreml.rs` (macOS only, `objc2-core-ml`) loads an embedded `.mlpackage`, compiles it for the Mac once into the temp folder under a hash of its bytes, and runs 8 one-tile inputs per `predictionsFromBatch` call. A model traced at a batch of 8 hung inside Metal, so batching stays in the call. Each call has a 120 s stall limit; after a stall Core ML is not used again in that process. `ARGUS_COREML_UNITS` (all, gpu, ane, cpu) picks the hardware for comparisons; Core ML chooses by default.
- `denoise::Backend` picks the runtime: Core ML when it loads, tract otherwise. If Core ML fails partway, the frame is redone on the CPU and the failure goes to the Debug log. `ARGUS_DENOISE_BACKEND=cpu` forces tract.
- The Core ML model is the same network and weights as the ONNX file, converted by `scripts/models/scunet_coreml.py` (coremltools 9, torch 2.7, float16 compute). The script checks the traced network against the ONNX model before writing. Masked attention scores use -1e4 instead of -inf so float16 never meets -inf minus -inf.
- tract stays the reference. Tests and cloud sessions run it, and `argus denoise --compare` on a Mac runs both and reports the speedup and how far Core ML's base strays from the CPU's.
- Nothing else moves to Core ML. Another model follows only when its CPU time is something a person waits on, with its own conversion script and the same compare check.

## Consequences

The Mac app carries two copies of the weights, the ONNX file (39 MB) and the Core ML package (42 MB). ADR 0013 asked models over 30 MB to download on first use; both are embedded for now so Denoise works offline from the first launch, and moving them to a download is a follow-up if the app size starts to matter.

On Greg's M4 Max (2026-10-21) a 61 MP frame takes 28 s, against 3.7 minutes on the CPU, and the result differs from the CPU's by at most 0.0003.

The first Denoise after an install or a model change pays a one-time compile of a few seconds.

Core ML on the GPU doesn't take the CPU cores the Compute levels guard, so a Denoise job no longer slows Greg's other apps the way the CPU path did. It still waits for idle like other model jobs when it's a batch.

The Core ML path can only be checked on a Mac. The cloud checks the converter (traced network against the ONNX model) and type-checks the Objective-C calls; the Mac thread runs `argus denoise --compare` after each change to the model or `coreml.rs`.
