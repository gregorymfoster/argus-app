import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { toast } from 'sonner';
import { PHOTOS, SHOOT } from './data/dummy';
import { lastRemovalAt, takeRemovedToast, undoRemoval } from './data/remove';
import { RemoveDialog, askRemove, showRemoved } from './ui/Remove';
import { PasteSettingsDialog } from './ui/PasteSettings';
import { CreatePresetDialog } from './ui/CreatePreset';
import { LIVE, inCollection, exportAfterLoad, noteImportFinished, refreshLivePhotos, runningImport, takeCullAfterLoad } from './data/live';
import { zoom } from './ui/zoom';
import { initialCull } from './agent/agent';
import { liveCullChat, useAgentRunner } from './agent/AgentPanel';
import { recipeOf, useLibrary } from './store/library';
import { runAuto } from './data/auto';
import { usePref, type Screen } from './store/variants';
import { go, useRoute } from './router';
import { refreshEditStamps } from './data/editedThumbs';
import { Sidebar } from './ui/Sidebar';
import { Palette, VariantsPanel, type Command } from './ui/Overlays';
import { ExportDialog } from './ui/Export';
import { TipProvider } from './ui/kit';
import { Toaster } from '@/components/ui/sonner';
import { ImportDialog } from './screens/Import';
import { CullScreen } from './screens/Cull';
import { LoupeScreen } from './screens/Loupe';
import { CompareScreen } from './screens/Compare';
import { PeopleScreen } from './screens/People';
import { BurstsScreen } from './screens/Bursts';
import { LookScreen } from './screens/Look';
import { EditScreen } from './screens/Edit';
import { SettingsDialog, openSettings } from './settings/Settings';
import { useUpdateToasts } from './ui/Updates';
import { checkForUpdates } from './data/updates';

