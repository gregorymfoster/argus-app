# Fine-tuning the local vision model on Greg's taste

Greg asked for this on 2026-10-23 (item 2 of the coordinator's list of bigger swings: "i love 1-6, do all of them"). Today the cull and the Agent prompt a stock Qwen. Here the model itself learns from what Greg already did: a LoRA adapter, trained overnight on his Mac with MLX, then scored against the stock model on the same benches. Shipping it through Ollama or the app comes later, and only if it wins.

Script: `bench/taste_tune.py` (`data`, `train`, `eval`, `report`). Training is mlx-vlm's own LoRA trainer (`python -m mlx_vlm.train`, MIT), driven with `--train-on-completions` so only the answer is learnt.

## What it learns

One adapter, two tasks, one image each.

**Cull: would Greg throw this frame out?** Labels come from the drive (`bench/drive_bench.py`): his rejects against the frames he left unflagged on the same day. Both are unedited Lightroom exports, so neither side carries his edit. Favorites stay out. They are his finished edits, and a favorite's RAW renders through Argus while its neighbours are Lightroom exports, so either way the model could learn the rendering instead of the moment. Rejects outnumber unflagged frames about 2.5 to 1, so training uses even numbers (the cull has to stay cautious, Greg 2026-10-20). This is the job the cull actually gives the local model: catching obvious rejects, with picking left to the person.

**Edit: his Basic sliders.** Input is Argus's unedited render of the RAW (`pair_features.py` plain renders, 384 px) plus Auto's ten sliders in the prompt. The answer is a JSON object of edit-fit's `fit` sliders, the Argus-space edit closest to his finished JPEG. That is the same target `whole_edit_render.py` scores, so the rendered comparison is like for like.

## Splits and what never trains

`bench/splits.py` fold 0 is test and never trains. A few whole fold-1 days validate. The five cull Shoots in `fixtures/bench/cull` never train, and neither do the 140 edit cases. The edit bench is scored only on the 34 cases from test-fold days, so no Shoot behind them trained either model. With the data on 2026-10-23 that gives about 1,400 cull and 3,500 edit training rows, 880 cull and 1,100 edit test rows (edits are sampled to 300 at eval, since each one generates a JSON answer).

## How it is scored

Base and adapter both run through the same MLX eval, same prompts, so the only difference is the adapter.

- Drive test days: AUC of the reject score (log-prob of "reject" minus "keep"), overall and per day.
- The 2026-10-10 card: pick-vs-not-picked AUC, how many frames it would reject and how many of those Greg picked (wrong rejects, the number that matters for a cautious cull).
- Damaged twins: 24 of the card's picks get a blurred, shaken, dark or badly cropped copy (`cull_degrade.py`'s damages). A good model scores the twin worse.
- Edits in slider space: z-scored error on the test pairs against Auto, Auto plus the average offset, and `whole_edit_render.py`'s ridge-scene trained on the same rows (`report --features`).
- Edits in pixels: `whole_edit_render.py --vlm NAME=FILE` renders the predictions on the 34 test-day bench cases next to none, Auto, the head and the ridge models, all scored on just those photos.

## Base model and licence

The base is whatever the Mac pulls, passed as `--model`; nothing in the code names one. `train` reads the model card's `license:` and refuses anything but Apache, MIT or BSD. The first run uses a small model from the same Qwen VL family (a 7-9B size class) because a 27B LoRA on 64 GB is slow; a short `--preflight` measures seconds per step first, and the 27B only gets a night if the small one shows any signal.

## What to expect

The priors aren't kind. Every model so far predicts Greg's keeps poorly (stock Qwen AUC 0.48 to 0.54 on the card), and edit models plateau near 10 dE no matter how much data they get. A fine-tuned model sees the pixels, though, which the ridge heads only see through an embedding. If it still lands where the ridge does, that settles the question cheaply.

## Run plan

Greg said go at 7:41 PM ET on 2026-10-23: start the heavy work now, checkpoint, inspect along the way, and write down what we learn. So rather than one long blind epoch, `taste_tune.py rounds` trains in slices.

