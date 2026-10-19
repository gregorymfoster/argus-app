// Settings › Export: where Exports go, what happens when they finish, and your own presets.
// The Export dialog reads and writes the same settings, so either place changes both.

import { useState } from 'react';
import { X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { pickFolder } from '../data/backend';
import { deleteExportPreset, exportPlace, savedExportPresets, setExportPlace, type ExportPlace } from '../ui/Export';
import { Row } from './Row';

export function ExportSection() {
  const [place, setPlace] = useState(exportPlace);
  const [presets, setPresets] = useState(savedExportPresets);
  const set = (patch: Partial<ExportPlace>) => {
    setExportPlace(patch);
    setPlace(exportPlace());
  };
  return (
    <div className="set-page" data-testid="settings-export-page">
      <h3>Location</h3>
      <Row title="Export to" note="Format, size and file names come from the preset you pick in the Export dialog">
        <span className="mono faint set-path" title={place.folder} data-testid="settings-export-folder">
          {place.folder}
        </span>
        <button
          className="import-pick"
          onClick={async () => {
            const dir = await pickFolder('Export to…', place.folder.startsWith('~') ? undefined : place.folder);
            if (dir) set({ folder: dir });
          }}
        >
          Choose…
        </button>
      </Row>
      <Row title="Put each Shoot in its own folder" note="Exports/2026-10-18_Boat-Trip/, named like the Shoot">
        <Switch checked={place.shootFolder} onCheckedChange={(c) => set({ shootFolder: c })} aria-label="Put each Shoot in its own folder" />
      </Row>
      <Row title="Show in Finder when done">
        <Switch checked={place.reveal} onCheckedChange={(c) => set({ reveal: c })} aria-label="Show in Finder when done" data-testid="settings-export-reveal" />
      </Row>
      <h3>Your presets</h3>
      {presets.length === 0 ? (
        <p className="set-note">None yet. In the Export dialog, set it up and choose Save as preset.</p>
      ) : (
        <div className="set-card">
          {presets.map((p) => (
            <div key={p.id} className="set-choice" data-testid="settings-export-preset">
              <div className="set-text">
                <div className="set-title">{p.label}</div>
                <div className="set-note">{p.summary}</div>
              </div>
              <Button
                variant="ghost"
                size="icon"
                aria-label={`Delete ${p.label}`}
                title="Delete preset"
                onClick={() => {
                  deleteExportPreset(p.id);
                  setPresets(savedExportPresets());
                }}
              >
                <X size={12} />
              </Button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
