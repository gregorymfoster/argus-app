// Builds the Tauri updater manifest (latest.json) for one release.
// Usage: node latest-json.mjs --version 0.1.0 --repo owner/name --tag v0.1.0 \
//          --platform darwin-aarch64 --bundle Argus_0.1.0_aarch64.app.tar.gz --sig path/to/bundle.sig [--notes "..."]
import { readFileSync } from "node:fs";
import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";

export function latestJson({ version, repo, tag, platform, bundle, signature, notes = "", pubDate = new Date() }) {
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) throw new Error(`not a semver version: ${version}`);
  if (!signature.trim()) throw new Error("empty updater signature");
  return {
    version,
    notes,
    pub_date: pubDate.toISOString().replace(/\.\d{3}Z$/, "Z"),
    platforms: {
      [platform]: {
        signature: signature.trim(),
        url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(bundle)}`,
      },
    },
  };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const { values: a } = parseArgs({
    options: Object.fromEntries(
      ["version", "repo", "tag", "platform", "bundle", "sig", "notes"].map((k) => [k, { type: "string" }]),
    ),
  });
  const json = latestJson({ ...a, signature: readFileSync(a.sig, "utf8") });
  process.stdout.write(JSON.stringify(json, null, 2) + "\n");
}