1. A full-power preflight (8 steps) sets the image size. If a step takes over 10 s, the data is rebuilt at 336 px and trained at rank 8.
2. Round 0 scores the stock model on a fixed quick set: 120 card frames (every pick with a damaged twin included), the 24 twins, 160 drive test frames and 40 test edits. That's about ten minutes of eval.
3. Each round trains on the next slice of fresh rows (about an hour's worth), resuming the previous adapter, then scores the same quick set. `out/taste-tune/curve-NAME.json` grows a row per round with every metric, the validation loss and the speed, so the curve can be read, stopped or resumed between rounds.
4. When the curve flattens or the time budget runs out, the last adapter gets the full eval and the rendered edit check.

What we want out of the curve: whether cull and edit metrics move at all, how soon they stop moving, and whether the edit task drags the cull down or the other way round. If nothing moves after two rounds, that's a finding, and the 27B isn't worth a night.

Known limits of this run: the edit-features job (job B) was still writing plain renders when the data was built, so the training set holds fewer edits than the ~3,500 the full data will have. Resuming an adapter restarts AdamW's moments each round.

## Runs

- 2026-10-23, background-priority preflight on mlx-community Qwen3.5-9B-4bit (Apache-2.0): 46 s per step. The smoke eval passed (12 of 12, 2.7 s per photo). Background priority is far too slow for training, so the real runs use full power.
- 2026-10-23 7:48 PM ET: data rebuilt with the renders done so far: 1,378 cull and 815 edit training rows (the edits are a quarter of what the full set will hold). Full power with the GPU to itself runs 2.9 to 3.6 s per step, so a 467-row round takes about 25 minutes plus a 6-minute quick eval.

Round 0 is the stock 9B, on the quick set:

| | stock (r0) | after 467 rows (r1) |
|---|---|---|
| card pick AUC (120 frames) | 0.68 | 0.39 |
| card frames rejected / of them picks | 31 / 10 | 120 / 65 |
| damaged twin scored worse than original | 24 of 24 | 0 of 24 |
| drive reject AUC (160 test frames) | 0.47 | 0.55 |
| edit z-error (Auto 1.36, average offset 0.88) | 1.36 | 1.22 |

The stock model is a better culler than we had any right to expect. On this slice it ranks the card's picks as well as the shipped taste head does (0.66 on the full card), it catches every damaged twin, and its rejects are mostly frames Greg didn't pick. It can't tell the drive's rejects from his unflagged frames at all, and for edits it hands back Auto's numbers in a code block.

One round of training broke the cull. The adapter answers "reject" to every frame, even drive frames like the ones it trained on, and its scores now rank the card backwards: picks above non-picks, damaged twins below their originals. That's the same thing `cull_taste.py` found with a centroid head (a direction learnt from drive reject vs unflagged scores 0.36 on the card), so the label is the likely cause. A format mismatch could also explain the all-reject answers: eval asks with the thinking block turned off, and training may render the turn differently. Diagnostics after round 2 separate the two. Edits are learning, in the sense that they now come back as plain JSON in his range, but they still trail a constant average offset.

Round 2 (934 rows seen): card pick AUC 0.42, 82 of 120 card frames rejected (48 of them picks), 7 of 24 twins scored worse, drive reject AUC 0.60 across the test frames but 0.15 averaged within days, edit z-error 1.10, validation loss 0.13. The within-day number settles the cull question. Across days the adapter is learning which days hold rejects. Within a day it ranks Greg's rejects below his unflagged frames. So it learns what kind of day it is and nothing about the frame. The drive's reject vs unflagged label doesn't teach taste, to a VLM any more than to a centroid. The joint run stops after round 3. An edit-only adapter from the stock model takes over (`rounds --task edit`), and the cull stays with the stock model, which is already the better culler.

Round 3 (1,401 rows seen), the last joint round: card 0.42, twins 2 of 24, drive 0.57 overall and 0.28 within days, edit z-error back up to 1.21. Diagnostics on round 2 rule out the boring explanation. Eval prompt and training text are identical up to the answer (Qwen 3.5's template puts the empty think block in both), and turning `enable_thinking` off or on changes nothing. On 80 of its own training rows round 2 scores 0.997, while the stock model scores 0.38 on the same rows. So the adapter memorised its slice, and what it memorised doesn't carry to new days. The stock model's 0.38 has a story of its own: by its eye, Greg's drive rejects look better than the frames he left unflagged, which fits these rejects being whole moments he passed on rather than flawed frames.

**Edit-only adapter**, from the stock model, `--task edit`, 400-row rounds over the 815 edits we had renders for, scored on 40 test-day edits (z-scored slider error; on the same 40 rows Auto scores 1.36, Auto plus his average offset 0.88, and `whole_edit_render.py`'s ridge-scene 0.72):

| round (rows seen) | 0 | 1 (400) | 2 (800) | 3 (1,200) | 4 (1,600) | 5 (2,000) |
|---|---|---|---|---|---|---|
| z-error | 1.36 | 1.05 | 1.03 | 1.11 | 0.94 | 0.85 |
| validation loss | | 0.46 | 0.44 | 0.43 | 0.43 | 0.44 |

Round 6 (2,400 rows, the end of the third pass) reached 0.81 and is the adapter scored below (`edit-r6`).

It learns his look quickly. Contrast error falls from 47 to 17 points against Auto, and temperature from 19 to 11. After round 3 it looked stalled, with validation loss falling while the slider error rose, and I read that as memorising. That was wrong. Rounds 4 and 5, the second and third passes over the same 815 edits, kept improving, and round 5 is the first to beat the constant offset (0.85 against 0.88). It is still behind ridge-scene (0.72). Forty edits make a noisy yardstick, and picking the best round on it flatters that round, so the full 300-edit test and the rendered bench check decide. Its answers also sit on round numbers (contrast 60, tint 20) that a regression wouldn't pick. Two cheap things are lined up. First, self-consistency: average five sampled answers instead of one greedy answer (`eval --samples 5`). Second, more data: the remaining ~2,700 edits get renders by morning, and a fresh run on all of them (`edit-full`) shows whether the gap to ridge closes with data or stays put.

**The full test, 225 held-out edits** (z-error; lower is better):

| | z-error |
|---|---|
| Auto | 1.42 |
| Auto plus his average offset | 0.83 |
| fine-tuned 9B (edit-r6), one greedy answer | 0.82 |
| ridge-scene, trained on the same 815 edits | **0.70** |
| fine-tune and ridge averaged | **0.69** |

On the quick 40, averaging five sampled answers took the adapter from 0.81 to 0.73, within reach of ridge's 0.72 on those rows. That cost five times the eval time (23 s a photo).

Per slider, the two models are good at different things:

| slider | fine-tune | ridge | average offset |
|---|---|---|---|
| temp | **0.38** | 0.45 | 0.70 |
| tint | **0.49** | 0.52 | 0.87 |
| contrast | **0.74** | 0.82 | 0.67 |
| vibrance | **0.69** | 0.70 | 0.83 |
| exposure | 0.84 | **0.71** | 1.05 |
| whites | 1.26 | **0.78** | 0.94 |
| blacks | 1.00 | **0.64** | 0.86 |
| highlights | 1.17 | 1.02 | **1.01** |
| shadows | 0.90 | 0.77 | **0.67** |
| saturation | 0.71 | **0.59** | 0.66 |

The model that looks at the picture does best on white balance and color, the moves that depend on what's in the scene (skin, sky, tungsten light). Ridge reads Auto's numbers and the histogram and does best on the tone extremes (whites, blacks, exposure), which a VLM squinting at a 384 px JPEG can't measure well. That split suggests a clear design. Let the vision model propose temp, tint, contrast and vibrance, and keep the cheap head for the tone sliders. A per-slider pick like that would score 0.68 here, against 0.70 for ridge alone. That number is optimistic, because the split was chosen on these same rows. The rendered check on the bench decides whether this survives in pixels.

**In pixels**, `whole_edit_render.py --vlm` on the 33 test-day bench photos (dE to Greg's JPEG, lower is better; "beats" counts photos more than 0.25 dE closer than Auto):

| method | trained on | dE | median | beats Auto |
|---|---|---|---|---|
| no edit | | 11.21 | 10.82 | 15/33 |
| Auto | | 11.37 | 9.82 | |
| stock 9B | | 11.37 | 9.82 | 0/33 |
| fine-tune (edit-r6) | 815 pairs | 10.94 | 10.88 | 22/33 |
| ridge-scene | 4,557 pairs | 10.35 | 10.04 | 22/33 |
| ridge-emb-year | 4,557 pairs | 9.97 | 9.72 | 24/33 |
| shipped head (trained on this bench, in-sample) | | 9.54 | 9.06 | 25/33 |
| best possible Basic sliders (fit) | | 4.91 | 3.90 | 33/33 |

The stock row equals Auto because the stock model hands Auto's numbers back. The fine-tune beats Auto on two thirds of the bench, but this comparison isn't fair yet: the ridges trained on 4,557 pairs and the fine-tune on 815. A fresh fine-tune on all 4,821 edit pairs (`edit-full`) is the fair rematch, and it started at 2 AM ET.

**edit-full**, a fresh adapter from the stock model on all 4,821 edit pairs (500-row rounds, one pass, so every row is new). Scored on the new data's own quick 40, where Auto scores 1.34 and the average offset 0.95:

| round (rows seen) | 0 | 1 (500) | 2 (1,000) | 3 (1,500) | 4 (2,000) | 5 (2,500) | 6 (3,000) |
|---|---|---|---|---|---|---|---|
| z-error | 1.34 | 1.06 | 1.12 | 1.19 | 1.11 | **1.03** | 1.12 |
| validation loss | | 0.44 | 0.44 | 0.44 | 0.43 | 0.43 | 0.43 |

Six times the data, seen once, did no better than 815 pairs seen three times. It bounces around 1.03 to 1.19 and never beats the constant offset. So the 815-pair run's late gains came from repeated passes at a fixed learning rate, not from more examples. Our best guess is that the adapter needs several passes to settle on the numbers. One greedy text answer per photo is a noisy readout of what it has learned, which is also why averaging five samples helped so much. The full 225-edit and rendered checks on round 5 are running at background priority this morning.

**Final check, edit-full round 5**, on the same 225 test edits and 33 bench photos as the 815-pair adapter:

| | test z-error (225) | bench dE (33) | median | beats Auto |
|---|---|---|---|---|
| Auto | 1.42 | 11.37 | 9.82 | |
| average offset | 0.83 | | | |
| fine-tune, 815 pairs × 3 passes | **0.82** | 10.94 | 10.88 | 22/33 |
| fine-tune, 4,821 pairs × 1 pass | 0.92 | 11.08 | 9.42 | 20/33 |
| same, average of 5 answers | 0.89 | 10.86 | 9.97 | 20/33 |
| ridge-scene (5,050 pairs) | 0.74 | 10.22 | 9.79 | 22/33 |
| ridge-emb-year (5,050 pairs) | | 10.08 | 9.33 | 26/33 |
| shipped head (in-sample) | | 9.54 | 9.06 | 25/33 |

## Verdict (2026-10-24 morning)

The fine-tune doesn't earn a place, for either job.

Culling. The stock 9B is already a decent culler. It ranks Greg's 10-10 picks at AUC 0.68 on the quick slice and catches every damaged twin. Training it on his drive rejects made it worse in a way that teaches something: it memorised the training frames (0.997 in-sample) and learnt what kind of day holds rejects, which runs backwards within a day. That's the second model to show that label carries no frame-level taste. Any cull fine-tune needs a label that compares frames of the same moment, such as Greg's blind taps or burst keepers, and we have a few hundred of those at most.

Editing. A 9B fine-tune learns his look (contrast and white balance errors fall by half against Auto) and beats Auto on two thirds of the bench. It never catches a ridge on cheap features, in slider space or in pixels, and six times the data in one pass did worse than 815 pairs in three. It did win one thing: white balance and color (temp, tint, vibrance, contrast), where seeing the scene matters. That's a lead worth keeping, but a cheaper way to chase it is to feed the ridge a scene embedding or a few color facts, which `whole_edit_render.py`'s embedding variants already partly do.

Not worth a night: the 27B (the 9B shows no signal that size would fix), more passes over the full set (a ridge-sized gap at ~4 s a step), and shipping the adapter to Ollama. Worth keeping: `bench/taste_tune.py` itself. Its rounds, quick set and learning curve make any future VLM fine-tune a one-command overnight job, and the stock 9B's cull numbers suggest a stock-VLM cull bench is worth running on the full card.
