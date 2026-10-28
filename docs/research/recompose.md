# Recompose: carrying Greg's crops across a Shoot

Greg's ask (2026-10-25, Shoot look flow): the batch edit should also handle framing, and on the decision card he chose "Recompose each photo", the AI cropping each frame the way he cropped the heroes, benched against his own crops first. This file is that bench, starting with how he actually crops.

## How Greg crops (2026-10-26)

Source: the Lightroom catalog copy in `/mnt/project-files/argus-pairs/interim-2026-10-22/catalog.jsonl`, which holds 8,767 edited RAWs with their full and cropped sizes, plus 2,704 XMP sidecars with crop rectangles and angles. This is data only; nothing was rendered or shown.

He rarely crops. Only 517 of the 8,767 edited RAWs (5.9%) have a crop. On 90 of the 168 days with 10 or more edited RAWs he cropped nothing, and on only 3 days did he crop more than half. Picks are cropped about as often as everything else (6.6%).

When he does crop, it's a punch-in at the camera's own shape. 506 of the 517 crops keep 3:2, and the crop keeps a median of 62% of the frame's area (quartiles 44% and 80%). Straightening is rarer still: 7 of the 92 cropped XMPs have an angle, a median of 2.9°.

Crops come in runs. A frame with another cropped frame within 2 minutes of it is cropped 24.6% of the time (1,576 frames). A frame without one is cropped 1.2% of the time (6,773 frames), a 20-fold difference. Within a day the crop's tightness varies less than across days (standard deviation of kept area 0.15 against 0.24 overall, over the 43 days with at least 4 crops). The 85 mm lens is cropped most (8.7%), then the 50 mm (5.9%) and the 28 mm (2.9%), and none of the 194 Leica Q3 frames. Cropped frames have a few more faces on average (2.4 against 1.8).

So a crop is a choice about a moment, not about a Shoot. Carrying the hero's crop to every frame of a Shoot would crop about 16 frames he'd leave alone for each one he'd crop. A useful recompose looks different: after he crops a frame, offer the same punch-in (3:2, about the same tightness, placed on that frame's own subject) to the frames shot around the same moment, as Suggestions he can accept or ignore.

## Copying the hero's crop to its neighbours (2026-10-26)

`bench/recompose_bench.py` on the interim pairs (3,083 frames with Lightroom settings, 108 cropped): each cropped frame is the hero for every frame of the same orientation shot within 2 minutes of it. Of those 531 offers, Greg cropped 166 (31%). On those 166, the whole frame (no crop) overlaps his crop at a mean IoU of 0.57, copying the hero's rectangle 0.62, and the hero's size centred 0.61. 45% of copies land at IoU 0.7 or better. So copying helps a little but misplaces the crop half the time: the subject moves between frames. The `shift` method (move the hero's rectangle by how the scene moved, measured by phase correlation on the small plain renders) needs the renders on the Mac; it recovers a synthetic 20 by 12 pixel move exactly.

## On all of Greg's pairs, with the scene's movement (2026-10-26)

The Mac ran the bench on the full pairs (8,517 frames with settings, 453 cropped) and the 384 px plain renders, in a few seconds. Of 2,748 offers to neighbours within 2 minutes, Greg cropped 1,068 (39%). On those:

| Method | Mean IoU | Median | IoU ≥ 0.7 |
|---|---|---|---|
| none (whole frame) | 0.626 | 0.658 | 47% |
| copy the hero's rectangle | 0.715 | 0.782 | 61% |
| hero's size, centred | 0.685 | 0.741 | 55% |
| shift by the scene's movement (898 with renders) | 0.741 | 0.810 | 68% |

On the same 898 frames copy scores 0.744, so following the scene's movement adds nothing: around a moment Greg reuses almost the same rectangle, and the frames barely move. Copying is the method; the open question is when to offer it. Offered to every frame of the same orientation within 2 minutes of a crop, it is what he did 39% of the time, a fair rate for a Suggestion he can accept or ignore, and far better than a Shoot-wide crop (6%).

Killed: phase-correlation shift (no gain over copy).

## Shipped: Crop like this (2026-10-27)

Greg said build it. A hand crop is offered as is to every frame of the Shoot with the same shape shot within 2 minutes (`engine/src/crop_offer.rs`, tool `offer_crop`, grid line with Accept all). A frame with a Shoot match waiting gets the crop inside that take. Accepting an offer changes only the crop, and accepting any other take keeps the frame's crop.

## Next

Copy wins, so a first version needs no subject boxes. Better placement (where on each frame's subject the crop should land) would need the crop rectangles together with what is in the frame: the subject and face boxes from Argus's analysis on the full-frame image. The crop rectangles live in the XMP sidecars of the RAW and edit pairs on Greg's Mac. Methods to score against his own crops (IoU of the crop rectangle, and whether he cropped at all):

1. No crop (the baseline he mostly agrees with).
2. Copy the hero's rectangle as is.
3. The hero's size and shape, centred where the hero's crop sat relative to its subject (face or person box), moved onto this frame's subject.
4. Method 3, offered only to frames within a few minutes of a cropped hero.
