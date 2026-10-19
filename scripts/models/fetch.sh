#!/usr/bin/env bash
# Rebuild engine/models/ (the local models, ADRs 0012 and 0013) from their upstream sources.
# The engine embeds these files, so this only runs when a model is replaced or upgraded.
#
# - selfie_multiclass_256x256: MediaPipe Image Segmenter (Apache 2.0). Shipped as TFLite;
#   converted to ONNX because tract can't run TFLite's TRANSPOSE_CONV.
# - face_detection_yunet_2023mar: OpenCV Zoo YuNet (MIT).
# - deeplabv3_mnv2_ade20k: TensorFlow DeepLab v3, MobileNetV2 on ADE20K (Apache 2.0). A
#   frozen TF graph, cut to the normalised input and the logits, then converted to ONNX.
set -euo pipefail
out="$(cd "$(dirname "$0")/../../engine/models" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

curl -fsSL -o "$work/selfie.tflite" \
  https://storage.googleapis.com/mediapipe-models/image_segmenter/selfie_multiclass_256x256/float32/latest/selfie_multiclass_256x256.tflite
python3 -m venv "$work/venv"
"$work/venv/bin/pip" install -q tensorflow-cpu tf2onnx onnx
"$work/venv/bin/python" -m tf2onnx.convert --tflite "$work/selfie.tflite" \
  --output "$out/selfie_multiclass_256x256.onnx" --opset 13

curl -fsSL -o "$work/ade.tgz" \
  https://storage.googleapis.com/download.tensorflow.org/models/deeplabv3_mnv2_ade20k_train_2018_12_03.tar.gz
tar xzf "$work/ade.tgz" -C "$work"
"$work/venv/bin/python" -m tf2onnx.convert \
  --graphdef "$work/deeplabv3_mnv2_ade20k_train_2018_12_03/frozen_inference_graph.pb" \
  --inputs "MobilenetV2/MobilenetV2/input:0[1,513,513,3]" --outputs "logits/semantic/BiasAdd:0" \
  --output "$out/deeplabv3_mnv2_ade20k.onnx" --opset 13

# MediaPipe Face Landmarker (Apache 2.0): the .task bundle is a zip of TFLite models; the
# landmarker and the blendshapes model become ONNX (its own face detector isn't used:
# YuNet finds the faces).
curl -fsSL -o "$work/fl.task" \
  https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/latest/face_landmarker.task
(cd "$work" && python3 -c "import zipfile; zipfile.ZipFile('fl.task').extractall('fl')")
"$work/venv/bin/python" -m tf2onnx.convert --tflite "$work/fl/face_landmarks_detector.tflite" \
  --output "$out/face_landmarks_mediapipe.onnx" --opset 13
"$work/venv/bin/python" -m tf2onnx.convert --tflite "$work/fl/face_blendshapes.tflite" \
  --output "$out/face_blendshapes_mediapipe.onnx" --opset 13

# MediaPipe Image Embedder (Apache 2.0), MobileNetV3 large. Opset 14 for HardSwish.
curl -fsSL -o "$work/embedder.tflite" \
  https://storage.googleapis.com/mediapipe-models/image_embedder/mobilenet_v3_large/float32/latest/mobilenet_v3_large.tflite
"$work/venv/bin/python" -m tf2onnx.convert --tflite "$work/embedder.tflite" \
  --output "$out/image_embedder_mediapipe.onnx" --opset 14

curl -fsSL -o "$out/face_detection_yunet_2023mar.onnx" \
  https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx

sha256sum "$out"/*.onnx
echo "Now update engine/models/manifest.json with these hashes and sizes."
