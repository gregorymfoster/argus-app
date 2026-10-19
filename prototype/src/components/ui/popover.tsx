// A popover with the menus' glass look (shadcn/ui on Radix).
import * as React from 'react';
import { Popover as P } from 'radix-ui';
import { cn } from '@/lib/utils';

const Popover = P.Root;
const PopoverTrigger = P.Trigger;
function PopoverContent({ className, sideOffset = 6, ...props }: React.ComponentProps<typeof P.Content>) {
  return (
    <P.Portal>
      <P.Content sideOffset={sideOffset} className={cn('anim-pop z-50 w-[320px] rounded-lg border border-border-strong bg-glass p-3 text-[12px] shadow-pop backdrop-blur-2xl backdrop-saturate-150 outline-none', className)} {...props} />
    </P.Portal>
  );
}

export { Popover, PopoverTrigger, PopoverContent };
