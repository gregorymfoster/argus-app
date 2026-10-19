# 12. Mask models run locally on the CPU, embedded in the binary

Status: accepted (2026-10-18). The models now run as Analyzers and their output is stored as Facts (ADR 0013): the `<preview>.mask` cache and `ANALYSIS_VERSION` below are gone.

## Context

Masks for people and sky need segmentation, and retouching needs face landmarks. Greg wants no API keys, the app is an unsigned download that should work offline, and the cloud sessions that build Argus must be able to run and check every Mask without his Mac. ONNX Runtime with CoreML would be fastest on the Mac, but it needs native libraries per platform and a download at build time, which the cloud proxy blocks.

## Decision

Three small open models run through tract (pure Rust ONNX inference) on the CPU, and their ONNX files live in `engine/models/` and are compiled into the engine with `include_bytes!`:

- MediaPipe's multiclass selfie segmenter (Apache 2.0, 16 MB): background, hair, body skin, face skin, clothes, other. It gives `subject`, `background`, `skin`, `hair` and `faces`.
- OpenCV Zoo's YuNet face detector (MIT, 0.2 MB): face boxes plus eye and mouth landmarks, for one-face Masks, eyes and teeth.
- TensorFlow's DeepLab v3 MobileNetV2 trained on ADE20K (Apache 2.0, 9 MB): the `sky` Mask. A colour-and-texture heuristic was tried first and took white walls for overcast sky.

`scripts/models/fetch.sh` rebuilds the files from upstream (two are converted from TFLite and a frozen TF graph with tf2onnx). A Photo is analysed once from its preview and cached next to it as `<preview>.mask`; renders refine the coarse model output against the render's own edges with a guided filter.

## Consequences

- Masks work offline and in every test, with nothing to install. The repo and the binary grow by about 26 MB.
- Analysing a Photo the first time takes about 1.5 s on the cloud CPU (the sky model is most of it); after that a Mask costs a guided filter, tens of milliseconds at edit size. Moving to ORT with CoreML later only changes `masks::models`.
- Changing a model or its preprocessing must bump `masks::ANALYSIS_VERSION`, so cached analyses are rebuilt.
- MediaPipe's segmenter is trained on selfies and portraits; distant people in wide shots segment less cleanly than close ones.
