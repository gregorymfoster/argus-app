import * as React from 'react';
import { Tooltip as TP } from 'radix-ui';
import { cn } from '@/lib/utils';

const TooltipProvider = TP.Provider;
const Tooltip = TP.Root;
const TooltipTrigger = TP.Trigger;

function TooltipContent({ className, sideOffset = 6, ...props }: React.ComponentProps<typeof TP.Content>) {
  return (
    <TP.Portal>
      <TP.Content sideOffset={sideOffset} className={cn('anim-pop z-50 flex items-center gap-2 rounded-md border border-border-strong bg-glass py-1 backdrop-blur-xl pl-2 pr-1.5 text-[11.5px] text-text shadow-pop', className)} {...props} />
    </TP.Portal>
  );
}

export { TooltipProvider, Tooltip, TooltipTrigger, TooltipContent };
