"""Depth Anything V2 Small (Apache-2.0) to the ONNX tract runs, for Lens Blur.

    python depth_onnx.py model.onnx depth_anything_v2_small_f16.onnx

Takes onnx-community/depth-anything-v2-small's fp32 `onnx/model.onnx` (scripts/models/fetch.sh)
and stores its weights as float16 with a Cast back to float32, which halves the file. The
graph still computes in float32: that repo's own `model_fp16.onnx` computes in float16, which
tract can't analyse. It also drops the export's symbolic shape notes (batch_size, height/14),
which tract can't unify with the fixed input shape Argus gives it.
"""
import sys
import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

src, out = sys.argv[1:3]
m = onnx.load(src)
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
del g.value_info[:]
for o in g.output:
    for d in o.type.tensor_type.shape.dim:
        d.Clear()
onnx.checker.check_model(m)
onnx.save(m, out)
