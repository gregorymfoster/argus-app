import * as React from 'react';
import { Checkbox as CB } from 'radix-ui';
import { CheckIcon } from 'lucide-react';
import { cn } from '@/lib/utils';

function Checkbox({ className, ...props }: React.ComponentProps<typeof CB.Root>) {
  return (
    <CB.Root className={cn('grid size-4 shrink-0 cursor-pointer place-items-center rounded-[4px] border border-white/20 bg-black/20 transition-colors duration-[120ms] hover:border-white/30 focus-visible:ring-[1.5px] focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-bg data-[state=checked]:border-accent-strong data-[state=checked]:bg-accent-strong data-[state=checked]:text-on-accent', className)} {...props}>
      <CB.Indicator>
        <CheckIcon size={12} strokeWidth={3} />
      </CB.Indicator>
    </CB.Root>
  );
}

export { Checkbox };
