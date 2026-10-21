"""SCUNet (Apache-2.0, github.com/cszn/SCUNet) to the ONNX tract runs, for AI Denoise.

    python scunet_onnx.py network_scunet.py scunet_color_real_psnr.pth out.onnx
    python scunet_onnx.py --shrink in.onnx out.onnx   # only the mask step below (no torch)

Loads the upstream network file with three changes that leave its output unchanged: no
`thop` or `timm` imports (unused at inference), and the shifted-window attention mask built
in numpy, so the export holds it as a constant (tract has no ScatterND). Exports a fixed
1x3x256x256 tile (engine/src/denoise.rs TILE) and stores the weights as float16 with a Cast
back to float32, which halves the file and changes outputs by under 2e-4. Last, each
shifted-window mask (a bool constant of (windows x 64 x 64), 22 MB in all) is stored as the
OR of a per-row and a per-column part, which tract folds back into one constant on load.
"""
import sys, types
import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper


def shrink_masks(m):
    """Replace each big bool Constant (1, 1, h*w, P, P) that is exactly row-part OR
    column-part with Or(row (h,1,P,P), col (1,w,P,P)) reshaped back. Returns how many."""
    g = m.graph
    nodes, n = [], 0
    for node in g.node:
        t = node.attribute[0].t if node.op_type == "Constant" and node.attribute else None
        if t is None or t.data_type != TensorProto.BOOL or len(t.dims) != 5 or t.dims[2] < 4:
            nodes.append(node)
            continue
        a = numpy_helper.to_array(t)
        side = int(round(a.shape[2] ** 0.5))
        p = a.shape[3]
        grid = a.reshape(side, side, p, p)
        row, col = grid[:, :1], grid[:1, :]  # column 0 / row 0 hold only the other part
        if side < 2 or not np.array_equal(np.logical_or(row, col), grid):
            nodes.append(node)
            continue
        out = node.output[0]
        nodes += [
            helper.make_node("Constant", [], [out + "_row"], value=numpy_helper.from_array(row)),
            helper.make_node("Constant", [], [out + "_col"], value=numpy_helper.from_array(col)),
            helper.make_node("Constant", [], [out + "_shape"],
                             value=numpy_helper.from_array(np.array(a.shape, dtype=np.int64))),
            helper.make_node("Or", [out + "_row", out + "_col"], [out + "_grid"]),
            helper.make_node("Reshape", [out + "_grid", out + "_shape"], [out]),
        ]
        n += 1
    del g.node[:]
    g.node.extend(nodes)
    return n


if sys.argv[1] == "--shrink":
    m = onnx.load(sys.argv[2])
    print("masks split:", shrink_masks(m))
    onnx.checker.check_model(m)
    onnx.save(m, sys.argv[3])
    sys.exit(0)

import torch

src, weights, out = sys.argv[1:4]
code = open(src).read()
code = code.replace("from thop import profile", "")
code = code.replace(
    "from timm.models.layers import trunc_normal_, DropPath",
    "from torch.nn.init import trunc_normal_\nDropPath = lambda *a, **k: nn.Identity()",
)
start = code.index("        attn_mask = torch.zeros(h, w, p, p, p, p")
end = code.index("        return attn_mask\n", code.index("rearrange(attn_mask"))
code = code[:start] + '''        m = np.zeros((h, w, p, p, p, p), dtype=bool)
        if self.type != 'W':
            s = p - shift
            m[-1, :, :s, :, s:, :] = True
            m[-1, :, s:, :, :s, :] = True
            m[:, -1, :, :s, :, s:] = True
            m[:, -1, :, s:, :, :s] = True
        attn_mask = torch.from_numpy(m.reshape(1, 1, h * w, p * p, p * p))
''' + code[end:]
mod = types.ModuleType("network_scunet")
exec(compile(code, src, "exec"), mod.__dict__)

net = mod.SCUNet(in_nc=3, config=[4, 4, 4, 4, 4, 4, 4], dim=64)
net.load_state_dict(torch.load(weights, map_location="cpu", weights_only=True), strict=True)
net.eval()
tmp = out + ".f32"
torch.onnx.export(net, torch.rand(1, 3, 256, 256), tmp, opset_version=17,
                  input_names=["input"], output_names=["output"], dynamo=False)

m = onnx.load(tmp)
g = m.graph
inits, casts = [], []
for init in g.initializer:
    a = numpy_helper.to_array(init)
    if a.dtype == np.float32 and a.size >= 64:
        h = numpy_helper.from_array(a.astype(np.float16), init.name + "_f16")
        inits.append(h)
        casts.append(helper.make_node("Cast", [h.name], [init.name], to=TensorProto.FLOAT))
    else:
        inits.append(init)
del g.initializer[:]
g.initializer.extend(inits)
nodes = casts + list(g.node)
del g.node[:]
g.node.extend(nodes)
print("masks split:", shrink_masks(m))
onnx.checker.check_model(m)
onnx.save(m, out)
