import * as React from "react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { XIcon } from "lucide-react";
import { cn } from "@/lib/utils";

const Dialog = DialogPrimitive.Root;
const DialogTrigger = DialogPrimitive.Trigger;
const DialogClose = DialogPrimitive.Close;

function DialogContent({
  className,
  children,
  closable = true,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Content> & {
  closable?: boolean;
}) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="anim-fade fixed inset-0 z-50 bg-black/50" />
      <DialogPrimitive.Content
        aria-describedby={undefined}
        className={cn(
          "anim-pop fixed left-1/2 top-[14vh] z-50 flex w-[min(520px,calc(100%-32px))] -translate-x-1/2 flex-col gap-4 rounded-[10px] border border-border-strong bg-panel p-5 shadow-pop outline-none",
          className,
        )}
        {...props}
      >
        {children}
        {closable && (
          <DialogPrimitive.Close
            className="absolute right-3.5 top-3.5 grid size-7 place-items-center rounded-md text-text-3 transition-colors hover:bg-hover hover:text-text"
            aria-label="Close"
          >
            <XIcon size={14} />
          </DialogPrimitive.Close>
        )}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}

function DialogTitle({
  className,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Title>) {
  return (
    <DialogPrimitive.Title
      className={cn("m-0 text-[15px] font-medium tracking-[-0.015em]", className)}
      {...props}
    />
  );
}

function DialogDescription({
  className,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Description>) {
  return (
    <DialogPrimitive.Description
      className={cn("m-0 text-text-2", className)}
      {...props}
    />
  );
}

export {
  Dialog,
  DialogTrigger,
  DialogClose,
  DialogContent,
  DialogTitle,
  DialogDescription,
};
