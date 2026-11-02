// Cull Rounds (GLOSSARY: Round): repeat Cull passes over the Picks (Greg, 2026-11-01: "take
// all the picks, restack them, and let me run through them one more time"). Pass 1 is the
// flag. Pass k >= 2 runs over the Photos that reached Round k-1 and marks each kept or cut.
// The same rule as the engine's `library::round_of`.

/** A Photo's marks for pass 2 and on: true kept, false cut. */
export type Marks = Record<number, boolean>;

export interface Round {
  /** 0 not a Pick, 1 a Pick, k kept in passes 2 to k as well. */
  round: number;
  /** The pass that cut it, if one did. */
  cutIn: number | null;
}

export function roundOf(pick: boolean, marks: Marks = {}): Round {
  if (!pick) return { round: 0, cutIn: null };
  let round = 1;
  for (;;) {
    const m = marks[round + 1];
    if (m === true) round++;
    else return { round, cutIn: m === false ? round + 1 : null };
  }
}

/** The marks a Round stands for (what the Library's `round` and `cut_in` say). */
export function marksFrom(round: number, cutIn: number | null | undefined): Marks {
  const m: Marks = {};
  for (let p = 2; p <= round; p++) m[p] = true;
  if (cutIn) m[cutIn] = false;
  return m;
}

/** In the running for pass `pass`: a Pick that reached Round pass-1. Pass 1 is everyone. */
export const inPass = (r: Round, pass: number) => pass <= 1 || r.round >= pass - 1;
