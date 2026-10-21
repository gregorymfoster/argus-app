import * as React from 'react';
import { ToggleGroup as TG } from 'radix-ui';
import { cn } from '@/lib/utils';

// Two looks (docs/design.md, Controls). `tray` is a form choice in a panel or Settings: a soft tray with the
// chosen item raised. `quiet` is a group in a header, tab row or bar: no tray, and only the active item gets
// the --chip fill, as in Claude's and Linear's headers.
type Look = 'tray' | 'quiet';
const LookContext = React.createContext<Look>('tray');

function ToggleGroup({ className, look = 'tray', ...props }: React.ComponentProps<typeof TG.Root> & { look?: Look }) {
  return (
    <LookContext.Provider value={look}>
      <TG.Root data-look={look} className={cn(look === 'tray' ? 'flex gap-0.5 rounded-[8px] bg-fill p-0.5' : 'flex gap-0.5', className)} {...props} />
    </LookContext.Provider>
  );
}

const ITEM = 'inline-flex h-6 cursor-pointer items-center gap-1.5 rounded-[6px] px-2.5 text-xs transition-colors duration-[120ms] [&_.mono]:text-text-3';
const TRAY = 'text-text-3 hover:text-text-2 data-[state=on]:bg-white/[0.1] data-[state=on]:text-text data-[state=on]:shadow-[inset_0_1px_0_rgb(255_255_255/0.06),0_1px_2px_rgb(0_0_0/0.3)] data-[state=on]:[&_.mono]:text-text-2';
const QUIET = 'h-[26px] px-2 text-text-2 hover:bg-hover hover:text-text data-[state=on]:bg-chip data-[state=on]:text-text';

function ToggleGroupItem({ className, ...props }: React.ComponentProps<typeof TG.Item>) {
  const look = React.useContext(LookContext);
  return <TG.Item className={cn(ITEM, look === 'tray' ? TRAY : QUIET, className)} {...props} />;
}

export { ToggleGroup, ToggleGroupItem };
