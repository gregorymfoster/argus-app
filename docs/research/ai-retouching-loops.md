# How to get a vision model to edit photos well

Research for the Agent and Suggestions (2026-10-18). The question: what does the literature say about letting an LLM or VLM set Lightroom-style sliders, and should it render, look, critique and try again in a loop?

## What the papers found

**PhotoArtAgent** ([arXiv 2505.23130](https://arxiv.org/abs/2505.23130), CVPR 2026 Findings) is the closest match to Argus. A frontier VLM drives Lightroom's global sliders and HSL. Each round it perceives the photo (subject, mood, genre), plans, reads a histogram analysis (clipping, midtone density, RGB balance), writes JSON values, renders and reflects. Reflection is what makes it work: without it the agent was no better than learned baselines. 76% of images took more than one round, and most converged in two or three. Its margin over Lightroom Auto in their user study was small (6.50 vs 6.36 out of 10), which matches our bench, where Auto is hard to beat.

**RetouchLLM** ([arXiv 2510.08054](https://arxiv.org/html/2510.08054v2)) is training-free. A VLM critic sees content-similar reference edits plus numeric statistics (mean, median, percentiles, RGB means) and writes three difference descriptions, each turned into an edit. Two details matter for us. The unedited source stays in the candidate pool, so a round can never make things worse. And selection is done by an external score, not by the VLM. More candidates help (1 to 3 to 5 each add PSNR), more references help a lot, and gains come early. Its critic often fails to say stop.

**MonetGPT** ([arXiv 2505.06176](https://arxiv.org/abs/2505.06176), SIGGRAPH 2025, Adobe) found that asking a pretrained multimodal model to name an operation and its value cold works poorly. It edits in stages (light, then white balance, then HSL), re-rendering after each stage.

**JarvisArt** ([arXiv 2506.17612](https://arxiv.org/abs/2506.17612), NeurIPS 2025) trains Qwen2.5-VL-7B for Lightroom tools with a CIELAB-distance reward. **RetouchIQ** ([arXiv 2602.17558](https://arxiv.org/html/2602.17558)) reports that frontier models tend to over-edit. FiveK gaps between all these methods are tiny, so treat PSNR claims (and our own 14-case bench) as gut checks.

**Idea2Img** ([arXiv 2310.08541](https://arxiv.org/abs/2310.08541)) loops drafts, comparative selection and one piece of feedback per round for three rounds, and iteration clearly beat one shot. Its authors note models struggle to compare similar images.

On judging, three results set the limits. Pairwise (2AFC) judging by multimodal models works for coarse differences and fails on fine ones, with strong position bias in open models ([arXiv 2402.01162](https://arxiv.org/abs/2402.01162)). Model judges prefer their own outputs ([arXiv 2604.11589](https://arxiv.org/abs/2604.11589)). And in grounding tasks, iterative self-correction drifts: self-verification barely tracks quality, and no label-free stopping rule beat a single shot, although the better answer was usually somewhere in the trajectory ([arXiv 2606.13156](https://arxiv.org/abs/2606.13156)). Q-Bench ([arXiv 2309.14181](https://arxiv.org/abs/2309.14181)) and ColorBench ([arXiv 2504.10514](https://arxiv.org/abs/2504.10514)) say VLM perception of brightness and colour is unstable, which fits our finding that judging exposure by eye is the hard part.

Not verified beyond the abstract: RetouchAgent (AAAI 2026, retrieval of similar past edits plus reflection). No source measures directly whether a side-by-side before/after helps.

## What we take from it

1. Give the model numbers every time it looks. Every render now carries `light` (luma percentiles, clipped and crushed percent, five zones, mid-tone cast), because the model's eye for absolute brightness and colour is the weak link.
2. Start from a strong deterministic anchor. Auto is our best single number on the bench, so the model sees Auto's render and values and the natural take starts from them, changing only what it can see is wrong.
3. Perceive before choosing values. The reply schema puts a short description of what the photo needs before the slider values.
4. Loop a few times, then stop. Review rounds render, look, critique and adjust, fixing the main remaining problem each round, and stop early when the model is satisfied or its changes become tiny.
5. Never let the loop make things worse on its own say-so. Self-judging is unreliable, so a revision is rejected by a deterministic guard when it clips or crushes clearly more than the version it replaces, and the bench, not the model, decides whether the loop earns its time.
6. Exemplar conditioning (built 2026-10-20, `engine/src/exemplars.rs`). A fresh Suggestion shows the model up to two of the person's own hand edits of similar scenes (embedding similarity 0.35 or more, latest Recipe Version theirs, not rejected), rendered, with their settings. It is the biggest lever in RetouchLLM and how practitioners get an AI to edit "like me" (Glyn Dewis, 2026). It does nothing until the person has edited Photos by hand, and the bench can't score it until a run seeds a Library with Greg's edits held out per case. The wider survey behind it is `/mnt/project-files/research/ai-culling-editing-landscape.md`.

## What the bench said (2026-10-18, claude-code, 14 cases, dE to Greg's edits, no edit 8.31)

| | first take | best take | time per Photo |
|---|---|---|---|
| Before (no Auto anchor, no review) | 7.91 | 7.01 | |
| Auto anchor and "see" first | 7.54 | 6.93 | 35 s |
| Same, then 2 Review rounds (scored per round from one run) | 7.57 → 7.56 → 7.57 | 6.90 → 6.98 → 6.99 | 64 s |

The anchor is the win. Review rounds behaved as the self-correction papers predict: the model found real things to fix (clipped windows, warm casts) and called a third of the takes done, but the result moved no closer to Greg's edits while the time doubled. So the default was no Review round at Gentle and Balanced and one at Full. On 2026-10-20 the round was dropped at Full too: it made both Qwen runs slightly worse by dE (default characters first take 8.45 to 8.60), and on a 30-Photo Claude Natural trio run it moved the first take from 10.53 to 10.60 and the best from 9.64 to 9.72. Where it changed a take (34 of 90), the Claude judge preferred the unreviewed version 7-1, with 23 unsure. A bench against one person's past edits can't see a fix Greg wouldn't have made, so a Mac run with Qwen, where the loop may matter more for a weaker model, is the next check. A trustworthy external verifier (pairwise judging by a different model, or the person's own past edits of similar scenes) is the likelier way to make iteration pay.

## Why the first take lost to Auto, and the fix (2026-10-19)

On Greg's Mac, Qwen's first take scored 8.75 against no edit 7.36, and in the cloud Claude's first take was 7.82 against Auto 6.76 (no edit 8.31). Putting back Auto's value one slider at a time on the Claude takes showed where the gap came from: Exposure accounted for about two thirds of it and Highlights for most of the rest, while the model's colour, contrast and white and black point changes were a wash. The model pulled bright scenes toward the "mean 48" the prompt quotes (a beach that Greg left at mean 64 went down half a stop) and pulled Highlights 16 below Auto on average. So the natural take now keeps Auto's Exposure and Highlights (`keep_brightness` in `engine/src/suggest.rs`), in every review round too, and the bolder takes still choose their own. The same 14-case Claude run then scored first take 6.66 (was 7.82), best take 6.46 (was 7.27), best closer to Greg on 11 of 14 (was 7). On the 3 RAWs reachable from the cloud the first take still trails no edit (7.57 against 7.32), so the Mac re-score with Qwen on all 14 RAWs is what tells whether this is enough there.
