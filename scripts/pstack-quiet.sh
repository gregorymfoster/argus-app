#!/bin/sh
# SessionStart hook: keeps the pstack plugin opt-in (Greg, 2026-10-21). pstack's own hook
# tells every session to route work through poteto-mode unless its sheet says
# `session hook: off`; this writes that line once, on the Mac and in the cloud alike.
# A sheet that already has a `session hook` line is left alone, so `setup-pstack` can turn it on.
sheet="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/pstack-models.md"
grep -q '^session hook:' "$sheet" 2>/dev/null && exit 0
mkdir -p "$(dirname "$sheet")" 2>/dev/null || exit 0
[ -f "$sheet" ] || printf '# pstack model configuration\n\n' > "$sheet"
printf 'session hook: off\n' >> "$sheet" 2>/dev/null
exit 0
