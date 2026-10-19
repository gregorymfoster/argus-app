import * as React from 'react';
import { Slot } from 'radix-ui';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '@/lib/utils';

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-md text-xs font-medium transition-[background-color,border-color,color,box-shadow,transform] duration-[120ms] outline-none focus-visible:outline-none focus-visible:ring-[1.5px] focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-bg active:translate-y-px disabled:pointer-events-none disabled:opacity-40 [&_svg]:shrink-0 cursor-pointer",
  {
    variants: {
      variant: {
        default: 'bg-accent-strong text-on-accent shadow-[inset_0_1px_0_rgb(255_255_255/0.18),0_1px_2px_rgb(0_0_0/0.4)] hover:bg-accent-hover active:bg-accent-strong',
        // Secondary: a soft tint, no outline (Claude-style). The transparent border keeps sizes equal to pick/reject.
        outline: 'border border-transparent bg-fill text-text hover:bg-fill-hover active:bg-press',
        ghost: 'text-text-2 hover:bg-hover hover:text-text active:bg-press',
        pick: 'border border-pick-line bg-pick-bg text-pick hover:brightness-125',
        reject: 'border border-reject-line bg-reject-bg text-reject hover:brightness-125',
      },
      size: {
        default: 'h-7 px-2.5',
        sm: 'h-6 rounded-[5px] px-2 text-[11px]',
        lg: 'h-8 px-3.5 text-[13px]',
        icon: 'size-7',
      },
    },
    defaultVariants: { variant: 'outline', size: 'default' },
  },
);

function Button({ className, variant, size, asChild = false, ...props }: React.ComponentProps<'button'> & VariantProps<typeof buttonVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : 'button';
  return <Comp data-slot="button" className={cn(buttonVariants({ variant, size, className }))} {...props} />;
}

export { Button, buttonVariants };
