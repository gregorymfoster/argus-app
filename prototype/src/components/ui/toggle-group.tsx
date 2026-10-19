import * as React from 'react';
import { ToggleGroup as TG } from 'radix-ui';
import { cn } from '@/lib/utils';

function ToggleGroup({ className, ...props }: React.ComponentProps<typeof TG.Root>) {
  return <TG.Root className={cn('flex gap-0.5 rounded-[8px] bg-fill p-0.5', className)} {...props} />;
}

function ToggleGroupItem({ className, ...props }: React.ComponentProps<typeof TG.Item>) {
  return (
    <TG.Item
      className={cn('inline-flex h-6 cursor-pointer items-center gap-1.5 rounded-[6px] px-2.5 text-xs text-text-3 transition-colors duration-[120ms] hover:text-text-2 data-[state=on]:bg-white/[0.1] data-[state=on]:text-text data-[state=on]:shadow-[inset_0_1px_0_rgb(255_255_255/0.06),0_1px_2px_rgb(0_0_0/0.3)] [&_.mono]:text-text-3 data-[state=on]:[&_.mono]:text-text-2', className)}
      {...props}
    />
  );
}

export { ToggleGroup, ToggleGroupItem };
