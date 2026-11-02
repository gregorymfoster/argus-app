#!/usr/bin/env python3
"""Fail when engine code holds a lock (or runs a one-time init) across parallel work.

The 0.1.17 Apply look hang (fixed 4bee11c): Lens::corrected held a Mutex while correct() ran
on rayon. A rayon worker waiting inside that parallel call steals other queued work; when the
stolen work wants the same lock, every worker ends up parked for good. The same goes for a
OnceLock/Lazy initialiser that goes parallel. See docs/agents/concurrency.md.

    scripts/lock-lint.py [FILES...]      default: every .rs under engine/src, cli/src, app/src

What it flags, by reading the source (no compiler, so it is deliberately simple):
  * a guard bound with `let g = <x>.lock()/.write()/.read()...;` and, before the end of its
    block or `drop(g)`, a parallel call: rayon (`par_*`, `into_par_iter`, `install`, `join`,
    `scope`) or a call to any function that (transitively) contains one;
  * the same inside a `get_or_init(...)` / `get_or_try_init(...)` closure;
  * a temporary guard `<x>.lock().unwrap().f(...)` whose statement makes such a call.
Calls are matched by function name across the crate (plain and path calls like `correct(..)` or
`lens::correct(..)`; method calls are too ambiguous by name and only count when rayon's own),
so a name shared with a parallel function can be a false alarm: move the work out of the lock (best), or mark the line before the guard
with `// lock-lint: ok <why>`.
"""
import os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RAYON = re.compile(r"\.(par_iter|par_iter_mut|into_par_iter|par_chunks|par_chunks_mut|par_chunks_exact|par_chunks_exact_mut|par_bridge|par_extend|par_sort\w*|par_lines|par_split\w*|install)\s*\(|\brayon::(join|scope|spawn)\b")
GUARD = re.compile(r"^\s*let\s+(?:mut\s+)?(\w+)\s*(?::[^=]+)?=\s*([^;]*?)\.(lock|write|read)\(\)\s*(\.unwrap\(\)|\.unwrap_or_else\([^;]*\)|\.expect\([^;]*\)|\?)?\s*;")
TEMP = re.compile(r"\.(lock|write|read)\(\)\s*(\.unwrap\(\)|\.unwrap_or_else\(\|\w+\|\s*\w+\.into_inner\(\)\)|\.expect\([^)]*\))\s*\.")
INIT = re.compile(r"\.get_or(_try)?_init\s*\(")
FN = re.compile(r"\bfn\s+(\w+)\s*(<[^{;]*?>)?\s*\(")
# Names too common to trust a match on (std methods, trait methods every type has).
COMMON = {"new", "get", "run", "call", "from", "into", "map", "len", "clone", "default", "fmt", "drop", "next", "load", "open", "read", "write", "lock", "send", "insert", "push", "apply", "with", "set", "build", "main", "test"}


