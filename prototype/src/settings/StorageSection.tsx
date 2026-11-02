// Settings › Storage and backup: where every part of the Library lives and what losing its
// disk would cost, plus the Backup places that keep copies elsewhere (an external disk, the
// Google Drive or iCloud Drive folder). Mockup: the canvas's "Storage and backup" boards;
// engine: library::places, ADR 0017.

import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { ChevronLeft, Cloud, HardDrive, Folder, Server } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { pickFolder } from '../data/backend';
import { LIVE } from '../data/live';
import { refreshPolls, usePoll } from '../data/poll';
import { bytes, clearCache, relinkOriginals, storage, tidy, type Storage } from '../data/libraries';
import {
  COPIES,
  KIND_LABEL,
  WHEN,
  addBackupPlace,
  backupPlaces,
  estimateBackup,
  findBackupPlaces,
  removeBackupPlace,
  runBackup,
  updateBackupPlace,
  when as whenText,
  type Copies,
  type Estimate,
  type Found,
  type Place,
  type PlaceKind,
  type Places,
  type When,
} from '../data/backups';
import { Seg } from '../ui/kit';
import { ProgressBar, eta } from '../ui/Progress';

interface RunningJob {
  id: number;
  label: string;
  done: number;
  total: number;
  started_at: string | null;
  args: { place?: string };
}

function KindIcon({ kind }: { kind: PlaceKind }) {
  if (kind === 'google_drive' || kind === 'icloud' || kind === 'dropbox' || kind === 'one_drive') return <Cloud size={13} />;
  if (kind === 'network_disk') return <Server size={13} />;
  if (kind === 'external_disk') return <HardDrive size={13} />;
  return <Folder size={13} />;
}

const tone = (s: Place['status']) => (s === 'up to date' ? 'ok' : s === 'failed' || s === 'not connected' ? 'warn' : '');
const fail = (e: unknown) => toast(String(e).replace(/^Error: /, ''), { id: 'argus' });

/** Seconds left from a running job's MB done and when it started. */
function left(j: RunningJob): number | null {
  if (!j.started_at || !j.total || !j.done) return null;
  const ran = (Date.now() - new Date(j.started_at).getTime()) / 1000;
  return (ran / j.done) * (j.total - j.done);
}

