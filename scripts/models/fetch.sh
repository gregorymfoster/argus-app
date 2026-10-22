#!/usr/bin/env bash
# Rebuild engine/models/ (the local models, ADRs 0012 and 0013) from their upstream sources.
# The engine embeds these files, so this only runs when a model is replaced or upgraded.
#
# - selfie_multiclass_256x256: MediaPipe Image Segmenter (Apache 2.0). Shipped as TFLite;
#   converted to ONNX because tract can't run TFLite's TRANSPOSE_CONV.
# - face_detection_yunet_2023mar: OpenCV Zoo YuNet (MIT).
# - face_recognition_sface_2021dec: OpenCV Zoo SFace (Apache 2.0), for grouping People.
# - deeplabv3_mnv2_ade20k: TensorFlow DeepLab v3, MobileNetV2 on ADE20K (Apache 2.0). A
#   frozen TF graph, cut to the normalised input and the logits, then converted to ONNX.
# - scunet_color_real_f16: SCUNet's blind real-noise model (Apache 2.0) for AI Denoise,
#   exported from PyTorch by scunet_onnx.py (fp16 weights), and as a Core ML package by
#   scunet_coreml.py for the Mac's GPU path.
# - depth_anything_v2_small_f16: Depth Anything V2 Small (Apache 2.0; Base and Large are
#   non-commercial) for Lens Blur, onnx-community's export with fp16 weights by depth_onnx.py.
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

curl -fsSL -o "$out/face_recognition_sface_2021dec.onnx" \
  https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/face_recognition_sface/face_recognition_sface_2021dec.onnx

sha256sum "$out"/*.onnx
echo "Now update engine/models/manifest.json with these hashes and sizes."

# SCUNet (Apache 2.0): network code and weights from the authors' GitHub, exported by
# scunet_onnx.py. A different torch version may write different bytes for the same model:
# update the manifest's sha256 after checking renders.
curl -fsSL -o "$work/network_scunet.py" https://raw.githubusercontent.com/cszn/SCUNet/main/models/network_scunet.py
curl -fsSL -o "$work/scunet.pth" https://github.com/cszn/KAIR/releases/download/v1.0/scunet_color_real_psnr.pth
python3 -m venv "$work/torch"
"$work/torch/bin/pip" install -q torch onnx einops numpy
"$work/torch/bin/python" "$(dirname "$0")/scunet_onnx.py" "$work/network_scunet.py" "$work/scunet.pth" \
  "$out/scunet_color_real_f16.onnx"
# The same network for Core ML on the Mac (ADR 0015). coremltools 9 converts from torch 2.7;
# it checks the traced network against the ONNX model above before writing.
"$work/torch/bin/pip" install -q "torch==2.7.0" coremltools==9.0 onnxruntime
rm -rf "$out/scunet_color_real.mlpackage"
"$work/torch/bin/python" "$(dirname "$0")/scunet_coreml.py" "$work/network_scunet.py" "$work/scunet.pth" \
  "$out/scunet_color_real.mlpackage" "$out/scunet_color_real_f16.onnx"

# Depth Anything V2 Small (Apache 2.0): onnx-community's fp32 export, then depth_onnx.py.
curl -fsSL -o "$work/depth.onnx" https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/main/onnx/model.onnx
"$work/venv/bin/python" "$(dirname "$0")/depth_onnx.py" "$work/depth.onnx" "$out/depth_anything_v2_small_f16.onnx"
