// Crop like this (Greg, 2026-10-27): after the person crops a Photo by hand, the engine offers the
// same crop to the frames of the same shape shot within 2 minutes of it (engine `crop_offer`),
// each as a take titled "Crop like <hero>". Nothing changes until they accept, and accepting one
// changes only the crop. A frame with a Shoot match ready gets the crop inside that take instead.

import { useMemo } from 'react';
import { toast } from 'sonner';
import { acceptMany, dismissSuggestion, useShootTakes } from './suggestions';
import { undoBatch } from './liveAgent';
import type { Suggestion } from '../domain/types';

/** The start of an offer's title (`crop_offer::PREFIX`). */
export const CROP_LIKE = 'Crop like ';

export interface CropOffers {
  /** Open offers, one per Photo, newest hero's first. */
  ready: Suggestion[];
  /** The Photo whose crop most of them carry, by file name. */
  hero: string | null;
}

export const isCropOffer = (s: Suggestion) => s.title.startsWith(CROP_LIKE);

export function useCropOffers(): CropOffers {
  const takes = useShootTakes();
  return useMemo(() => {
    const ready = Object.values(takes).flatMap((list) => list.filter((s) => s.status === 'open' && isCropOffer(s)).slice(0, 1));
    const count = new Map<string, number>();
    for (const s of ready) count.set(s.title, (count.get(s.title) ?? 0) + 1);
    const top = [...count].sort((a, b) => b[1] - a[1])[0]?.[0];
    return { ready, hero: top ? top.slice(CROP_LIKE.length) : null };
  }, [takes]);
}

/** Accept every offer as one Undo: each frame gets only the crop. */
export function acceptCropOffers(list: Suggestion[]) {
  if (!list.length) return;
  void acceptMany(list).then(({ batch }) => {
    window.dispatchEvent(new Event('argus:suggestions'));
    toast(list.length === 1 ? 'Crop accepted' : `Crop accepted on ${list.length} Photos`, {
      id: 'argus',
      action: batch == null ? undefined : { label: 'Undo', onClick: () => void undoBatch(batch).then(() => window.dispatchEvent(new Event('argus:suggestions'))) },
    });
  });
}

/** Not for these: dismiss every offer, so the same crop isn't offered to them again. */
export function dismissCropOffers(list: Suggestion[]) {
  void Promise.all(list.map((s) => dismissSuggestion(s))).then(() => window.dispatchEvent(new Event('argus:suggestions')));
}
