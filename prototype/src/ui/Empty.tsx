// An empty state: Greg's small glass art over one plain line and the next step (Greg, 2026-10-27).
// The art rises in once as the state appears; Reduce motion shows it still. Kept small and
// quiet so it never reads louder than the photos it stands in for.

import type { ReactNode } from 'react';
import { EMPTY_ART, type EmptyArt } from './emptyArt';

export function EmptyState({ art, title, children, testId }: { art: EmptyArt; title: ReactNode; children?: ReactNode; testId?: string }) {
  return (
    <div className="empty-state" data-testid={testId}>
      <img className={`empty-art ${art}`} src={EMPTY_ART[art]} alt="" draggable={false} />
      <div className="empty-title">{title}</div>
      {children && <div className="empty-body">{children}</div>}
    </div>
  );
}
