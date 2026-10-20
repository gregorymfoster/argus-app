# Third-party notices

Argus's own code is MIT (see `LICENSE`). It ships the open models and libraries below under their own licences.

## Models embedded in the app

These live in `engine/models/`, are listed with their hashes in `engine/models/manifest.json`, and are rebuilt by `scripts/models/fetch.sh`. `engine/tests/analyzers.rs` fails if a model's licence is not on the allow-list.

| File | What it is | Licence | Source |
|---|---|---|---|
| `selfie_multiclass_256x256.onnx` | MediaPipe multiclass selfie segmenter (converted from TFLite) | Apache-2.0 | Google MediaPipe |
| `face_detection_yunet_2023mar.onnx` | YuNet face detector | MIT | OpenCV Zoo |
| `deeplabv3_mnv2_ade20k.onnx` | DeepLabV3 MobileNetV2 scene segmenter trained on ADE20K | Apache-2.0 | TensorFlow models |
| `face_landmarks_mediapipe.onnx` | MediaPipe face landmarker | Apache-2.0 | Google MediaPipe |
| `face_blendshapes_mediapipe.onnx` | MediaPipe face blendshapes | Apache-2.0 | Google MediaPipe |
| `image_embedder_mediapipe.onnx` | MediaPipe image embedder (MobileNetV3 large) | Apache-2.0 | Google MediaPipe |

The Apache-2.0 models are distributed under the Apache License, Version 2.0 (https://www.apache.org/licenses/LICENSE-2.0). They were converted to ONNX with tf2onnx for Argus (the DeepLab graph is also cut at its logits); the weights are unchanged. Copyright in each belongs to its authors (Google LLC for MediaPipe and the TensorFlow models; the OpenCV Zoo contributors for YuNet).

The model Argus asks to cull and edit (Ollama's `qwen3.8:27b` by default, or Claude) is not bundled. You download or connect it yourself, under its own terms.

## Libraries

The Rust crates are listed in `Cargo.lock` and `app/Cargo.lock`, the JavaScript packages in `prototype/package-lock.json` and `app/package-lock.json`. Nearly all are MIT, Apache-2.0, BSD, ISC, Zlib or Unicode licensed. Two are worth naming:

- [rawler](https://github.com/dnglab/dnglab) (RAW decoding) is LGPL-2.1. Argus links it statically. Its source is public, and Argus's own source here lets anyone rebuild the app against a modified rawler, which is what the LGPL asks for.
- webpki-roots (TLS root certificates) is CDLA-Permissive-2.0.

## Fonts

- [Nordwand Mono](https://github.com/tywr/Nordwand-Mono) 1.3.1 by Tanguy Vivier, Argus's monospace for file names, EXIF and numbers, is under the SIL Open Font License 1.1. The WOFF2 files and the licence text are in `prototype/src/fonts/nordwand-mono/`.
- Geist and Geist Mono (Vercel), the fallback UI fonts, are under the SIL Open Font License 1.1 and come from the `@fontsource-variable` packages.
