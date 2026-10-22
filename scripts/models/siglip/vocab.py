# Write SigLIP's SentencePiece vocabulary as `piece<TAB>score` lines in id order, the file
# engine/src/meaning.rs reads (no protobuf parser in the engine).
import sys, sentencepiece as spm
sp = spm.SentencePieceProcessor(model_file=sys.argv[1])
with open(sys.argv[2], "w") as f:
    f.write("\n".join(f"{sp.id_to_piece(i)}\t{sp.get_score(i)}" for i in range(sp.get_piece_size())))
