import { useEffect, useState } from 'react';
import { Toaster as Sonner, type ToasterProps } from 'sonner';

// The bars docked at the bottom of each screen: the Cull footer, the Loupe's flag row, the Look's bar and the Compare and Stacks footers. Toasts sit just above whichever is
// highest, so an Undo toast never covers the key hints.
const BOTTOM_BARS = '.cull-foot, .loupe-bar, .look-bar, .view-foot';
const GAP = 12;

function useBottomOffset() {
  const [offset, setOffset] = useState(56);
  useEffect(() => {
    // A ResizeObserver on the bars themselves follows a bar that grows (the Loupe's row wraps);
    // the route change and resize re-find them. No observer on the whole DOM: the grid's virtual
    // rows mutate on every scroll and a layout read per frame would cost (docs/agents/performance.md).
    const ro = new ResizeObserver(() => measure());
    const timers: number[] = [];
    function measure() {
      let top = window.innerHeight;
      for (const el of document.querySelectorAll<HTMLElement>(BOTTOM_BARS)) {
        const r = el.getBoundingClientRect();
        if (r.height && r.bottom > window.innerHeight - 320) top = Math.min(top, r.top);
      }
      setOffset(Math.round(window.innerHeight - top + GAP));
    }
    function refind() {
      ro.disconnect();
      document.querySelectorAll<HTMLElement>(BOTTOM_BARS).forEach((el) => ro.observe(el));
      measure();
    }
    const onRoute = () => {
      timers.push(window.setTimeout(refind, 50), window.setTimeout(refind, 600));
    };
    onRoute();
    window.addEventListener('resize', measure);
    window.addEventListener('hashchange', onRoute);
    return () => {
      ro.disconnect();
      timers.forEach(clearTimeout);
      window.removeEventListener('resize', measure);
      window.removeEventListener('hashchange', onRoute);
    };
  }, []);
  return offset;
}

function Toaster(props: ToasterProps) {
  const offset = useBottomOffset();
  return (
    <Sonner
      theme="dark"
      position="bottom-center"
      offset={offset}
      gap={8}
      toastOptions={{ style: { background: 'var(--raised-2)', border: '1px solid var(--border-strong)', borderRadius: 10, boxShadow: 'var(--pop)', color: 'var(--text)', fontFamily: 'var(--sans)', fontSize: 13, padding: '10px 12px 10px 14px' } }}
      {...props}
    />
  );
}

export { Toaster };
