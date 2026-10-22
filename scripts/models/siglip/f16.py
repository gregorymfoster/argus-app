# Store an ONNX model's big float32 weights as float16 with a Cast back to float32, so the
# file halves and the runtime still computes in float32 (tract folds the Casts on load).
import sys, onnx, numpy as np
from onnx import numpy_helper, helper, TensorProto
src, dst = sys.argv[1], sys.argv[2]
m = onnx.load(src)
g = m.graph
casts, keep = [], []
for t in g.initializer:
    if t.data_type == TensorProto.FLOAT and np.prod(t.dims) >= 1024:
        a = numpy_helper.to_array(t).astype(np.float16)
        h = numpy_helper.from_array(a, t.name + "__f16")
        keep.append(h)
        casts.append(helper.make_node("Cast", [h.name], [t.name], to=TensorProto.FLOAT, name=t.name + "__cast"))
    else:
        keep.append(t)
del g.initializer[:]
g.initializer.extend(keep)
nodes = list(g.node)
del g.node[:]
g.node.extend(casts + nodes)
onnx.save(m, dst)
print(dst, len(casts), "weights to float16")