export function App() {
  const route = useRoute();
  const { state, dispatch } = useLibrary();
  const [palette, setPalette] = useState(false);
  const [exporting, setExporting] = useState(false);
  // A selection to export instead of the Picks (Lightroom exports what's selected).
  const [exportIds, setExportIds] = useState<string[] | undefined>();
  // Greg: click Import and walk away; the cull runs after it, at the Compute setting.
  const [cullPref, setCullPref] = usePref<'1' | '0'>('import-cull', '1');
  const autoCull = cullPref === '1';
  const setAutoCull = (v: boolean) => setCullPref(v ? '1' : '0');
  const runCull = useAgentRunner('cull');
  // Check for Updates… by hand says what it found; the background check stays quiet.
  const askedUpdate = useRef(false);
  useUpdateToasts(askedUpdate);

  // Kick off the AI cull once previews are ready, like the real app would.
  useEffect(() => {
    if (LIVE) return;
    const ready = state.imported === PHOTOS.length && !state.importing;
    if (ready && autoCull && !state.culling && Object.keys(state.aiVerdicts).length === 0) {
      dispatch({ type: 'cull/start' });
      runCull(initialCull(state));
    }
  }, [state, autoCull, dispatch, runCull]);

  // Live: an Import opened its Shoot early; Photos pop into the grid as they land.
  useEffect(() => {
    if (!LIVE) return;
    let live = true;
    let t: ReturnType<typeof setTimeout>;
    const follow = async (first: boolean) => {
      const task = await runningImport();
      if (!live) return;
      // An Import that ended while the page loaded may have landed more Photos since.
      const had = PHOTOS.length;
      const n = await refreshLivePhotos().catch(() => had);
      if (!live || (first && !task && n === had)) return;
      // While Auto tones the new Photos, the bar counts those instead.
      const auto = !!task && !task.label.startsWith('Importing');
      dispatch({ type: 'import/live', n, done: auto ? task.done : n, total: auto ? task.total : Math.max(n, task?.total ?? n), eta: task?.eta_s ?? null, importing: !!task, label: task?.label });
      if (task) t = setTimeout(() => follow(false), 700);
      else noteImportFinished();
    };
    void follow(true);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [dispatch]);

  // Live: a real import asked to cull the Shoot it just opened.
  useEffect(() => {
    if (LIVE && takeCullAfterLoad()) void liveCullChat('Cull this shoot', dispatch);
  }, [dispatch]);

  // Live: removing a Shoot reloaded the page; say so, with Undo.
  useEffect(() => {
    const r = LIVE ? takeRemovedToast() : null;
    if (r) showRemoved(r, () => void undoRemoval());
  }, []);

  // Live: Export… from another Shoot's menu reloaded into that Shoot.
  useEffect(() => {
    if (LIVE && exportAfterLoad()) setExporting(true);
  }, []);

  // The store raises toasts; Sonner shows them, with Undo for batch changes. At most one
  // per frame, the newest: Sonner commits each toast under flushSync, and a toast per key
  // while ⌘Z is held down, with the Library's answers waiting to render, piled up past
  // React's nested-update limit ("Maximum update depth exceeded", fuzz 2026-10-22).
  const toastFrame = useRef<{ raf: number; t: typeof state.toast }>({ raf: 0, t: null });
  useEffect(() => {
    const f = toastFrame.current;
    f.t = state.toast;
    if (!f.t || f.raf) return;
    f.raf = requestAnimationFrame(() => {
      f.raf = 0;
      const t = f.t;
      if (!t) return;
      const action = t.undo ? { label: 'Undo', onClick: () => dispatch({ type: 'undo' }) } : t.redo ? { label: 'Redo', onClick: () => dispatch({ type: 'redo' }) } : undefined;
      toast(t.text, { id: 'argus', action });
    });
  }, [state.toast, dispatch]);
  useEffect(() => {
    const f = toastFrame.current;
    return () => {
      cancelAnimationFrame(f.raf);
      f.raf = 0;
    };
  }, []);

  // Live: once a change reaches the Library (edits, Auto, undo), thumbnails show it.
  const lastBatch = state.past[state.past.length - 1]?.batch;
  useEffect(() => {
    if (!LIVE) return;
    // Twice: an Undo's engine call isn't awaited by the store.
    const ts = [400, 1500].map((ms) => setTimeout(() => void Promise.resolve(lastBatch).catch(() => {}).then(refreshEditStamps), ms));
    return () => ts.forEach(clearTimeout);
  }, [state.past.length, state.future.length, lastBatch, route]);

  useHotkeys('mod+k', () => setPalette((p) => !p), { preventDefault: true, enableOnFormTags: true });
  // ⌘Z undoes a Remove from Library when that was the last thing done.
  useHotkeys(
    'mod+z',
    () => {
      if (lastRemovalAt() > (state.past[state.past.length - 1]?.at ?? 0)) void undoRemoval().then((n) => n != null && dispatch({ type: 'photos/changed', imported: n }));
      else dispatch({ type: 'undo' });
    },
    { preventDefault: true },
    [state.past],
  );
  useHotkeys('mod+shift+z', () => dispatch({ type: 'redo' }), { preventDefault: true });

  // Lightroom's Auto on every Photo of the open Shoot, as one undoable batch.
  const autoShoot = () =>
    runAuto(
      PHOTOS.slice(0, state.imported).map((p) => p.id),
      (id) => recipeOf(state, id),
      dispatch,
    );
  // The AI cull of the whole open Shoot: Verdicts, stars, Stacks and Best Photos.
  const cullShoot = () => {
    if (inCollection()) return void toast('Open a Shoot to cull it', { id: 'argus', description: 'The AI cull compares the Photos of one Shoot.' });
    if (LIVE) return void liveCullChat('Cull this shoot', dispatch);
    if (state.culling) return;
    dispatch({ type: 'cull/start' });
    runCull(initialCull(state));
  };

  const commands: Command[] = useMemo(() => [
      { label: 'Export Picks…', hint: '⌘E', run: () => setExporting(true) },
      { label: 'Settings…', hint: '⌘,', run: () => openSettings() },
      { label: 'Keyboard shortcuts', hint: '⌘/', run: () => openSettings('keys') },
      {
        label: 'Check for Updates…',
        run: () => {
          askedUpdate.current = true;
          void checkForUpdates();
        },
      },
      { label: 'Zoom In', hint: '⌘+', run: () => void zoom('in') },
      { label: 'Zoom Out', hint: '⌘−', run: () => void zoom('out') },
      { label: 'Actual Size', hint: '⌘0', run: () => void zoom('reset') },
      {
        label: 'Auto every Photo in the Shoot',
        hint: '⇧A',
        run: autoShoot,
      },
      ...(SHOOT.id !== 'none' && !inCollection() ? [{ label: `Remove ${SHOOT.name} from Library…`, run: () => askRemove({ shoot: { id: SHOOT.id, name: SHOOT.name, count: state.imported } }) }] : []),
    ], [state, dispatch]);
  useHotkeys(['mod+e', 'mod+shift+e'], () => setExporting(true), { preventDefault: true });
  useHotkeys('mod+shift+i', () => go({ name: 'import' }), { preventDefault: true });
  // In the app the menu's Settings… item owns ⌘,; in a browser this does.
  useHotkeys('mod+comma', () => openSettings(), { preventDefault: true, enableOnFormTags: true });
  // ⌘/ lists every shortcut, as Lightroom's Help › Keyboard Shortcuts does; ? as in Linear and Gmail.
  useHotkeys(['mod+slash', 'shift+slash'], () => openSettings('keys'), { preventDefault: true });
  const screen: Screen = route.name;
  const sidebar = route.name === 'import' || route.name === 'cull' || route.name === 'people';

  let body;
  switch (route.name) {
    case 'import':
    case 'cull':
      body = (
        <CullScreen
          onCull={cullShoot}
          onExport={(ids) => {
            setExportIds(ids);
            setExporting(true);
          }}
        />
      );
      break;
    case 'loupe':
      body = <LoupeScreen id={route.id} />;
      break;
    case 'compare':
      body = <CompareScreen ids={route.ids} />;
      break;
    case 'bursts':
      body = <BurstsScreen start={route.id} />;
      break;
    case 'look':
      body = <LookScreen onExport={() => setExporting(true)} />;
      break;
    case 'edit':
      body = <EditScreen key={route.id} id={route.id} />;
      break;
    case 'people':
      body = <PeopleScreen key={route.id ?? ''} id={route.id} />;
      break;
  }

  return (
    <TipProvider>
      <div className="app" style={{ ['--left' as string]: sidebar ? '232px' : '0px' } as CSSProperties}>
        {sidebar && <Sidebar route={route} onPalette={() => setPalette(true)} onExport={() => setExporting(true)} onAuto={autoShoot} onCull={cullShoot} />}
        <div className={`workspace ${sidebar ? '' : 'full'}`}>
          {body}
        </div>
        <ImportDialog open={route.name === 'import'} autoCull={autoCull} setAutoCull={setAutoCull} />
        <SettingsDialog />
        <RemoveDialog />
        <PasteSettingsDialog />
        <CreatePresetDialog onSaved={(text) => dispatch({ type: 'toast', text })} />
        <Palette open={palette} onOpenChange={setPalette} extra={commands} />
        <ExportDialog
          open={exporting}
          ids={exportIds}
          onOpenChange={(o) => {
            setExporting(o);
            if (!o) setExportIds(undefined);
          }}
        />
        <VariantsPanel screen={screen} />
        <Toaster />
      </div>
    </TipProvider>
  );
}
