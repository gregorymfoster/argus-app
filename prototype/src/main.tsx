// Fonts ship with the app (no Google Fonts request), so the Mac app looks the same offline.
import '@fontsource-variable/geist/wght.css';
import '@fontsource-variable/geist-mono/wght.css';
import { installMacChrome } from './ui/macChrome';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { LIVE, loadLive } from './data/live';
import { initZoom } from './ui/zoom';
import { initEditMenu } from './ui/editMenu';
import { libraryInfo } from './data/libraries';
import { FirstRun } from './settings/FirstRun';
import { LibraryProvider } from './store/library';
import { VariantsProvider } from './store/variants';
import './styles.css';
import './views.css';

installMacChrome();

// ?demo=culled skips Import and starts with AI Verdicts written, for
// screenshots and for jumping straight to the cull. Live mode (the desktop app, or
// ?library=live against `argus serve`, see README) shows the real Library and also
// skips Import, unless the Library is still empty.
const skipImport = LIVE || new URLSearchParams(location.search).get('demo') === 'culled' || (location.hash !== '' && !location.hash.startsWith('#/import'));
initZoom();
initEditMenu();
const root = createRoot(document.getElementById('root')!);

async function start() {
  if (LIVE) {
    // No Library yet (first launch), or its disk isn't connected: ask before anything else.
    try {
      const info = await libraryInfo();
      if (!info.current || !info.exists) {
        root.render(<FirstRun info={info} />);
        return;
      }
    } catch {
      // An older `argus serve` without Library commands: carry on with its Library.
    }
    try {
      const any = await loadLive();
      if (!any) location.hash = '#/import';
    } catch (e) {
      root.render(<pre style={{ color: 'var(--reject)', padding: 24, whiteSpace: 'pre-wrap' }}>Live Library unavailable: {String(e)}</pre>);
      return;
    }
  }
  root.render(
    <StrictMode>
      <VariantsProvider>
        <LibraryProvider skipImport={skipImport}>
          <App />
        </LibraryProvider>
      </VariantsProvider>
    </StrictMode>,
  );
}
void start();
