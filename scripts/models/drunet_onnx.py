"""Export KAIR's DRUNet colour denoiser (MIT, github.com/cszn/KAIR, `drunet_color.pth` from
its v1.0 release) to ONNX for Argus's AI Denoise.

DRUNet is non-blind: it takes the image plus a fourth plane holding the noise level (sigma,
0..1) and was trained on white Gaussian noise. Argus feeds it RAW sites made Gaussian by a
variance-stabilising transform, so the noise level is known exactly (engine/src/denoise.rs).

The network is written out here (UNetRes: 4 levels, 4 residual blocks each, no biases) so no
code from the download runs; the weights load with torch's safe `weights_only` loader.

    python3 scripts/models/drunet_onnx.py drunet_color.pth engine/models/drunet_color_f16.onnx [--tile 256]
"""
import argparse

import torch
from torch import nn


class Res(nn.Module):
    def __init__(self, c):
        super().__init__()
        self.res = nn.Sequential(nn.Conv2d(c, c, 3, 1, 1, bias=False), nn.ReLU(), nn.Conv2d(c, c, 3, 1, 1, bias=False))

    def forward(self, x):
        return x + self.res(x)


def level(c, nb=4):
    return [Res(c) for _ in range(nb)]


class DRUNet(nn.Module):
    def __init__(self, nc=(64, 128, 256, 512)):
        super().__init__()
        self.m_head = nn.Conv2d(4, nc[0], 3, 1, 1, bias=False)
        self.m_down1 = nn.Sequential(*level(nc[0]), nn.Conv2d(nc[0], nc[1], 2, 2, 0, bias=False))
        self.m_down2 = nn.Sequential(*level(nc[1]), nn.Conv2d(nc[1], nc[2], 2, 2, 0, bias=False))
        self.m_down3 = nn.Sequential(*level(nc[2]), nn.Conv2d(nc[2], nc[3], 2, 2, 0, bias=False))
        self.m_body = nn.Sequential(*level(nc[3]))
        self.m_up3 = nn.Sequential(nn.ConvTranspose2d(nc[3], nc[2], 2, 2, 0, bias=False), *level(nc[2]))
        self.m_up2 = nn.Sequential(nn.ConvTranspose2d(nc[2], nc[1], 2, 2, 0, bias=False), *level(nc[1]))
        self.m_up1 = nn.Sequential(nn.ConvTranspose2d(nc[1], nc[0], 2, 2, 0, bias=False), *level(nc[0]))
        self.m_tail = nn.Conv2d(nc[0], 3, 3, 1, 1, bias=False)

    def forward(self, x0):
        x1 = self.m_head(x0)
        x2 = self.m_down1(x1)
        x3 = self.m_down2(x2)
        x4 = self.m_down3(x3)
        x = self.m_body(x4)
        x = self.m_up3(x + x4)
        x = self.m_up2(x + x3)
        x = self.m_up1(x + x2)
        return self.m_tail(x + x1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("weights")
    ap.add_argument("out")
    ap.add_argument("--tile", type=int, default=256)
    ap.add_argument("--fp32", action="store_true")
    args = ap.parse_args()
    net = DRUNet()
    net.load_state_dict(torch.load(args.weights, map_location="cpu", weights_only=True), strict=True)
    net.eval()
    x = torch.rand(1, 4, args.tile, args.tile)
    torch.onnx.export(net, x, args.out, input_names=["input"], output_names=["output"],
                      opset_version=17, dynamo=False)
    if not args.fp32:
        import onnx
        from onnx import numpy_helper
        import numpy as np
        # Weights as fp16, computed in fp32: the file halves and the result doesn't move.
        m = onnx.load(args.out)
        nodes = []
        for init in list(m.graph.initializer):
            arr = numpy_helper.to_array(init)
            if arr.dtype == np.float32:
                half = numpy_helper.from_array(arr.astype(np.float16), init.name + "_f16")
                m.graph.initializer.remove(init)
                m.graph.initializer.append(half)
                nodes.append(onnx.helper.make_node("Cast", [half.name], [init.name], to=onnx.TensorProto.FLOAT))
        for n in reversed(nodes):
            m.graph.node.insert(0, n)
        onnx.save(m, args.out)
    print(args.out, sum(p.numel() for p in net.parameters()), "parameters")


if __name__ == "__main__":
    main()
