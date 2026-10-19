import { test } from "node:test";
import assert from "node:assert/strict";
import { latestJson } from "./latest-json.mjs";

const base = {
  version: "0.2.0",
  repo: "gregorymfoster/argus-releases",
  tag: "v0.2.0",
  platform: "darwin-aarch64",
  bundle: "Argus_0.2.0_aarch64.app.tar.gz",
  signature: "c2lnbmF0dXJl\n",
  pubDate: new Date("2026-10-18T03:00:00.123Z"),
};

test("points the platform at the release asset with a trimmed signature", () => {
  assert.deepEqual(latestJson(base), {
    version: "0.2.0",
    notes: "",
    pub_date: "2026-10-18T03:00:00Z",
    platforms: {
      "darwin-aarch64": {
        signature: "c2lnbmF0dXJl",
        url: "https://github.com/gregorymfoster/argus-releases/releases/download/v0.2.0/Argus_0.2.0_aarch64.app.tar.gz",
      },
    },
  });
});

test("rejects a version the updater can't compare", () => {
  assert.throws(() => latestJson({ ...base, version: "v0.2.0" }), /semver/);
});

test("rejects an empty signature", () => {
  assert.throws(() => latestJson({ ...base, signature: " \n" }), /signature/);
});
