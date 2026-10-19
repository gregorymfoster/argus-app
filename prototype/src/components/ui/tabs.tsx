import * as React from 'react';
import { Tabs as TabsPrimitive } from 'radix-ui';
import { cn } from '@/lib/utils';

const Tabs = TabsPrimitive.Root;

function TabsList({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.List>) {
  return <TabsPrimitive.List className={cn('flex flex-none gap-0.5 border-b border-border px-3 pt-2', className)} {...props} />;
}

function TabsTrigger({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Trigger>) {
  return (
    <TabsPrimitive.Trigger
      className={cn('-mb-px cursor-pointer border-b-[1.5px] border-transparent px-2 pb-1.5 pt-1 text-[12.5px] font-medium text-text-3 transition-colors duration-[120ms] hover:text-text-2 data-[state=active]:border-text data-[state=active]:text-text', className)}
      {...props}
    />
  );
}

function TabsContent({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Content>) {
  return <TabsPrimitive.Content className={cn('min-h-0 flex-1 overflow-auto outline-none', className)} {...props} />;
}

export { Tabs, TabsList, TabsTrigger, TabsContent };
