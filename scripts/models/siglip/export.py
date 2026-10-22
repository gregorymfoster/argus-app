# Export SigLIP's two towers to ONNX (float32), unit-length outputs. f16.py then halves them.
import sys, torch, onnx
from onnx import numpy_helper, helper, TensorProto
from transformers import SiglipModel
m = SiglipModel.from_pretrained(sys.argv[1]).eval(); out = sys.argv[2]
class Img(torch.nn.Module):
    def __init__(s): super().__init__(); s.v = m.vision_model
    def forward(s, x):
        y = s.v(pixel_values=x).pooler_output
        return y / y.norm(dim=-1, keepdim=True)
class Txt(torch.nn.Module):
    def __init__(s): super().__init__(); s.t = m.text_model
    def forward(s, ids):
        y = s.t(input_ids=ids).pooler_output
        return y / y.norm(dim=-1, keepdim=True)
torch.onnx.export(Img(), torch.zeros(1, 3, 224, 224), f'{out}/siglip_image.onnx', input_names=['pixels'], output_names=['embedding'], opset_version=17, dynamo=False)
torch.onnx.export(Txt(), torch.ones(1, 64, dtype=torch.int64), f'{out}/siglip_text.onnx', input_names=['ids'], output_names=['embedding'], opset_version=17, dynamo=False)
print('scale', m.logit_scale.exp().item(), 'bias', m.logit_bias.item())
