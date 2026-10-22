# Load big_vision SigLIP B/16 224 (npz) into transformers' SiglipModel and check it.
import sys, numpy as np, torch
from transformers import SiglipConfig, SiglipModel, SiglipTokenizer
d = np.load(sys.argv[1]); out = sys.argv[2]; vocab = sys.argv[3]
cfg = SiglipConfig(
    text_config=dict(vocab_size=32000, hidden_size=768, intermediate_size=3072, num_hidden_layers=12, num_attention_heads=12, max_position_embeddings=64, projection_size=768),
    vision_config=dict(hidden_size=768, intermediate_size=3072, num_hidden_layers=12, num_attention_heads=12, image_size=224, patch_size=16),
)
m = SiglipModel(cfg).eval()
sd = {}
T = lambda a: torch.from_numpy(np.ascontiguousarray(a)).float()
def attn(src, dst):
    for n in ['query', 'key', 'value']:
        hf = {'query': 'q_proj', 'key': 'k_proj', 'value': 'v_proj'}[n]
        sd[f'{dst}.{hf}.weight'] = T(d[f'{src}/{n}/kernel'].reshape(768, 768).T)
        sd[f'{dst}.{hf}.bias'] = T(d[f'{src}/{n}/bias'].reshape(-1))
    sd[f'{dst}.out_proj.weight'] = T(d[f'{src}/out/kernel'].reshape(768, 768).T)
    sd[f'{dst}.out_proj.bias'] = T(d[f'{src}/out/bias'])
def ln(src, dst):
    sd[f'{dst}.weight'] = T(d[f'{src}/scale']); sd[f'{dst}.bias'] = T(d[f'{src}/bias'])
def dense(src, dst):
    sd[f'{dst}.weight'] = T(d[f'{src}/kernel'].T); sd[f'{dst}.bias'] = T(d[f'{src}/bias'])
def blocks(src, dst):
    for i in range(12):
        s = f'{src}/encoderblock_{i}'; t = f'{dst}.layers.{i}'
        ln(f'{s}/LayerNorm_0', f'{t}.layer_norm1'); ln(f'{s}/LayerNorm_1', f'{t}.layer_norm2')
        attn(f'{s}/MultiHeadDotProductAttention_0', f'{t}.self_attn')
        dense(f'{s}/MlpBlock_0/Dense_0', f'{t}.mlp.fc1'); dense(f'{s}/MlpBlock_0/Dense_1', f'{t}.mlp.fc2')
p = 'params/img'; v = 'vision_model'
sd[f'{v}.embeddings.patch_embedding.weight'] = T(d[f'{p}/embedding/kernel'].transpose(3, 2, 0, 1))
sd[f'{v}.embeddings.patch_embedding.bias'] = T(d[f'{p}/embedding/bias'])
sd[f'{v}.embeddings.position_embedding.weight'] = T(d[f'{p}/pos_embedding'][0])
blocks(f'{p}/Transformer', f'{v}.encoder'); ln(f'{p}/Transformer/encoder_norm', f'{v}.post_layernorm')
h = f'{p}/MAPHead_0'
sd[f'{v}.head.probe'] = T(d[f'{h}/probe'])
qkv = [d[f'{h}/MultiHeadDotProductAttention_0/{n}/kernel'].reshape(768, 768).T for n in ['query', 'key', 'value']]
sd[f'{v}.head.attention.in_proj_weight'] = T(np.concatenate(qkv, 0))
sd[f'{v}.head.attention.in_proj_bias'] = T(np.concatenate([d[f'{h}/MultiHeadDotProductAttention_0/{n}/bias'].reshape(-1) for n in ['query', 'key', 'value']]))
sd[f'{v}.head.attention.out_proj.weight'] = T(d[f'{h}/MultiHeadDotProductAttention_0/out/kernel'].reshape(768, 768).T)
sd[f'{v}.head.attention.out_proj.bias'] = T(d[f'{h}/MultiHeadDotProductAttention_0/out/bias'])
ln(f'{h}/LayerNorm_0', f'{v}.head.layernorm')
dense(f'{h}/MlpBlock_0/Dense_0', f'{v}.head.mlp.fc1'); dense(f'{h}/MlpBlock_0/Dense_1', f'{v}.head.mlp.fc2')
p = 'params/txt'; t = 'text_model'
sd[f'{t}.embeddings.token_embedding.weight'] = T(d[f'{p}/Embed_0/embedding'])
sd[f'{t}.embeddings.position_embedding.weight'] = T(d[f'{p}/pos_embedding'][0])
blocks(f'{p}/Encoder_0', f'{t}.encoder'); ln(f'{p}/Encoder_0/encoder_norm', f'{t}.final_layer_norm')
dense(f'{p}/head', f'{t}.head')
sd['logit_scale'] = T(d['params/t']); sd['logit_bias'] = T(d['params/b'])
own = m.state_dict()
missing = [k for k in own if k not in sd and 'position_ids' not in k]
extra = [k for k in sd if k not in own]
print('missing', missing, 'extra', extra)
m.load_state_dict(sd, strict=False)
m.save_pretrained(out)
tok = SiglipTokenizer(vocab, model_max_length=64)
tok.save_pretrained(out)
print('saved', out)
