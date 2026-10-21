"""SCUNet (Apache-2.0) to a Core ML model, the GPU path of AI Denoise on the Mac (ADR 0015).

    python scunet_coreml.py network_scunet.py scunet_color_real_psnr.pth out.mlpackage [ref.onnx]

Same network and weights as scunet_onnx.py (its `load_net`), traced at a fixed batch of
BATCH 256 px tiles, converted to an ML Program that computes in float16 with float32 input and output.
With the CPU model (`ref.onnx`) given, it checks every tile of the traced batch against it
before writing anything. Core ML itself only runs on macOS, so the check of the converted
model is `argus denoise --compare` on the Mac.
"""
import os
import sys

# One tile: the engine feeds Core ML several per call through predictionsFromBatch. A model
# traced at a batch of 8 hung inside Metal on the Mac (2026-10-21).
BATCH = 1

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from scunet_onnx import load_net  # noqa: E402

import coremltools as ct  # noqa: E402
import torch  # noqa: E402
from coremltools.converters.mil import Builder as mb  # noqa: E402
from coremltools.converters.mil.frontend.torch import ops as torch_ops  # noqa: E402

_cast = torch_ops._cast


def _cast_item(context, node, dtype, dtype_name):
    """coremltools 9 fails on int() of a folded length-1 array (the padding sums in
    SCUNet.forward): take its item."""
    x = torch_ops._get_inputs(context, node, expected=1)[0]
    if x.can_be_folded_to_const() and np.ndim(x.val) > 0:
        context.add(mb.const(val=dtype(np.asarray(x.val).item()), name=node.name))
    else:
        _cast(context, node, dtype, dtype_name)


torch_ops._cast = _cast_item

src, weights, out = sys.argv[1:4]
ref = sys.argv[4] if len(sys.argv) > 4 else None

# Masked attention scores get -1e4, not -inf: float16 holds it, and softmax gives the
# same zero weight without risking -inf - -inf = NaN on the Neural Engine.
net = load_net(src, weights, mask_value=-1e4)
rng = np.random.default_rng(7)
y, x = np.mgrid[0:256, 0:256] / 256.0
tiles = []
for i in range(BATCH):
    clean = np.stack([0.2 + 0.6 * x, 0.3 + 0.4 * y, 0.5 + 0.3 * np.sin((6 + i) * x * y)])
    tiles.append(clean[:, ::-1] if i % 2 else clean)
tile = np.clip(np.stack(tiles) + rng.normal(0, 0.08, (BATCH, 3, 256, 256)), 0, 1)
tile = tile.astype(np.float32)

with torch.no_grad():
    traced = torch.jit.trace(net, torch.from_numpy(tile))
    want = traced(torch.from_numpy(tile)).numpy()
if ref:
    import onnxruntime as ort

    session = ort.InferenceSession(ref)
    got = np.concatenate([session.run(None, {"input": t[None]})[0] for t in tile])
    print(f"torch batch of {BATCH} vs CPU model tile by tile: max {np.abs(want - got).max():.2e}")
    assert np.abs(want - got).max() < 2e-3

mlmodel = ct.convert(
    traced,
    inputs=[ct.TensorType(name="input", shape=tile.shape, dtype=np.float32)],
    outputs=[ct.TensorType(name="output", dtype=np.float32)],
    convert_to="mlprogram",
    compute_precision=ct.precision.FLOAT16,
    minimum_deployment_target=ct.target.macOS14,
)
mlmodel.short_description = f"SCUNet real-noise denoiser (Apache-2.0), {BATCH} tiles of 256 px"
mlmodel.save(out)
print("wrote", out)
