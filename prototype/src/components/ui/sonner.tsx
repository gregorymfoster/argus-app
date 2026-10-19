import { Toaster as Sonner, type ToasterProps } from 'sonner';

function Toaster(props: ToasterProps) {
  return (
    <Sonner
      theme="dark"
      position="bottom-center"
      offset={56}
      gap={8}
      toastOptions={{ style: { background: 'var(--raised-2)', border: '1px solid var(--border-strong)', borderRadius: 10, boxShadow: 'var(--pop)', color: 'var(--text)', fontFamily: 'var(--sans)', fontSize: 13, padding: '10px 12px 10px 14px' } }}
      {...props}
    />
  );
}

export { Toaster };
