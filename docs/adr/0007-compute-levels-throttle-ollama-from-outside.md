# 7. Compute levels throttle Ollama from outside

Status: accepted (2026-10-18)

## Context

A local vision model cull pegs an M4 Max for 15 minutes or more. Greg works on the same Mac and found it unusable during a test run, but he's happy to let it run flat out overnight. He asked for a configurable setting.

On Apple Silicon, Ollama does almost all of the work on the GPU through Metal. Ollama's `num_thread` option only limits CPU threads, and macOS gives no way to cap another process's GPU share.

## Decision

A Compute level (Gentle, Balanced or Full) controls how much of the Mac a cull may take. Gentle is the default. While an Ollama cull runs on this Mac, a Throttle in `engine::compute` acts on Ollama's own processes. It targets the model processes (`llama-server` in Ollama 0.35, `ollama runner` before), or the server when none is loaded. It puts them at background QoS with `taskpolicy -b` and duty-cycles them with SIGSTOP and SIGCONT: Gentle runs for 4 s and pauses for 4 s, and asks Ollama for batches of 256. Balanced runs for 6 s and pauses for 2 s. Full leaves Ollama alone. Argus loads the model with exactly the cull's options before throttling, because any option difference makes Ollama reload, and a model paused while loading took 81 s instead of 4 s.

The level comes from `--compute` on `argus cull`, `compute` on `POST /api/cull`, or `ARGUS_COMPUTE`.

## Consequences

- Duty-cycling is the only lever that really bounds GPU time from outside the process. The price is that a Gentle cull takes at least twice as long.
- Pausing someone else's process is surprising. A kill skips Drop, which would leave Ollama frozen, so each Throttle spawns a watchdog shell in its own process group. When Argus exits, the watchdog sends SIGCONT and clears background QoS. Every Throttle also thaws Ollama when it starts, and `pkill -CONT -f llama-server` recovers by hand.
- A remote Ollama (when OLLAMA_HOST is not local) is never throttled.
- First measurement on Greg's M4 Max (Ollama 0.40.6, 3 photos): a 1 s / 1 s cycle took GPU use from 73% to 61% and system idle from 46% to 52%. Queued GPU work keeps running through a short pause, so the cycle became longer pauses with smaller batches. A second try at 1.5 s / 3 s with batches of 128 halved GPU use (93% to 48%), but culls took 7 times as long, because every restart costs time. That led to the current long cycles. Retune the numbers in `Compute::plan` from new measurements, and ask Greg whether Gentle feels smooth while he works.
