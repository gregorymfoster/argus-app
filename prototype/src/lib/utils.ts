import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Say that a background save failed, in a toast, instead of swallowing it. */
export function toastFailure(what: string) {
  return (e: unknown) => {
    void import('sonner').then(({ toast }) => toast.error(what, { description: String(e).replace(/^Error: /, '') }));
  };
}
