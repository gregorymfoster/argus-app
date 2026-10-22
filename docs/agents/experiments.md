# Harness experiments

How to try a change to the AI (prompts, takes, review rounds, model) cheaply, and why we tune the harness before we train anything.

## Harness first, training last (2026-10-19)

Greg asked whether to climb the benches by tuning the harness or fine-tuning the model, with only his MacBook to train on. Harness first. Nearly every gain so far came from it: the toolbox and named takes made Suggestions bolder, and keeping Auto's exposure and highlights on the Natural take closed most of its gap to Greg's edits. We have 14 edit cases and a few dozen blind picks, which a fine-tune would memorise. A LoRA pass on a 27B vision model takes hours on the Mac and ties it up, while a harness variant runs in the cloud on `claude-code` in minutes. Harness gains also survive a model upgrade; a fine-tune does not.

The order that makes sense is harness variants now, then small learned models on frozen image embeddings (a "Greg's taste" scorer trained on his picks and the usage log, as an Analyzer, which trains in minutes and can rank culls and judge variants), and LoRA on the vision model only once the usage log holds thousands of kept and rejected takes and harness changes stop moving the bench.

## Variant files

A variant is a JSON file that swaps parts of the harness without a rebuild (`engine/src/variant.rs`). Every field is optional and anything left out keeps the built-in value:

