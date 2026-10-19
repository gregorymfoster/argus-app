# Harness experiments

How to try a change to the AI (prompts, takes, review rounds, model) cheaply, and why we tune the harness before we train anything.

## Harness first, training last (2026-10-19)

Greg asked whether to climb the benches by tuning the harness or fine-tuning the model, with only his MacBook to train on. Harness first. Nearly every gain so far came from it: the toolbox and named takes made Suggestions bolder, and keeping Auto's exposure and highlights on the Natural take closed most of its gap to Greg's edits. We have 14 edit cases and a few dozen blind picks, which a fine-tune would memorise. A LoRA pass on a 27B vision model takes hours on the Mac and ties it up, while a harness variant runs in the cloud on `claude-code` in minutes. Harness gains also survive a model upgrade; a fine-tune does not.

The order that makes sense is harness variants now, then small learned models on frozen image embeddings (a "Greg's taste" scorer trained on his picks and the usage log, as an Analyzer, which trains in minutes and can rank culls and judge variants), and LoRA on the vision model only once the usage log holds thousands of kept and rejected takes and harness changes stop moving the bench.

## Variant files

A variant is a JSON file that swaps parts of the harness without a rebuild (`engine/src/variant.rs`). Every field is optional and anything left out keeps the built-in value:

`name`, `notes`, `fresh` (the fresh Suggestions prompt, `{count}` filled in), `review` (review-round prompt, `{before}` filled in), `settings` (the settings reference appended to both), `takes` (a list of `{name, brief, start}`, where `start` holds moves on top of Auto in the same paths the model uses, e.g. `local.sky.exposure`), `rounds`, `keep_brightness`, `adaptive`, and `cull` (the cull system prompt).

`ARGUS_VARIANT=<file>` applies it to any `argus` process, and `bench/score.py --variant <file>` sets that for the run, labels the run with the variant's name and stores the whole file in the run's `meta.variant`, so the report and later readers see exactly what ran. Unknown fields are refused and a bad file fails the job, so an experiment never quietly runs the default. Examples live in `bench/variants/`; copy the built-in prompt out of `engine/src/suggest.rs` when you want to edit it.

```sh
python3 bench/score.py --only edit --suggest --provider claude-code --variant bench/variants/natural-trio.json --case 2017-06-16_DSC03945 --case 2017-08-03_DSC04949
```

`--case` (repeatable) keeps a dev loop to a few Photos; run all cases once the variant looks worth it. A variant that wins moves into the code as the new default, with the bench numbers in a comment like the others in `suggest.rs`.

## Where to run what

Cloud sessions run `--provider claude-code` freely (about 20-30 s per call, a session limit after roughly ten full runs). Qwen runs need the Mac, which the main thread owns: batch several variants into one overnight request so one build scores them back to back.

dE to Greg's edits rewards timid edits, so treat it as a gut check. Greg's blind picks on the Edit Bench Review page are the judge (the `bench-report` skill).

## Screening a variant between Greg's taps

`python3 bench/judge.py runs out/bench/<time>/report/run.json <baseline run id>` puts each take of the variant run against the same-named take of the baseline (Natural vs Natural) on every photo, in both orders, and prints the judge's win rate with a 95% range per take. The judge is Claude (sonnet by default) calibrated on Greg's blind picks: about 70% agreement, where dE to his edit agrees 41% (the `bench-report` skill). Only a variant the judge prefers needs to go on the Edit Bench Review page for his taps. Baseline ids are in `/mnt/project-files/argus-bench/report/runs/index.json`; each call is `claude -p`, about 10 s, cached in `runs/judge.json`.

## Not yet

The Agent's prompt (`agent.rs` GUIDE) and the matched-Suggestion prompt aren't in variants yet. There is no cache of model replies, so re-scoring a run re-pays every call.