export function StorageSection() {
  const [disk, setDisk] = useState<Storage | null>(null);
  const [demoPlaces, setPlaces] = useState<Places | null>(null);
  // Live: asked again every few seconds, so a backup running on the Queue shows as it goes.
  const polled = usePoll<Places>(LIVE ? 'backup_places' : null, {}, 3000);
  const places = LIVE ? (polled ?? null) : demoPlaces;
  const [open, setOpen] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const running = usePoll<RunningJob[]>(LIVE ? 'list_jobs' : null, { kind: 'backup', status: 'running' }, 1500) ?? [];

  const refresh = useCallback(async () => {
    try {
      const [s, p] = await Promise.all([storage(), backupPlaces()]);
      setDisk(s);
      setPlaces(p);
      refreshPolls('backup_places');
    } catch (e) {
      setError(String(e));
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  // A backup finishing changes the counts.
  const runningKey = running.map((j) => j.id).join(',');
  useEffect(() => {
    void refresh();
  }, [runningKey, refresh]);

  if (!disk || !places) return <p className="muted">{error ?? 'Reading the Library…'}</p>;

  const place = places.places.find((p) => p.id === open);
  if (adding) return <AddPlace onDone={() => (setAdding(false), void refresh())} />;
  if (place) return <PlaceDetail place={place} job={running.find((j) => j.args.place === place.id)} onBack={() => setOpen(null)} onChange={refresh} />;

  const denoise = disk.denoise_cache ?? { bytes: 0, files: 0 };
  const parts = [
    { key: 'originals', label: 'Originals in the Library', where: 'originals/', u: disk.originals_in_library, color: 'var(--text)', why: places.places.length ? 'Copied to your backups' : 'Only copy until backed up' },
    {
      key: 'elsewhere',
      label: 'Originals left in place',
      where: disk.elsewhere_folders?.length ? `${tidy(disk.elsewhere_folders[0])}${disk.elsewhere_folders.length > 1 ? `, +${disk.elsewhere_folders.length - 1}` : ''}` : 'none',
      u: disk.originals_elsewhere,
      color: 'var(--text-3)',
      why: disk.elsewhere_offline ? `Outside the Library · ${disk.elsewhere_offline} folders offline` : 'Outside the Library folder',
    },
    { key: 'db', label: 'Edits and Library database', where: 'library.db', u: disk.database, color: 'var(--done)', why: 'Every Verdict, Recipe and Look' },
    { key: 'snapshots', label: 'Database backups', where: 'backups/', u: disk.backups, color: 'var(--shoot)', why: 'Daily copies of the database, same disk' },
    { key: 'previews', label: 'Previews', where: 'previews/', u: disk.previews, color: 'var(--pick)', why: 'Rebuildable from the originals' },
    { key: 'denoise', label: 'AI Denoise cache', where: 'cache/denoise/', u: denoise, color: 'var(--accent)', why: `Rebuildable · up to ${Math.round((disk.denoise_cap ?? 0) / 2 ** 30)} GB` },
    { key: 'cache', label: 'Other cache', where: 'cache/', u: { bytes: Math.max(0, disk.cache.bytes - denoise.bytes), files: Math.max(0, disk.cache.files - denoise.files) }, color: 'var(--reject)', why: 'Rebuildable' },
  ];
  const whole = disk.total.bytes + (disk.free_bytes ?? 0) || 1;
  const connected = places.places.filter((p) => p.available).length;

  const relink = () =>
    LIVE
      ? void pickFolder('Where are the missing originals?').then(async (dir) => {
          if (!dir) return;
          const r = await relinkOriginals(dir);
          toast(r.relinked ? `Found ${r.relinked} originals` : 'No missing originals in that folder');
          await refresh();
        }, fail)
      : toast('The prototype has one sample Library');

  return (
    <div className="lib-settings store" data-testid="storage-settings">
      <div className="store-stats">
        <div className="store-stat grow">
          <span className="faint">Library</span>
          <span className="mono" title={disk.library}>{tidy(disk.library)}</span>
          <span className="faint" data-testid="storage-free">{bytes(disk.free_bytes)} free on this disk</span>
        </div>
        <div className="store-stat">
          <span className="faint">In the Library folder</span>
          <span className="store-big">{bytes(disk.total.bytes)}</span>
        </div>
        <div className="store-stat">
          <span className="faint">Can be cleared</span>
          <span className="store-big muted">{bytes(disk.cache.bytes)}</span>
        </div>
        <div className="store-stat">
          <span className="faint">Photos with 2+ backups</span>
          <span className={`store-big ${places.photos && places.two_copies >= places.photos ? 'ok' : ''}`} data-testid="two-copies">
            {places.two_copies.toLocaleString()} <span className="faint">of {places.photos.toLocaleString()}</span>
          </span>
        </div>
      </div>
      <div className="lib-bar" role="img" aria-label="Disk use">
        {parts.map((p) => (
          <span key={p.key} style={{ width: `${(p.u.bytes / whole) * 100}%`, background: p.color }} />
        ))}
      </div>
      <div className="store-table" role="table" aria-label="Where the Library's parts live">
        <div className="store-head" role="row">
          <span>Part</span>
          <span>Where</span>
          <span>Size</span>
          <span>Files</span>
          <span>If this disk dies</span>
        </div>
        {parts.map((p) => (
          <div key={p.key} role="row" data-testid={`part-${p.key}`}>
            <span>
              <i className="lib-swatch" style={{ background: p.color }} />
              {p.label}
            </span>
            <span className="mono muted" title={p.where}>
              {p.where}
            </span>
            <span className="mono">{bytes(p.u.bytes)}</span>
            <span className="mono faint">{p.u.files.toLocaleString()}</span>
            <span className="faint" title={p.why}>
              {p.why}
            </span>
          </div>
        ))}
      </div>
      <div className="lib-row">
        <Button
          size="sm"
          variant="outline"
          data-testid="clear-cache"
          disabled={disk.cache.bytes === 0}
          onClick={() => void clearCache().then((f) => (toast(`Freed ${bytes(f.bytes)}`), refresh()), fail)}
        >
          Clear rebuildable cache · {bytes(disk.cache.bytes)}
        </Button>
        {disk.missing > 0 && (
          <Button size="sm" variant="ghost" onClick={relink}>
            Find {disk.missing} missing originals…
          </Button>
        )}
        <span className="grow" />
        <span className="faint lib-note">Clearing never touches originals, edits or backups.</span>
      </div>

      <h3 className="store-h">
        Backups
        <span className="faint">
          {places.places.length ? `${places.places.length} places · ${connected} connected` : 'none yet: originals copied into the Library have no other copy'}
        </span>
        <span className="grow" />
        {places.places.length > 0 && (
          <Button size="sm" variant="ghost" disabled={!connected} onClick={() => void runBackup({ why: 'By hand' }).then(() => (toast('Backing up on the Queue'), refresh()), fail)}>
            Back up all now
          </Button>
        )}
        <Button size="sm" variant="outline" onClick={() => setAdding(true)} data-testid="add-backup">
          Add backup…
        </Button>
      </h3>
      <div className="store-places">
        {places.places.map((p) => {
          const job = running.find((j) => j.args.place === p.id);
          return (
            <button key={p.id} className="store-place" onClick={() => setOpen(p.id)} data-testid="backup-place">
              <span className="store-place-top">
                <KindIcon kind={p.kind} />
                <span className="store-name" title={p.name}>{p.name}</span>
                <span className={`store-tag ${tone(p.status)}`}>{p.status}</span>
              </span>
              <span className="mono faint store-path" title={p.copy_dir}>
                {tidy(p.copy_dir)}
              </span>
              <span className="faint">
                {COPIES.find((c) => c.value === p.copies)?.label} · {WHEN.find((w) => w.value === p.when)?.label.toLowerCase()}
              </span>
              {job ? (
                <ProgressBar label={`${bytes(job.done * 1048576)} of ${bytes(job.total * 1048576)}`} done={job.done} total={job.total} eta={left(job)} count={false} />
              ) : (
                <span className="faint">
                  {p.waiting > 0 ? <span className="accent-text">{p.waiting.toLocaleString()} Photos waiting</span> : `${p.photos_in.toLocaleString()} Photos`} · last {whenText(p.last_ok)}
                </span>
              )}
            </button>
          );
        })}
      </div>
      {error && <p className="lib-error">{error}</p>}
    </div>
  );
}

function PlaceDetail({ place: p, job, onBack, onChange }: { place: Place; job?: RunningJob; onBack: () => void; onChange: () => Promise<void> }) {
  const set = (change: Parameters<typeof updateBackupPlace>[1]) => void updateBackupPlace(p.id, change).then(onChange, fail);
  return (
    <div className="lib-settings store" data-testid="backup-detail">
      <div className="lib-row">
        <Button size="sm" variant="ghost" onClick={onBack} aria-label="Back to Storage and backup">
          <ChevronLeft size={13} /> Storage and backup
        </Button>
        <span className="store-name" title={p.name}>{p.name}</span>
        <span className={`store-tag ${tone(p.status)}`}>{p.status}</span>
        <span className="grow" />
        <Button size="sm" variant="ghost" disabled={!p.available || !!job} onClick={() => void runBackup({ id: p.id, verify: true, why: 'Check by hand' }).then(onChange, fail)}>
          Check now
        </Button>
        <Button size="sm" variant="outline" disabled={!p.available || !!job} onClick={() => void runBackup({ id: p.id, why: 'By hand' }).then(onChange, fail)} data-testid="backup-now">
          Back up now
        </Button>
      </div>
      <div className="store-stats">
        <div className="store-stat grow">
          <span className="faint">Copy</span>
          <span className="mono" title={p.copy_dir}>{tidy(p.copy_dir)}</span>
          <span className="faint">
            {KIND_LABEL[p.kind]}
            {p.available ? ` · ${bytes(p.free_bytes)} free` : ' · not connected'}
          </span>
        </div>
        <div className="store-stat">
          <span className="faint">Backup size</span>
          <span className="store-big">{bytes(p.bytes)}</span>
        </div>
        <div className="store-stat">
          <span className="faint">Photos in it</span>
          <span className="store-big">{p.photos_in.toLocaleString()}</span>
        </div>
        <div className="store-stat">
          <span className="faint">Waiting</span>
          <span className={`store-big ${p.waiting ? 'accent-text' : 'ok'}`}>{p.waiting.toLocaleString()}</span>
        </div>
      </div>
      {job && <ProgressBar label={`${job.label} · ${bytes(job.done * 1048576)} of ${bytes(job.total * 1048576)}`} done={job.done} total={job.total} eta={left(job)} count={false} />}
      {p.last_error && <div className="lib-row lib-warn">{p.last_error}</div>}
      <div className="lib-row">
        <span>Copies</span>
        <span className="grow" />
        <Seg label="What it copies" value={p.copies} onChange={(v: Copies) => set({ copies: v })} items={COPIES.map((c) => ({ value: c.value, label: c.label, title: c.note }))} />
      </div>
      <div className="lib-row">
        <span>Runs</span>
        <span className="grow" />
        <Seg label="When it runs" value={p.when} onChange={(v: When) => set({ when: v })} items={WHEN} />
      </div>
      <label className="lib-row">
        <Checkbox checked={p.include_elsewhere} onCheckedChange={(c) => set({ include_elsewhere: c === true })} />
        <span>Include originals left in place</span>
        <span className="faint">copied to elsewhere/ in the backup</span>
      </label>
      <label className="lib-row">
        <Checkbox checked={p.verify_monthly} onCheckedChange={(c) => set({ verify_monthly: c === true })} />
        <span>Re-read every file once a month</span>
        <span className="faint">{p.last_verified ? `last ${whenText(p.last_verified)}` : 'catches a failing disk before you need it'}</span>
      </label>
      <p className="faint lib-note">
        A backup never deletes: Photos you remove stay in it. It has the Library's own layout, so on any Mac Library › Open… on the copy opens it. Keeps {p.keep_snapshots} daily database copies.
      </p>
      <h3 className="store-h">History</h3>
      <div className="store-table store-history" role="table" aria-label="Backup history">
        <div className="store-head" role="row">
          <span>When</span>
          <span>Why</span>
          <span>Copied</span>
          <span>Took</span>
          <span>Check</span>
        </div>
        {p.history.length === 0 && (
          <div role="row">
            <span className="faint">Not run yet</span>
          </div>
        )}
        {p.history.map((r) => (
          <div key={r.at + r.why} role="row">
            <span className="mono muted">{whenText(r.at)}</span>
            <span title={r.why}>{r.why}</span>
            <span className="mono">{r.bytes ? `+${r.files.toLocaleString()} · ${bytes(r.bytes)}` : `${r.files.toLocaleString()} files`}</span>
            <span className="mono faint">{r.secs < 5 ? '<5 s' : (eta(r.secs) ?? '–').replace('~', '')}</span>
            <span>
              <span className={`store-tag ${r.ok ? 'ok' : 'warn'}`} title={r.check}>
                {r.check}
              </span>
            </span>
          </div>
        ))}
      </div>
      <div className="lib-row">
        <span className="grow" />
        <Button size="sm" variant="ghost" onClick={() => void removeBackupPlace(p.id).then(() => (toast(`Stopped backing up to ${p.name}. Its copy stays where it is.`), onBack(), onChange()), fail)}>
          Stop backing up here
        </Button>
      </div>
    </div>
  );
}

function AddPlace({ onDone }: { onDone: () => void }) {
  const [found, setFound] = useState<Found[] | null>(null);
  const [folder, setFolder] = useState<string | null>(null);
  const [copies, setCopies] = useState<Copies>('originals');
  const [when, setWhen] = useState<When>('after_import');
  const [est, setEst] = useState<Estimate | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void findBackupPlaces().then((f) => {
      setFound(f);
      const first = f.find((x) => !x.in_use);
      if (first) setFolder(first.folder);
    }, fail);
  }, []);
  useEffect(() => {
    setEst(null);
    if (folder) void estimateBackup(folder, copies, false).then(setEst, () => setEst(null));
  }, [folder, copies]);
  const chosen = found?.find((f) => f.folder === folder);
  const other = () => void pickFolder('Back up the Library to…').then((d) => d && setFolder(d), fail);
  const cloud = chosen && chosen.kind !== 'external_disk' && chosen.kind !== 'network_disk' && chosen.kind !== 'folder';
  const add = () => {
    if (!folder) return;
    setBusy(true);
    void addBackupPlace({ folder, name: chosen?.name.replace(/ \(.*\)$/, ''), copies, when, include_elsewhere: false, run: true }).then(
      (r) => (toast(`Backing up to ${r.place.name} on the Queue`), onDone()),
      (e) => (setBusy(false), fail(e)),
    );
  };
  return (
    <div className="lib-settings store" data-testid="add-backup-place">
      <div className="lib-row">
        <Button size="sm" variant="ghost" onClick={onDone} aria-label="Back to Storage and backup">
          <ChevronLeft size={13} /> Storage and backup
        </Button>
        <span className="store-name">Add a backup</span>
      </div>
      <div className="store-add">
        <div className="store-found" role="radiogroup" aria-label="Where">
          <span className="faint">Found on this Mac</span>
          {found === null && <span className="faint">Looking…</span>}
          {found?.map((f) => (
            <button key={f.folder} role="radio" aria-checked={folder === f.folder} disabled={f.in_use} className={`store-opt ${folder === f.folder ? 'on' : ''}`} onClick={() => setFolder(f.folder)}>
              <KindIcon kind={f.kind} />
              <span className="store-opt-text">
                <span title={f.folder}>
                  {f.name}
                  {f.in_use && <span className="store-tag">in use</span>}
                </span>
                <span className="faint" title={f.note ?? undefined}>
                  {KIND_LABEL[f.kind]} · {bytes(f.free_bytes)} free
                </span>
              </span>
            </button>
          ))}
          <button className={`store-opt ${folder && !chosen ? 'on' : ''}`} onClick={other}>
            <Folder size={13} />
            <span className="store-opt-text">
              <span title={folder ?? undefined}>{folder && !chosen ? tidy(folder) : 'Another folder…'}</span>
              <span className="faint">A NAS or any other folder</span>
            </span>
          </button>
        </div>
        <div className="store-add-body">
          {cloud && <p className="lib-note muted">Argus writes into the {KIND_LABEL[chosen.kind]} app's folder and the app uploads it. No sign-in or keys in Argus.</p>}
          {chosen?.note && <p className="lib-note faint">{chosen.note}</p>}
          <div className="lib-row">
            <span>Copies</span>
            <span className="grow" />
            <Seg label="What it copies" value={copies} onChange={setCopies} items={COPIES.map((c) => ({ value: c.value, label: c.label, title: c.note }))} />
          </div>
          <p className="faint lib-note">{COPIES.find((c) => c.value === copies)?.note}</p>
          <div className="lib-row">
            <span>Runs</span>
            <span className="grow" />
            <Seg label="When it runs" value={when} onChange={setWhen} items={WHEN} />
          </div>
          {est && (
            <div className="store-stat" data-testid="backup-estimate">
              <span className="faint">First backup</span>
              <span className="store-big">
                {bytes(est.bytes)} <span className="faint">{est.files.toLocaleString()} files</span>
              </span>
              <span className={est.fits ? 'faint' : 'lib-error'}>
                {est.fits ? `About ${eta(est.secs) ?? 'a few seconds'} to write${cloud ? ', then the app uploads it at its own pace' : ''}. Runs on the Queue and carries on after a quit.` : `Needs ${bytes(est.bytes)}; only ${bytes(est.free_bytes)} free there.`}
              </span>
            </div>
          )}
          <span className="grow" />
          <div className="lib-row">
            <span className="grow" />
            <Button size="sm" variant="ghost" onClick={onDone}>
              Cancel
            </Button>
            <Button size="sm" disabled={!folder || busy || (est != null && !est.fits)} onClick={add} data-testid="add-backup-go">
              Add and back up
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
