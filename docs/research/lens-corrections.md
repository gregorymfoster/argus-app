# Lens corrections from the camera's own data

Lightroom applies a lens profile to nearly every Photo. Argus can't ship lensfun's database, because it is CC-BY-SA. Sony cameras solve this for us: every ARW carries the distortion, vignetting and lateral chromatic aberration corrections the camera's own JPEG engine uses. `engine/src/lens.rs` reads them and applies them with no model and no licence question.

## Where the data lives

The data sits in the RAW image's IFD, which is IFD0's first SubIFD (tag 0x14a). The layout comes from ExifTool's public Sony tag tables. We didn't read darktable's GPL code or decompile anything.

| Tag | What | Shape |
|---|---|---|
| 0x7031 | vignetting applied to the camera JPEG | 257 or 272 = auto |
| 0x7032 | vignetting | int16 count (16), then 16 knots |
| 0x7034 | CA applied | 1 = auto |
| 0x7035 | lateral CA | count (32), then 16 red knots and 16 blue |
| 0x7036 | distortion applied | 17 on the A7R II with the FE 28mm F2, 0 on the A7R V with the FE 50mm F1.4 GM |
| 0x7037 | distortion | count (16), then 16 knots |

All of it falls in the first ~140 KB of the file, so `lens::read` reads only the first 1 MB.

## The model

Knot i sits at radius (i + 1) / n, where 0 is the centre and 1 is a corner. Inside the first knot the curve holds its first value. Between knots it's linear.

Distortion is a source-radius factor of 1 + d / 2^14. The output pixel at radius r samples the uncorrected image at r × factor. We fitted this against A7R II camera JPEGs, where the camera did apply distortion correction. Gradient NCC against the camera JPEG went from 0.64 and 0.63 (uncorrected) to 0.85 and 0.96 (corrected) on two frames. The opposite sign scored worse than no correction at all.

The A7R V JPEG with the 50mm GM isn't distortion-corrected (0x7036 = 0). The uncorrected render matched it at 0.95 and the corrected one at 0.83. That lines up with the flag, so the default follows the camera.

For vignetting, the light reaching the sensor at a knot is 2^(0.5 − 2^(v / 2^13 − 1)) of the centre's, and the correction divides by it. On the 28mm the corners get about 0.68 of the centre's light, so about +0.55 EV of correction. That's physically plausible for an f/2 wide-angle. The magnitude can't be checked against the camera JPEG, because DRO and the tone curve hide it there (the JPEG's measured corner gain was only 1.05 to 1.13). Expect Argus's corners to be a bit brighter than the camera's.

CA is a per-channel radius factor of 1 + c / 2^21 for red and blue. The effect is a fraction of a pixel, and the sign has only weak support from the fit (+1).

Distortion zooms in just far enough that no empty edge shows, as the camera does. The zoom comes from 65 samples along each edge.

## In the Recipe and the pipeline

`global.lensCorrections` takes "camera", "on" or "off". "camera" is the default and is also what an absent key means: it does what the camera did to its own JPEG. "on" applies everything the file carries, which is Lightroom's Enable Profile Corrections. "off" applies nothing. The Edit panel shows this as a three-way choice at the top of Optics, and it pastes with the Optics Settings group.

The corrections run on linear demosaiced camera RGB before colour and tone. Masks, crop, spots and effects all see the corrected frame. At preview sizes the corrected pixels are cached on the `Demosaiced` image, so slider drags don't warp again (edit_slider_ms unchanged at ~31 ms). At full size the image is demosaiced whole and corrected rows stream into colour and tone. A 61 MP export's develop goes from ~0.36 s to ~1.2 s on the 4-core cloud box.

The goldens were re-blessed when this landed. The A7R II fixtures get all three corrections, and the diffs are edges moving outward plus brighter corners.

## Open questions

AI Masks read Facts computed from the preview. Previews rendered before this change are uncorrected, so a Mask can sit a few pixels off near the corners until `rebuild_previews` runs. The Leica Q3's DNGs probably carry the same data as DNG WarpRectilinear and FixVignetteRadial opcodes, but nobody has checked yet. Other makes get no correction.