def strip(src):
    """Blank out comments and string/char literals, keeping offsets and line breaks."""
    out, i, n = list(src), 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j): out[k] = " "
            i = j
        elif src.startswith("/*", i):
            j = src.find("*/", i + 2)
            j = n if j < 0 else j + 2
            for k in range(i, j):
                if src[k] != "\n": out[k] = " "
            i = j
        elif c == '"' or (c == "r" and re.match(r'r#*"', src[i:i + 4]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_"))):
            m = re.match(r'r(#*)"', src[i:]) if c == "r" else None
            end = '"' + m.group(1) if m else '"'
            j = i + (len(m.group(0)) if m else 1)
            while j < n:
                if not m and src[j] == "\\": j += 2; continue
                if src.startswith(end, j): j += len(end); break
                j += 1
            for k in range(i + 1, min(j, n) - 1):
                if src[k] != "\n": out[k] = " "
            i = j
        elif c == "'" and re.match(r"'(\\.|[^\\'])'", src[i:i + 4]):
            m = re.match(r"'(\\.|[^\\'])'", src[i:])
            i += len(m.group(0))
        else:
            i += 1
    return "".join(out)


def block_end(s, start):
    """Offset of the brace closing the block that contains `start`."""
    depth = 0
    for k in range(start, len(s)):
        if s[k] == "{": depth += 1
        elif s[k] == "}":
            if depth == 0: return k
            depth -= 1
    return len(s)


def match_paren(s, open_at):
    depth = 0
    for k in range(open_at, len(s)):
        if s[k] in "([{": depth += 1
        elif s[k] in ")]}":
            depth -= 1
            if depth == 0: return k
    return len(s)


def expr_end(s, start):
    """Where the expression holding a temporary guard ends: `;` or `,` at its own depth, or the
    bracket that closes around it (the guard is dropped there)."""
    depth = 0
    for k in range(start, len(s)):
        c = s[k]
        if c == "{" and depth == 0: return match_paren(s, k)  # `if let .. = guard.. { .. }` keeps it to the block's end
        if c in "([{": depth += 1
        elif c in ")]}":
            if depth == 0: return k
            depth -= 1
        elif c in ";," and depth == 0: return k
    return len(s)


def functions(s):
    """(name, body_start, body_end) for every fn with a body."""
    out = []
    for m in FN.finditer(s):
        close = match_paren(s, s.find("(", m.end() - 1))
        k = close
        while k < len(s) and s[k] not in "{;": k += 1
        if k < len(s) and s[k] == "{":
            out.append((m.group(1), k, match_paren(s, k)))
    return out


def main(files):
    srcs = {f: strip(open(f, encoding="utf-8").read()) for f in files}
    raw = {f: open(f, encoding="utf-8").read().split("\n") for f in files}
    fns = [(f, name, a, b) for f, s in srcs.items() for name, a, b in functions(s)]
    parallel = {name for f, name, a, b in fns if RAYON.search(srcs[f][a:b])}
    while True:  # fixpoint: a function calling a parallel function is parallel
        pat = re.compile(r"(?<![.\w])(" + "|".join(sorted(re.escape(n) for n in parallel - COMMON)) + r")\s*(::<[^>]*>)?\s*\(") if parallel - COMMON else None
        more = {name for f, name, a, b in fns if name not in parallel and pat and pat.search(srcs[f][a + 1:b])}
        if not more: break
        parallel |= more
    calls = re.compile(r"(?<![.\w])(" + "|".join(sorted(re.escape(n) for n in parallel - COMMON)) + r")\s*(::<[^>]*>)?\s*\(") if parallel - COMMON else re.compile(r"$^")

    def parallel_in(text):
        m = RAYON.search(text) or calls.search(text)
        return m.group(0).strip("(. ") if m else None

    problems = []
    for f, s in srcs.items():
        lines = raw[f]
        line_at = lambda off: s.count("\n", 0, off) + 1
        allowed = lambda ln: any("lock-lint: ok" in lines[k] for k in range(ln - 4, ln) if 0 <= k < len(lines))
        for m in GUARD.finditer(s, 0) if False else re.finditer(r"(?m)^.*$", s):
            g = GUARD.match(m.group(0))
            if not g: continue
            ln = line_at(m.start())
            if allowed(ln): continue
            var = g.group(1)
            end = block_end(s, m.end())
            d = re.search(r"\bdrop\(\s*" + re.escape(var) + r"\s*\)", s[m.end():end])
            if d: end = m.end() + d.start()
            hit = parallel_in(s[m.end():end])
            if hit: problems.append(f"{os.path.relpath(f, ROOT)}:{ln}: `{var}` holds a lock across parallel work (`{hit}`)")
        for m in INIT.finditer(s):
            ln = line_at(m.start())
            if allowed(ln): continue
            hit = parallel_in(s[m.end():match_paren(s, m.end() - 1)])
            if hit: problems.append(f"{os.path.relpath(f, ROOT)}:{ln}: a one-time init runs parallel work (`{hit}`); load before the parallel loop instead")
        for m in TEMP.finditer(s):
            ln = line_at(m.start())
            if allowed(ln): continue
            hit = parallel_in(s[m.end() - 1:expr_end(s, m.end())])
            if hit: problems.append(f"{os.path.relpath(f, ROOT)}:{ln}: a lock is held for a statement that runs parallel work (`{hit}`)")
    for p in sorted(set(problems)): print(p)
    if problems:
        print(f"\nlock-lint: {len(set(problems))} place(s) hold a lock across parallel work; see docs/agents/concurrency.md", file=sys.stderr)
        return 1
    print(f"lock-lint: ok ({len(parallel)} parallel functions, {len(files)} files)")
    return 0


if __name__ == "__main__":
    files = sys.argv[1:] or [os.path.join(d, f) for top in ("engine/src", "cli/src", "app/src") for d, _, fs in os.walk(os.path.join(ROOT, top)) for f in fs if f.endswith(".rs")]
    sys.exit(main(files))