`name`, `notes`, `fresh` (the fresh Suggestions prompt, `{count}` filled in), `review` (review-round prompt, `{before}` filled in), `settings` (the settings reference appended to both), `takes` (a list of `{name, brief, start}`, where `start` holds moves on top of Auto in the same paths the model uses, e.g. `local.sky.exposure`), `rounds`, `keep_brightness`, `adaptive`, `style` (moves added to every fresh take in place of the Style profile learned from the usage log, `{}` for none; bench Libraries have no usage log, so it is empty there unless set), `exemplars` (how many of the person's own hand edits of similar scenes a fresh Suggestion shows the model, default 2, 0 for none; bench Libraries have none unless a run seeds them) `cull` (the cull system prompt) and `cull_taste` (`"signals"`: the model checks only technical failures and `engine/src/taste.rs` rates taste from the Facts; `"model"`: the model does the whole cull, as before 2026-10-20; unset it is `"model"` when the variant has its own `cull` prompt and `"signals"` otherwise, so `bench/variants/stars-first.json` and `cautious-rejects.json` still run as written). `bench/variants/whole-cull.json` is the built-in prompt with the model rating taste.

`ARGUS_VARIANT=<file>` applies it to any `argus` process, and `bench/score.py --variant <file>` sets that for the run, labels the run with the variant's name and stores the whole file in the run's `meta.variant`, so the report and later readers see exactly what ran. `think` and `auto` are described under Tuning with AI judges below. Unknown fields are refused and a bad file fails the job, so an experiment never quietly runs the default. Examples live in `bench/variants/`; copy the built-in prompt out of `engine/src/suggest.rs` when you want to edit it.

```sh
python3 bench/score.py --only edit --suggest --provider claude-code --variant bench/variants/natural-trio.json --case 2017-06-16_DSC03945 --case 2017-08-03_DSC04949
```

`--case` (repeatable) keeps a dev loop to a few Photos; run all cases once the variant looks worth it. A variant that wins moves into the code as the new default, with the bench numbers in a comment like the others in `suggest.rs`.

## Where to run what

A Qwen run's report is too big to bring across from the Mac; stage its score.py result (out/bench/<time>.json) instead and `python3 bench/rerender.py <result> <time>` re-renders every take from its Recipe on the bench RAWs, ready for `report.py add`.

Cloud sessions run `--provider claude-code` freely (about 20-30 s per call, a session limit after roughly ten full runs). Qwen runs need the Mac, which the main thread owns: batch several variants into one overnight request so one build scores them back to back.

dE to Greg's edits rewards timid edits, so treat it as a gut check. Greg's blind picks on the Edit Bench Review page are the judge (the `bench-report` skill).

## Screening a variant between Greg's taps

`python3 bench/judge.py runs out/bench/<time>/report/run.json <baseline run id>` puts each take of the variant run against the same-named take of the baseline (Natural vs Natural) on every photo, in both orders, and prints the judge's win rate with a 95% range per take. The judge is Claude (sonnet by default) calibrated on Greg's blind picks: about 70% agreement, where dE to his edit agrees 41% (the `bench-report` skill). Only a variant the judge prefers needs to go on the Edit Bench Review page for his taps. Baseline ids are in `/mnt/project-files/argus-bench/report/runs/index.json`; each call is `claude -p`, about 10 s, cached in `runs/judge.json`.

## Fake cull rejects

`python3 bench/cull_degrade.py [--provider claude-code] [--shoot DATE]` is a floor check for a cull variant. Greg's rejects were deleted, so the cull bench never sees a plainly bad frame; this gives a few of his picks per bench Shoot a damaged twin (focus blur, shake, 2 EV under, a crop that cuts off the top) with the same capture time, culls the Shoot, and counts twins the AI picked, put in Best Photos, or starred at or above their original. A good cull picks none. Passing says little about taste; failing is a plain bug. Its Shoots go to `out/cull-degrade/`, the result to `out/bench/cull-degrade-<time>.json`.

## Tuning with AI judges (bench/tune.py)

Greg asked (2026-10-20) to use the stronger models as judges to tune the harness around Qwen. They are a noisy screen: on his 222 blind picks Codex (ChatGPT) agrees 68%, Opus 67%, Sonnet 61%; on "an edit vs no edit" Codex 70% and Claude near chance (`/mnt/project-files/argus-bench/model-judges/WRITEUP.md`). So a sweep ranks variants, holds photos out to catch overfitting, and a winner still goes to Greg's taps before it becomes a default.

```sh
python3 bench/tune.py auto bench/sweeps/auto-1.json --cases 16 --holdout 16 --judge opus     # Auto dials, cloud
python3 bench/tune.py suggest SWEEP.json --provider claude-code --what take:Natural          # Suggestions variants
python3 bench/tune.py judge NEW.json BASE.json --what first --judge codex                    # two finished runs
```

A sweep file (`bench/sweeps/`) is a `base` variant plus named `variants` laid over it. Auto's dials (`auto::Params`: brightness target, whether RAWs get Exposure and tone, white balance strength, Highlights, Whites/Blacks and Vibrance gains) are a variant's `auto` field, so Auto tunes without a rebuild. `think` turns Qwen's thinking on for the cull, Suggestions and the Agent. Verdicts are cached by image content in `out/tune/`, every pair is judged in both orders, and `--judge opus,codex` counts a win only when both agree.

## Super edit (bench/super_edit.py)

Greg's "super edit" (2026-10-20): many subroutines bundled into one edit. Per Photo: K takes plus Auto and no edit as candidates, a knockout judged in both orders (calmest first, a split keeps the calmer one), then up to `--rounds` Agent critique-and-fix rounds that the judge must prefer. `--judge self,opus` runs one bracket and loop per judge from the same takes, to compare Qwen judging itself with an outside judge; `--think` lets Qwen reason. Judge the result's `super` take against its `first` take (what Suggestions show today) with an outside judge that wasn't in the loop, then with Greg's taps. A Qwen super edit costs roughly 30 min a Photo at Full, so it runs overnight on a handful of Photos.

What it found (2026-10-20 to 22, `/mnt/project-files/argus-bench/super-edit/`): the critique-and-fix rounds kept nothing in 12 tries on Claude, and Qwen thinking doubled the time and lost to the first take 0-4. Qwen can't judge its own edits: pairwise it keeps the unedited photo (it answers by slot), and scoring each take alone it said "no better than the original" on 152 of 160 candidates, where Codex preferred the takes to no edit 13-1. With the first take holding the title, Qwen scoring changed 1 of 20 Photos. Opus in the loop switched 3 of 6, but Opus and Codex judging afterwards disagreed about those switches. So no default changed. A super edit needs an outside judge, and the judges only agree with Greg about two in three, so his taps decide.

## Not yet

The Agent's prompt (`agent.rs` GUIDE) and the matched-Suggestion prompt aren't in variants yet. There is no cache of model replies, so re-scoring a run re-pays every call.

## Edit heads from Greg's finished edits

`bench/edit_head.py` (features, eval, page) fits the Basic sliders that best reproduce each of Greg's bench edits with `argus edit-fit` (`cli/src/edit_fit.rs`, hundreds of in-process renders per RAW), then scores rules and small heads that predict the offset from Auto on capture days they never saw. Results and caveats: `docs/research/edit-head.md`.
