import * as React from 'react';
import { Slider as SliderPrimitive } from 'radix-ui';
import { cn } from '@/lib/utils';

/** `track` paints the track (Lightroom's Temp and Tint gradients); `bipolar` drops the fill, as Lightroom's zero-centred sliders have none. */
function Slider({ className, track, bipolar, ...props }: React.ComponentProps<typeof SliderPrimitive.Root> & { track?: string; bipolar?: boolean }) {
  return (
    <SliderPrimitive.Root className={cn('group relative flex h-[18px] w-full touch-none select-none items-center', className)} {...props}>
      <SliderPrimitive.Track className="relative h-[3px] grow overflow-hidden rounded-full bg-white/10" style={track ? { background: track } : undefined}>
        <SliderPrimitive.Range className={cn('absolute h-full bg-white/35 transition-colors group-hover:bg-white/50', (track || bipolar) && 'opacity-0')} />
      </SliderPrimitive.Track>
      <SliderPrimitive.Thumb aria-label={props['aria-label']} className="block size-3 rounded-full bg-text shadow-[0_0_0_1px_rgb(0_0_0/0.4),0_1px_3px_rgb(0_0_0/0.5)] outline-none transition-transform duration-[120ms] hover:scale-[1.2] active:scale-[1.25] focus-visible:ring-[1.5px] focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-bg" />
    </SliderPrimitive.Root>
  );
}

export { Slider };
