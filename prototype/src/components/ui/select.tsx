// A compact select, as in shadcn/ui, styled like the menus (menu.tsx).
import * as React from 'react';
import { Select as S } from 'radix-ui';
import { Check, ChevronDown } from 'lucide-react';
import { cn } from '@/lib/utils';

interface Props<T extends string> {
  value: T;
  onValueChange: (v: T) => void;
  items: { value: T; label: React.ReactNode }[];
  label: string;
  className?: string;
  disabled?: boolean;
}

export function Select<T extends string>({ value, onValueChange, items, label, className, disabled }: Props<T>) {
  return (
    <S.Root value={value} onValueChange={(v) => onValueChange(v as T)} disabled={disabled}>
      <S.Trigger
        aria-label={label}
        className={cn(
          'inline-flex h-[26px] min-w-0 items-center justify-between gap-2 rounded-[5px] border border-border-strong bg-raised px-2 text-[12px] text-text outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 [&_svg]:text-text-3',
          className,
        )}
      >
        <span className="truncate">
          <S.Value />
        </span>
        <S.Icon asChild>
          <ChevronDown size={12} />
        </S.Icon>
      </S.Trigger>
      <S.Portal>
        <S.Content position="popper" sideOffset={4} className="anim-pop z-50 min-w-[var(--radix-select-trigger-width)] rounded-lg border border-border-strong bg-glass p-1 shadow-pop backdrop-blur-2xl backdrop-saturate-150">
          <S.Viewport>
            {items.map((i) => (
              <S.Item key={i.value} value={i.value} className="relative flex h-7 cursor-pointer select-none items-center gap-2 rounded-[5px] pl-6 pr-2.5 text-[12.5px] text-text-2 outline-none data-[highlighted]:bg-selected data-[highlighted]:text-text">
                <S.ItemIndicator className="absolute left-1.5 inline-flex">
                  <Check size={12} />
                </S.ItemIndicator>
                <S.ItemText>{i.label}</S.ItemText>
              </S.Item>
            ))}
          </S.Viewport>
        </S.Content>
      </S.Portal>
    </S.Root>
  );
}
