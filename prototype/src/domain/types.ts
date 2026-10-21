// Domain types for the prototype. Names follow GLOSSARY.md so this can seed
// the real app's UI types.

export type Verdict = 'pick' | 'reject' | 'unflagged';
export type Actor = 'ai' | 'you';

export type SceneKind = 'lake' | 'forest' | 'cabin' | 'dusk' | 'street';

export interface Person {
  /** Horizontal centre, 0..1. */
  x: number;
  /** Relative size, around 1. */
  scale: number;
  skin: string;
  shirt: string;
  hair: string;
  eyesClosed: boolean;
  smile: boolean;
}

export interface Scene {
  kind: SceneKind;
  seed: number;
  people: Person[];
}

/** How the "camera" captured a frame: the faults culling looks for. */
export interface Capture {
  blur: number; // px of defocus at 1000px width
  motion: number; // px of motion smear
  ev: number; // exposure error
  wb: number; // -1 cool .. 1 warm cast
  faceShadow: number; // 0..1 how much faces sit in shadow
  shift: number; // small framing offset between burst frames
}

export interface FaceBox {
  /** Centre and radius as fractions of width (x, r) and height (y). */
  x: number;
  y: number;
  r: number;
  eyesClosed: boolean;
}

export interface Signals {
  focus: number;
  eyesOpen: [open: number, of: number];
  exposure: number;
  expression: 'Great' | 'Good' | 'Neutral' | 'Awkward';
}

export interface Photo {
  id: string;
  file: string;
  index: number;
  scene: Scene;
  capture: Capture;
  faces: FaceBox[];
  /** Fraction of height where the sky ends. */
  horizon: number;
  signals: Signals;
  aiVerdict: Verdict;
  reason: string;
  stackId?: string;
  /** Rank inside its Stack, 0 = best. */
  stackRank?: number;
  /** One line for headers: shutter · f · ISO · focal (camera · date · size when the
   *  original recorded no exposure settings). */
  exif: string;
  /** Live mode: the Loupe's Metadata rows, each left out when the camera didn't record it. */
  meta?: { camera: string; lens?: string; captured: string; size: string; exposure?: string; isoFocal?: string };
  /** Live: a stamp of the current Recipe (null if never edited), so the grid shows edits. */
  edit?: string | null;
  group: boolean;
  /** Live mode: the person's own Verdict already in the Library. */
  personVerdict?: Verdict;
  /** The AI's star Rating (1-5), and the person's own (0-5) when they set one. */
  aiStars?: number;
  personStars?: number;
  /** Rank in the Shoot's Best Photos (1 = best). */
  best?: number;
  /** Live mode: URL of the engine's preview JPEG, used instead of painting. */
  preview?: string;
  /** The original's size in pixels (live mode), for 100% zoom. */
  pixels?: [number, number];
}

export interface Stack {
  id: string;
  photoIds: string[];
}

export interface Shoot {
  id: string;
  name: string;
  count: number;
  live: boolean;
}

/** Slider values. Every field is optional so partial adjustments stack. */
export interface Adjust {
  exposure?: number; // EV, -3..3
  contrast?: number; // -100..100
  highlights?: number;
  shadows?: number;
  whites?: number;
  blacks?: number;
  temp?: number; // -100..100
  tint?: number;
  vibrance?: number;
  saturation?: number;
  fade?: number; // 0..100
  grain?: number; // 0..100
  skin?: number; // 0..100, natural smoothing only
  eyes?: number; // 0..100
  teeth?: number; // 0..100
  /** Color mixer, per band, -100..100. */
  hsl?: Partial<Record<HslBand, HslShift>>;
  /** Tone Curve: parametric regions and point curves (engine/src/recipe.rs `Curve`). */
  curve?: Curve;
  /** Color Grading wheels (engine `Grading`). */
  grading?: Grading;
  /** Calibration (engine `Calibration`). */
  calibration?: Calibration;
  /** Point Color: shifts for colours picked from the Photo. */
  pointColor?: PointColor[];
  // Effects (engine MORE_SLIDERS; absent = the default in brackets)
  texture?: number; // -100..100
  clarity?: number;
  dehaze?: number;
  vignette?: number; // post-crop, -100..100
  vignetteMidpoint?: number; // 0..100 [50]
  vignetteRoundness?: number; // -100..100
  vignetteFeather?: number; // 0..100 [50]
  vignetteHighlights?: number; // 0..100
  grainSize?: number; // 0..100 [25]
  grainRoughness?: number; // 0..100 [50]
  // Detail
  denoise?: number; // AI Denoise Amount 0..100, RAW only (engine/src/denoise.rs)
  sharpen?: number; // 0..150
  sharpenRadius?: number; // 0.5..3 [1]
  sharpenDetail?: number; // 0..100 [25]
  sharpenMasking?: number;
  noise?: number; // luminance noise reduction 0..100
  noiseDetail?: number; // [50]
  noiseContrast?: number;
  colorNoise?: number;
  colorNoiseDetail?: number; // [50]
  colorNoiseSmoothness?: number; // [50]
  // Optics
  /** Lens corrections from the camera's own data (engine lens.rs); absent = 'camera'. */
  lensCorrections?: 'camera' | 'on' | 'off';
  removeCA?: number; // 0 or 1
  defringePurple?: number; // 0..20
  defringeGreen?: number; // 0..20
  distortion?: number; // -100..100
  lensVignette?: number; // -100..100
  lensVignetteMidpoint?: number; // 0..100 [50]
}

