#!/usr/bin/env bash
# Rebuild the search-by-meaning model (SigLIP B/16 224, Apache 2.0) for the models folder:
# scripts/models/siglip/build.sh OUT_DIR. Weights from Google's big_vision bucket, the
# vocabulary from T5's. Writes siglip-image.onnx, siglip-text.onnx and siglip-vocab.tsv;
# check their sha256 against engine/models/manifest.json (a different torch can change
# the bytes but not the vectors; engine/tests/meaning.rs checks those).
set -euo pipefail
out=${1:?usage: build.sh OUT_DIR}
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
mkdir -p "$out"
curl -fsSL -o "$work/b16.npz" https://storage.googleapis.com/big_vision/siglip/webli_en_b16_224_63724782.npz
curl -fsSL -o "$work/vocab.model" https://storage.googleapis.com/t5-data/vocabs/cc_en.32000/sentencepiece.model
python3 -m venv "$work/venv"
"$work/venv/bin/pip" install -q torch transformers sentencepiece onnx numpy
"$work/venv/bin/python" "$here/convert.py" "$work/b16.npz" "$work/hf" "$work/vocab.model"
mkdir -p "$work/onnx"
"$work/venv/bin/python" "$here/export.py" "$work/hf" "$work/onnx"
for t in image text; do
  "$work/venv/bin/python" "$here/f16.py" "$work/onnx/siglip_$t.onnx" "$out/siglip-$t.onnx"
done
"$work/venv/bin/python" "$here/vocab.py" "$work/vocab.model" "$out/siglip-vocab.tsv"
sha256sum "$out"/siglip-*
