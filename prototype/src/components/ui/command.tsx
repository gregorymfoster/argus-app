import * as React from 'react';
import { SearchIcon } from 'lucide-react';
import { Command as CommandPrimitive } from 'cmdk';
import { Dialog as DialogPrimitive } from 'radix-ui';
import { cn } from '@/lib/utils';

function CommandDialog({ open, onOpenChange, children, label }: { open: boolean; onOpenChange: (o: boolean) => void; children: React.ReactNode; label: string }) {
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="anim-fade fixed inset-0 z-50 bg-black/45" />
        <DialogPrimitive.Content aria-describedby={undefined} className="anim-pop fixed left-1/2 top-[12vh] z-50 w-[min(620px,calc(100%-32px))] -translate-x-1/2 overflow-hidden rounded-[10px] border border-border-strong bg-glass shadow-pop backdrop-blur-2xl backdrop-saturate-150">
          <DialogPrimitive.Title className="sr-only">{label}</DialogPrimitive.Title>
          <CommandPrimitive label={label}>{children}</CommandPrimitive>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

function CommandInput({ className, ...props }: React.ComponentProps<typeof CommandPrimitive.Input>) {
  return (
    <div className="flex items-center gap-2.5 border-b border-border px-4">
      <SearchIcon size={15} className="shrink-0 text-text-3" />
      <CommandPrimitive.Input className={cn('h-11 w-full bg-transparent text-[14px] outline-none placeholder:text-text-3', className)} {...props} />
    </div>
  );
}

function CommandList({ className, ...props }: React.ComponentProps<typeof CommandPrimitive.List>) {
  return <CommandPrimitive.List className={cn('max-h-[min(52vh,420px)] scroll-py-1.5 overflow-auto p-1.5', className)} {...props} />;
}

function CommandEmpty(props: React.ComponentProps<typeof CommandPrimitive.Empty>) {
  return <CommandPrimitive.Empty className="px-3 py-8 text-center text-text-3" {...props} />;
}

function CommandGroup({ className, ...props }: React.ComponentProps<typeof CommandPrimitive.Group>) {
  return <CommandPrimitive.Group className={cn('[&:not(:first-child)]:mt-1 [&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:pt-2 [&_[cmdk-group-heading]]:text-[11.5px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-text-3', className)} {...props} />;
}

function CommandItem({ className, ...props }: React.ComponentProps<typeof CommandPrimitive.Item>) {
  return <CommandPrimitive.Item className={cn('flex h-8 cursor-pointer items-center gap-2.5 rounded-[5px] px-2.5 text-[13px] text-text-2 data-[selected=true]:bg-selected data-[selected=true]:text-text [&_svg]:text-text-3 data-[selected=true]:[&_svg]:text-text-2', className)} {...props} />;
}

/** Linear-style hint bar under the list: how to drive the palette from the keyboard. */
function CommandFooter({ children }: { children?: React.ReactNode }) {
  return (
    <div className="flex items-center gap-4 border-t border-border px-4 py-2 text-[11.5px] text-text-3">
      <span className="flex items-center gap-1.5"><kbd>↑</kbd><kbd>↓</kbd> Navigate</span>
      <span className="flex items-center gap-1.5"><kbd>↵</kbd> Open</span>
      <span className="flex items-center gap-1.5"><kbd>Esc</kbd> Close</span>
      {children && <span className="ml-auto">{children}</span>}
    </div>
  );
}

export { CommandDialog, CommandInput, CommandList, CommandEmpty, CommandGroup, CommandItem, CommandFooter };
