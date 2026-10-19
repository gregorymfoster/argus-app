import type { ReactNode } from 'react';

/** One setting: its name and a line of what it does on the left, the control on the right. */
export function Row({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <div className="set-row">
      <div className="set-text">
        <div className="set-title">{title}</div>
        {note && <div className="set-note">{note}</div>}
      </div>
      <div className="set-control">{children}</div>
    </div>
  );
}
