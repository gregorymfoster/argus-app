// Live Agent: a request in plain words goes to the engine's Agent (`agent` command), which
// edits through the tool layer as one batch; Undo reverts that batch (`undo_batch`). The
// chat only shows what came back: the tool calls as steps, and the Agent's answer.

import { call } from './backend';
import { chosenProvider, liveShootId } from './live';
import type { Recipe } from '../domain/types';

export interface AgentStep {
  tool: string;
  args: Record<string, unknown>;
  ok: boolean;
  error: string | null;
  ms: number;
}

export interface AgentReply {
  text: string;
  batch_id: number | null;
  recipes: number[];
  verdicts: number[];
  looks: boolean;
  steps: AgentStep[];
}

export interface UndoReport {
  label: string;
  recipes: number[];
  verdicts: number[];
  ratings?: number[];
  looks: number;
}

export interface EngineVersion {
  version: number;
  author: 'ai' | 'person';
  batch_id: number | null;
  label: string | null;
  at: string;
  recipe: Recipe;
}

export interface Turn {
  role: 'user' | 'agent';
  text: string;
}

/** Ask the Agent. `photoId` is the Photo in focus (open in Edit, or selected in the grid). */
export function askAgent(prompt: string, photoId: string | null, history: Turn[]): Promise<AgentReply> {
  return call<AgentReply>('agent', {
    prompt,
    shoot_id: liveShootId() || undefined,
    ...(photoId ? { photo_id: Number(photoId) } : {}),
    history: history.slice(-12),
    provider: chosenProvider(),
  });
}

export function undoBatch(batchId: number): Promise<UndoReport> {
  return call<UndoReport>('undo_batch', { batch_id: batchId });
}

export function listVersions(photoId: string): Promise<EngineVersion[]> {
  return call<EngineVersion[]>('list_versions', { photo_id: Number(photoId) });
}

/** A tool call as a chat step: "Edited 1 Photo", "Looked at the render". */
export function stepLabel(s: AgentStep): string {
  const ids = (s.args.photo_ids as unknown[] | undefined)?.length ?? 0;
  const photos = `${ids} Photo${ids === 1 ? '' : 's'}`;
  const set = Object.keys((s.args.set as Record<string, unknown>) ?? {}).join(', ');
  const label: Record<string, string> = {
    edit_recipe: `Edited ${photos}${set ? `: ${set}` : ''}`,
    write_recipe: `Wrote the Recipe on ${photos}`,
    render_photo: 'Looked at the render',
    get_preview: 'Looked at a Photo',
    get_contact_sheet: 'Read the Contact sheet',
    read_recipe: 'Read the Recipe',
    list_versions: 'Read the History',
    list_photos: 'Listed the Photos',
    list_shoots: 'Listed the Shoots',
    list_stacks: 'Read the Stacks',
    list_looks: 'Read the Looks',
    set_look: `Set the Look${set ? `: ${set}` : ''}`,
    sync_to_look: 'Synced to the Look',
    set_verdict: `Flagged ${photos} ${String(s.args.verdict ?? '')}`.trim(),
  };
  const text = label[s.tool] ?? s.tool;
  return s.ok ? text : `${text} (failed, retried)`;
}
