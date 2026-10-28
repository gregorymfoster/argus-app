import * as React from 'react';
import { Slider as SliderPrimitive } from 'radix-ui';
import { cn } from '@/lib/utils';

/** `track` paints the track (Lightroom's Temp and Tint gradients); `bipolar` drops the fill, as Lightroom's zero-centred sliders have none. */
function Slider({ className, track, bipolar, ...props }: React.ComponentProps<typeof SliderPrimitive.Root> & { track?: string; bipolar?: boolean }) {
  // While the person drags or keys it, the thumb follows at once; a value set from outside glides (styles.css).
  const [live, setLive] = React.useState(false);
  const idle = React.useRef(0);
  const touch = () => {
    setLive(true);
    clearTimeout(idle.current);
    idle.current = window.setTimeout(() => setLive(false), 400);
  };
  React.useEffect(() => () => clearTimeout(idle.current), []);
  return (
    <SliderPrimitive.Root
      className={cn('slider-root group relative flex h-[18px] w-full touch-none select-none items-center', className)}
      data-live={live || undefined}
      onPointerDownCapture={() => {
        setLive(true);
        clearTimeout(idle.current);
        window.addEventListener('pointerup', touch, { once: true });
      }}
      onKeyDownCapture={touch}
      {...props}
    >
      <SliderPrimitive.Track className="relative h-[3px] grow overflow-hidden rounded-full bg-white/10" style={track ? { background: track } : undefined}>
        <SliderPrimitive.Range className={cn('slider-range absolute h-full bg-white/35 transition-colors group-hover:bg-white/50', (track || bipolar) && 'opacity-0')} />
      </SliderPrimitive.Track>
      <SliderPrimitive.Thumb aria-label={props['aria-label']} className="slider-thumb block size-3 rounded-full bg-text shadow-[0_0_0_1px_rgb(0_0_0/0.4),0_1px_3px_rgb(0_0_0/0.5)] outline-none transition-transform duration-[120ms] hover:scale-[1.2] active:scale-[1.25] focus-visible:ring-[1.5px] focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-bg" />
    </SliderPrimitive.Root>
  );
}

export { Slider };
