# Export

How Export works and why its defaults are what they are. Code: `engine/src/export.rs` (the `export_photos` and `add_to_apple_photos` tools), `engine/src/icc.rs`, `engine/src/watermark.rs`, UI in `prototype/src/ui/Export.tsx`.

## Recommended preset (Greg's A7R V, 2026-11-02)

**JPEG, quality 85, full size, sRGB, no output sharpening.** It's the first preset and the default for new Libraries.

We measured it on four A7R V frames (61 MP, 9504×6336) rendered by Argus, comparing each JPEG to a lossless TIFF of the same render:

| Quality | Size (avg) | Bytes/pixel | SSIM (luma) | PSNR |
|---|---|---|---|---|
| 75 | 3.5 MB | 0.057 | 0.970 | 44.7 dB |
| 80 | 4.1 MB | 0.069 | 0.974 | 45.2 dB |
| 85 | 5.2 MB | 0.086 | 0.977 | 45.9 dB |
| 90 | 7.2 MB | 0.120 | 0.982 | 46.9 dB |
| 95 | 12.2 MB | 0.203 | 0.987 | 48.4 dB |

Past 85 the file grows fast (85 to 90 is +39% for +1 dB, 90 to 95 is +70% for +1.5 dB) and the gain is invisible at normal viewing. At 85 a full 61 MP frame is only about 5 MB, so there's no reason to shrink it: keep every pixel for cropping and printing later, and let the phone or website downsize. Output sharpening is for downsized exports (the Web and Instagram presets use it); at full size it only adds halos. sRGB stays the default because it looks the same in every app; choose Display P3 for files that stay on Apple devices. Sample: n = 4 frames, one shoot, so treat the bytes/pixel as a rough guide (the Estimate learns real numbers after one export).

## Display P3

A P3 export swaps the camera matrix for camera to linear P3 (`pipeline::Primaries`), so colours past sRGB survive instead of clipping, and embeds an ICC v4 Display P3 profile (APP2) built in `icc.rs`. Checked against littlecms: sRGB red comes out as (234, 51, 35) in P3, as expected, and an A7R V render converted back to sRGB matches the sRGB export within 0.8 levels on average. JPEG and HEIC only; TIFF stays sRGB.

## Watermark

White text with a soft dark shadow in a corner, sized as a percentage of the short edge, drawn with the Mac's system font (no font files ship with Argus). Never drawn on an Original.

## Send to photo apps (Apple Photos, Google Photos)

Greg, 2026-11-02: "i also wish it could upload to my iphotos and google photos trivially." Both are a tick in the Export dialog's Send to section and a switch in Settings › Export (the same setting), and after an export the files go into an album named after the Shoot. Neither blocks the export: the tool `send_to_photos {to, paths, album?, shoot_id?}` queues a `send_photos` job, so the upload shows in the Status bar and Work mode with a bar, runs at the open Photo's priority (it is what the person just asked for, and network-bound), and works through at most two minutes of files per job before queueing the rest, so a big upload never holds the AI work up.

Apple Photos runs AppleScript through `osascript` (`add_to_apple_photos`, 20 files per call so the bar moves). The first run makes macOS ask to let Argus control Photos; a refusal comes back as a clear error pointing at System Settings › Privacy & Security › Automation.

Google Photos uses the Library API (`engine/src/google_photos.rs`). Google cut that API back on 2025-03-31: an app can no longer read the person's library, but the `photoslibrary.appendonly` scope still uploads, creates albums and adds to albums the app made, which is all Argus asks for. Argus never lists albums; it remembers the album it made for each name in the Library setting `google_photos_albums` and makes it again if the person deleted it. Sign-in is OAuth in the browser (loopback redirect with PKCE). No Google key ships with Argus (Greg's rule: no baked-in keys), so the person makes a free Desktop OAuth client once in their own Google Cloud project (enable the Photos Library API, External consent screen with themselves as a test user and then Publish app, because a Testing app's sign-in lapses after 7 days; then Credentials › OAuth client ID › Desktop app) and pastes the ID and secret into Settings › Export. The client and refresh token live in `~/Library/Application Support/Argus/google-photos.json` (mode 0600), not in the Library, so they never travel with a Library backup. The test (`engine/tests/google_photos.rs`) runs the whole sign-in and upload against a fake Google on a loopback port.
