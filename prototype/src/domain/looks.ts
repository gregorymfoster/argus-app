import type { Look } from './types';

const LOOKS: Look[] = [
  { id: 'warm-film', name: 'Warm film', adjust: { temp: 30, tint: 3, contrast: -12, fade: 18, grain: 22, saturation: -6 } },
  { id: 'clean-natural', name: 'Clean natural', adjust: { temp: 4, contrast: 6, saturation: 4, shadows: 10 } },
  { id: 'cool-moody', name: 'Cool moody', adjust: { temp: -32, contrast: 22, saturation: -18, shadows: -12, fade: 8 } },
  { id: 'bright-airy', name: 'Bright and airy', adjust: { exposure: 0.35, temp: 6, contrast: -18, shadows: 26, saturation: -4, fade: 10 } },
];

export const LOOK_BY_ID: Record<string, Look> = Object.fromEntries(LOOKS.map((l) => [l.id, l]));
