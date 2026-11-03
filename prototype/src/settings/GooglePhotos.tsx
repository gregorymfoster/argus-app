// Settings › Export › Google Photos: the person's own Google client, then Sign in with Google.
// Argus ships no Google key (docs/export.md), so the first time takes three steps in Google
// Cloud; after that it is one click, and Disconnect forgets the sign-in on this Mac.

import { useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { call } from '../data/backend';
import { LIVE } from '../data/live';
import { connect, disconnect, saveClient, useGooglePhotos } from '../data/googlePhotos';

const clock = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;

export function GoogleAccount() {
  const g = useGooglePhotos();
  const [editing, setEditing] = useState(false);
  const [id, setId] = useState('');
  const [secret, setSecret] = useState('');
  if (!g) return <div className="set-note gp-account">Checking Google Photos…</div>;

  if (!g.client || editing)
    return (
      <div className="gp-account" data-testid="gphotos-setup">
        <div className="set-note">Connect Google Photos once, with your own free Google Cloud client (Argus ships no Google key):</div>
        <ol className="gp-steps set-note">
          <li>
            In Google Cloud, make a project and turn on the <b>Photos Library API</b>.
          </li>
          <li>
            Under OAuth consent screen, choose External, add yourself as a test user, then <b>Publish app</b> so the sign-in lasts.
          </li>
          <li>
            Under Credentials, create an <b>OAuth client ID</b> of type Desktop app and paste it here.
          </li>
        </ol>
        <form
          className="gp-form"
          onSubmit={(e) => {
            e.preventDefault();
            void saveClient(id.trim(), secret.trim()).then(
              () => {
                setEditing(false);
                setId('');
                setSecret('');
              },
              (err) => toast.error(String((err as Error).message ?? err).replace(/^Error: /, '')),
            );
          }}
        >
          <input className="set-input mono" placeholder="Client ID · …apps.googleusercontent.com" value={id} onChange={(e) => setId(e.target.value)} autoComplete="off" spellCheck={false} aria-label="Client ID" data-testid="gphotos-client-id" />
          <input type="password" className="set-input mono" placeholder="Client secret" value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete="off" spellCheck={false} aria-label="Client secret" />
          <div className="row gp-actions">
            <Button size="sm" variant="ghost" type="button" onClick={() => LIVE && void call('google_photos_console')}>
              Open Google Cloud
            </Button>
            <span className="grow" />
            {editing && (
              <Button size="sm" variant="ghost" type="button" onClick={() => setEditing(false)}>
                Cancel
              </Button>
            )}
            <Button size="sm" type="submit" disabled={!id.trim()}>
              Save
            </Button>
          </div>
        </form>
      </div>
    );

  return (
    <div className="gp-account row" data-testid="gphotos-account">
      {g.connected ? (
        <>
          <span className="gp-dot on" />
          <span className="grow">Connected. Exports go into an album per Shoot.</span>
          <Button size="sm" variant="ghost" onClick={() => void disconnect()}>
            Disconnect
          </Button>
        </>
      ) : g.waiting ? (
        <>
          <span className="gp-dot wait" />
          <span className="grow trunc">Finish signing in with Google in your browser · {clock(g.waited_s ?? 0)}</span>
          <Button size="sm" variant="ghost" onClick={() => void connect()}>
            Open again
          </Button>
        </>
      ) : (
        <>
          <span className="gp-dot" />
          <span className="grow trunc" title={g.error ?? undefined}>
            {g.error ?? 'Not signed in'}
          </span>
          <Button size="sm" variant="ghost" onClick={() => setEditing(true)}>
            Change client
          </Button>
          <Button size="sm" onClick={() => void connect().catch((err) => toast.error(String((err as Error).message ?? err)))} data-testid="gphotos-connect">
            Sign in with Google
          </Button>
        </>
      )}
    </div>
  );
}
