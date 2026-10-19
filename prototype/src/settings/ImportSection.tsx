// Settings › Import: where each choice in the Import sheet starts (Lightroom's Import
// presets, simplified to defaults). The sheet still changes any of them for one Import.

import { Checkbox } from '@/components/ui/checkbox';
import { Switch } from '@/components/ui/switch';
import { usePref } from '../store/variants';
import { Seg } from '../ui/kit';
import { Row } from './Row';

export function ImportSection() {
  const [show, setShow] = usePref<'new' | 'all'>('import-show', 'new');
  const [rawOnly, setRawOnly] = usePref<'1' | '0'>('import-raw-only', '1');
  const [copy, setCopy] = usePref<'1' | '0'>('import-copy', '0');
  const [aiName, setAiName] = usePref<'1' | '0'>('import-ai-name', '1');
  const [cull, setCull] = usePref<'1' | '0'>('import-cull', '1');
  const [suggest, setSuggest] = usePref<'1' | '0'>('import-suggest', '1');
  const [auto, setAuto] = usePref<'1' | '0'>('import-auto', '0');
  return (
    <div className="set-page" data-testid="settings-import-page">
      <h3>Source</h3>
      <Row title="Show" note="New only hides what this Library already has, so a card you imported last week shows only today’s frames.">
        <Seg label="Show" value={show} onChange={setShow} items={[{ value: 'new', label: 'New only' }, { value: 'all', label: 'All photos' }]} />
      </Row>
      <Row title="RAW+JPEG pairs" note="Either way the pair is one Photo. RAW only leaves the camera’s JPEG on the card.">
        <Seg label="RAW+JPEG pairs" value={rawOnly} onChange={setRawOnly} items={[{ value: '1', label: 'RAW only' }, { value: '0', label: 'Keep both' }]} />
      </Row>
      <Row title="Photos from a folder" note="Cards are always copied, since they get unplugged. A folder can stay where it is, like Lightroom’s Add.">
        <Seg label="Photos from a folder" value={copy} onChange={setCopy} items={[{ value: '0', label: 'Add in place' }, { value: '1', label: 'Copy into Library' }]} />
      </Row>
      <h3>Shoot</h3>
      <Row title="Name Shoots with AI" note="Looks at a few photos and suggests a name after the date, like 2026-10-18_Boat-Trip. Off, the date alone names it.">
        <Switch checked={aiName === '1'} onCheckedChange={(c) => setAiName(c ? '1' : '0')} aria-label="Name Shoots with AI" data-testid="settings-ai-name" />
      </Row>
      <h3>After import</h3>
      <Row title="Start in the background" note="Runs at the Compute level set in AI and Compute. Suggestions wait for you to accept them; the cull and Auto tone undo with ⌘Z.">
        <label className="imp-check">
          <Checkbox checked={cull === '1'} onCheckedChange={(c) => setCull(c === true ? '1' : '0')} /> AI cull
        </label>
        <label className="imp-check">
          <Checkbox checked={suggest === '1'} onCheckedChange={(c) => setSuggest(c === true ? '1' : '0')} /> Suggest edits
        </label>
        <label className="imp-check">
          <Checkbox checked={auto === '1'} onCheckedChange={(c) => setAuto(c === true ? '1' : '0')} /> Auto tone
        </label>
      </Row>
    </div>
  );
}
