// Dropdown and context menus share one look, as in shadcn/ui.
import * as React from 'react';
import { ContextMenu as CM, DropdownMenu as DM } from 'radix-ui';
import { cn } from '@/lib/utils';

const content = 'anim-pop z-50 min-w-[220px] rounded-lg border border-border-strong bg-glass p-1 shadow-pop backdrop-blur-2xl backdrop-saturate-150';
const item = 'flex h-7 cursor-pointer select-none items-center gap-2 rounded-[5px] px-2.5 text-[13px] text-text-2 outline-none data-[highlighted]:bg-selected data-[highlighted]:text-text [&_svg]:text-text-3 data-[disabled]:pointer-events-none data-[disabled]:opacity-40';

const DropdownMenu = DM.Root;
const DropdownMenuTrigger = DM.Trigger;
function DropdownMenuContent({ className, sideOffset = 6, ...props }: React.ComponentProps<typeof DM.Content>) {
  return (
    <DM.Portal>
      <DM.Content sideOffset={sideOffset} className={cn(content, className)} {...props} />
    </DM.Portal>
  );
}
function DropdownMenuItem({ className, ...props }: React.ComponentProps<typeof DM.Item>) {
  return <DM.Item className={cn(item, className)} {...props} />;
}

const ContextMenu = CM.Root;
const ContextMenuTrigger = CM.Trigger;
function ContextMenuContent({ className, ...props }: React.ComponentProps<typeof CM.Content>) {
  return (
    <CM.Portal>
      <CM.Content className={cn(content, className)} {...props} />
    </CM.Portal>
  );
}
function ContextMenuItem({ className, ...props }: React.ComponentProps<typeof CM.Item>) {
  return <CM.Item className={cn(item, className)} {...props} />;
}
function ContextMenuSeparator({ className, ...props }: React.ComponentProps<typeof CM.Separator>) {
  return <CM.Separator className={cn('-mx-1 my-1 h-px bg-border', className)} {...props} />;
}

export { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem, ContextMenu, ContextMenuTrigger, ContextMenuContent, ContextMenuItem, ContextMenuSeparator };
