#!/usr/bin/env python3
"""Render every built-in Preset on every Photo of a Library, for looking at them side by side.

    scripts/preset-previews.py LIBRARY OUT [--edge 560] [--no-auto] [--only id,id]

Each Photo gets Auto first (as in the app, where a Preset stacks on Auto), then each Preset
at Amount 100 goes to OUT/<photo>/<preset>.jpg, with OUT/index.json listing them. Renders
run 8 at a time through `argus tool render_photo` (about 0.2 s each on the fast profile).
"""
import argparse, json, os, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ARGUS = os.path.join(ROOT, "target", "fast", "argus")


def tool(lib, name, args, out=None):
    cmd = [ARGUS, "tool", name, json.dumps(args), "--library", lib] + (["--out", out] if out else [])
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode:
        raise SystemExit(f"{name} failed: {r.stderr.strip()}")
    return r.stdout


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("library")
    ap.add_argument("out")
    ap.add_argument("--edge", type=int, default=560)
    ap.add_argument("--no-auto", action="store_true")
    ap.add_argument("--only", default="")
    a = ap.parse_args()
    presets = json.load(open(os.path.join(ROOT, "engine", "presets", "builtin.json")))["presets"]
    if a.only:
        keep = set(a.only.split(","))
        presets = [p for p in presets if p["id"] in keep]
    photos = json.loads(subprocess.run([ARGUS, "list", "--library", a.library, "--photos"], capture_output=True, text=True, check=True).stdout)
    if not a.no_auto:
        tool(a.library, "auto_edit", {"photo_ids": [p["id"] for p in photos]})
    jobs = []
    for ph in photos:
        stem = os.path.splitext(ph["file_name"])[0]
        os.makedirs(os.path.join(a.out, stem), exist_ok=True)
        jobs.append((ph["id"], None, os.path.join(a.out, stem, "_base.jpg")))
        for p in presets:
            jobs.append((ph["id"], p["id"], os.path.join(a.out, stem, p["id"] + ".jpg")))
    done = [0]

    def run(j):
        pid, preset, path = j
        args = {"photo_id": pid, "long_edge": a.edge}
        if preset:
            args["preset"] = preset
        tool(a.library, "render_photo", args, path)
        done[0] += 1
        if done[0] % 25 == 0 or done[0] == len(jobs):
            print(f"::progress {done[0]}/{len(jobs)} renders", flush=True)

    with ThreadPoolExecutor(8) as ex:
        list(ex.map(run, jobs))
    index = {"photos": [os.path.splitext(p["file_name"])[0] for p in photos], "presets": presets}
    json.dump(index, open(os.path.join(a.out, "index.json"), "w"), indent=1)
    print(f"wrote {len(jobs)} renders to {a.out}")


if __name__ == "__main__":
    sys.exit(main())