/** A point curve: [input, output] pairs on 0..255. */
export type Points = [number, number][];
export type CurveChannel = 'rgb' | 'red' | 'green' | 'blue';
export interface Curve {
  highlights?: number;
  lights?: number;
  darks?: number;
  shadows?: number;
  splitShadows?: number; // [25]
  splitMidtones?: number; // [50]
  splitHighlights?: number; // [75]
  rgb?: Points;
  red?: Points;
  green?: Points;
  blue?: Points;
  refineSat?: number; // 0..100 [100]
}
/** Hue 0..360, sat 0..100, lum -100..100. */
export type Wheel = { hue?: number; sat?: number; lum?: number };
export type GradeRange = 'shadows' | 'midtones' | 'highlights' | 'global';
export type Grading = Partial<Record<GradeRange, Wheel>> & { blending?: number; balance?: number };
export type Primary = { hue?: number; sat?: number };
export interface Calibration {
  shadowsTint?: number;
  red?: Primary;
  green?: Primary;
  blue?: Primary;
}
export interface PointColor {
  hue: number;
  sat: number;
  lum: number;
  hueShift?: number;
  satShift?: number;
  lumShift?: number;
  range?: number;
}

/** What a `person` Mask selects, as Lightroom's Select People. */
export type PersonPart = 'face_skin' | 'body_skin' | 'eyes' | 'teeth' | 'hair' | 'whole';

export const HSL_BANDS = ['red', 'orange', 'yellow', 'green', 'aqua', 'blue', 'purple', 'magenta'] as const;
export type HslBand = (typeof HSL_BANDS)[number];
export type HslShift = { hue?: number; sat?: number; lum?: number };

/** The Adjust sliders that are plain numbers (all but the Color mixer). */
export type NumKey = Exclude<keyof Adjust, 'hsl' | 'curve' | 'grading' | 'calibration' | 'pointColor' | 'lensCorrections'>;

/** How a Mask is made; the engine's kinds (engine/src/masks/mod.rs). */
export type MaskRef = (
  | { kind: 'faces'; face?: number }
  | { kind: 'sky' }
  | { kind: 'subject' }
  | { kind: 'background' }
  | { kind: 'skin' }
  | { kind: 'hair' }
  | { kind: 'person'; person?: number; parts: PersonPart[] }
  | { kind: 'gradient'; side: 'left' | 'right' | 'bottom'; extent: number }
  | { kind: 'linear'; from: { x: number; y: number }; to: { x: number; y: number } }
  | { kind: 'radial'; x: number; y: number; rx: number; ry: number; feather?: number }
  | { kind: 'brush'; dots: { x: number; y: number; r: number }[]; erase?: { x: number; y: number; r: number }[] }
) & { invert?: boolean };

export interface LocalAdjust {
  id: string;
  name: string;
  mask: MaskRef;
  adjust: Adjust;
  source: Actor;
}

export interface Look {
  id: string;
  name: string;
  adjust: Adjust;
}

/** Per-Photo baseline under the Look. */
export interface Correction {
  exposure: number;
  wbK: number; // kelvin shift, negative = cooler
  face: number; // EV on faces
}

/** Straighten (degrees, + clockwise) then crop, as fractions of the straightened frame. */
export interface Crop {
  angle: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Recipe {
  schema: 1;
  global: Adjust;
  look: string | null;
  correction: Correction;
  local: LocalAdjust[];
  /** Absent means no crop. */
  crop?: Crop;
  /** Presets on this Recipe, in the order applied (engine/src/presets.rs). */
  presets?: AppliedPreset[];
  /** Heal and Clone Spots, on the uncropped frame; absent means none. */
  spots?: Spot[];
}

/**
 * One Heal or Clone circle (engine `heal::Spot`): pixels from `from` cover the circle at
 * x, y. x and y are 0..1 of the upright image; r is a fraction of its long edge.
 */
export interface Spot {
  id: string;
  mode: 'heal' | 'clone';
  x: number;
  y: number;
  r: number;
  from: { x: number; y: number };
  feather: number;
  opacity: number;
  source?: Actor;
}

/** A Preset on a Recipe: its Amount (0..2) and each setting it changed, before and after. */
export interface AppliedPreset {
  id: string;
  amount: number;
  before: Record<string, unknown>;
  after: Record<string, unknown>;
}

/** A complete edit the AI proposes for one Photo; nothing changes until it's accepted. */
export interface Suggestion {
  id: string;
  photoId: string;
  rank: number;
  title: string;
  why: string;
  recipe: Recipe;
  /** Set when it matches a Hero Photo's edit ("apply to the rest"). */
  referenceId: string | null;
  status: 'open' | 'accepted' | 'dismissed';
}

export interface Version {
  n: number;
  label: string;
  by: Actor;
  at: number;
  recipe: Recipe;
}
