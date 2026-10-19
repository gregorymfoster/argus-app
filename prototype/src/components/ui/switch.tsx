// A switch, as in shadcn/ui on Radix.
import * as React from 'react';
import { Switch as S } from 'radix-ui';
import { cn } from '@/lib/utils';

function Switch({ className, ...props }: React.ComponentProps<typeof S.Root>) {
  return (
    <S.Root
      className={cn('inline-flex h-[18px] w-[30px] flex-none cursor-pointer items-center rounded-full border border-border-strong bg-white/10 p-px transition-colors duration-[120ms] outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-default disabled:opacity-40 data-[state=checked]:border-transparent data-[state=checked]:bg-accent-strong', className)}
      {...props}
    >
      <S.Thumb className="block size-[14px] rounded-full bg-white shadow transition-transform duration-[120ms] data-[state=checked]:translate-x-3" />
    </S.Root>
  );
}

export { Switch };
