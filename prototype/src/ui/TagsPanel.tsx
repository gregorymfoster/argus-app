// The loupe's Caption and Keywords (Lightroom's Keywording panel, simplified). The AI's
// suggestions are dashed with a sparkle until kept: click one to keep it, × to dismiss it,
// ⇧↵ keeps them all. K adds a Keyword with autocomplete from the Library's own. People are
// never guessed: the faces the models found show as unnamed circles to name.

import { useEffect, useRef, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Plus, Sparkles, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Kbd } from './kit';
import { aiName } from '../data/live';
import { FACETS, changeKeywords, keywordCounts, keywordLabel, keywordPath, parseKeyword, setCaption, useTags, type KeywordCount, type PhotoKeyword } from '../data/tags';

export function TagsPanel({ photoId, faces }: { photoId: string; faces: number }) {
  const tags = useTags(photoId);
  const [adding, setAdding] = useState<string | null>(null);
  const keywords = tags?.keywords ?? [];
  const suggested = keywords.filter((k) => k.suggested);
  const caption = tags?.caption ?? null;
  const pending = suggested.length + (caption?.suggested ? 1 : 0);
  const acceptAll = () => pending && void changeKeywords([photoId], { acceptAll: true });

  useHotkeys('k', () => setAdding(''), { preventDefault: true }, [photoId]);
  useHotkeys('shift+enter', acceptAll, { preventDefault: true }, [photoId, pending]);
  useEffect(() => setAdding(null), [photoId]);

  const byFacet = (f: string) => keywords.filter((k) => k.facet === f);
  const people = byFacet('People');
  return (
    <>
      <section data-testid="caption">
        <div className="row side-title">
          <h2>Caption</h2>
          <span className="grow" />
          {caption?.suggested && (
            <span className="faint mono tag-by">
              <Sparkles size={11} /> by {aiName()}
            </span>
          )}
        </div>
        <CaptionBox key={`${photoId}:${caption?.text ?? ''}`} photoId={photoId} text={caption?.text ?? ''} ai={!!caption?.suggested} />
      </section>
      <section data-testid="keywords">
        <div className="row side-title">
          <h2>Keywords</h2>
          <span className="grow" />
          {keywords.length > 0 && (
            <span className="faint mono tag-count">
              {keywords.length - suggested.length} of {keywords.length} kept
            </span>
          )}
          {pending > 0 && (
            <Button size="sm" variant="ghost" onClick={acceptAll} data-testid="accept-all" title="Keep every AI suggestion and the caption">
              Accept all <Kbd quiet>⇧↵</Kbd>
            </Button>
          )}
        </div>
        <div className="kw-rows">
          {(faces > 0 || people.length > 0) && (
            <div className="kw-row">
              <span className="kw-facet">People</span>
              <div className="kw-chips">
                {people.map((k) => (
                  <Chip key={k.keyword} k={k} photoId={photoId} />
                ))}
                {Array.from({ length: Math.max(0, Math.min(6, faces - people.length)) }, (_, i) => (
                  <button key={i} className="kw-face" aria-label="Name this person" title="Name this person" onClick={() => setAdding('People › ')}>
                    ?
                  </button>
                ))}
                {faces > people.length && <span className="faint kw-note">{faces === 1 ? '1 face' : `${faces} faces`} · click to name</span>}
              </div>
            </div>
          )}
          {FACETS.filter((f) => f !== 'People').map((f) => {
            const list = byFacet(f);
            if (!list.length) return null;
            return (
              <div key={f} className="kw-row">
                <span className="kw-facet">{f}</span>
                <div className="kw-chips">
                  {list.map((k) => (
                    <Chip key={k.keyword} k={k} photoId={photoId} />
                  ))}
                </div>
              </div>
            );
          })}
          {tags && !keywords.length && !faces && <p className="faint kw-note">No Keywords yet. The AI suggests some after a cull, or press K to add one.</p>}
        </div>
        {adding === null ? (
          <button className="kw-add" onClick={() => setAdding('')} data-testid="kw-add">
            <Plus size={12} /> Add keyword <span className="grow" />
            <Kbd quiet>K</Kbd>
          </button>
        ) : (
          <KeywordInput photoId={photoId} initial={adding} done={() => setAdding(null)} />
        )}
      </section>
    </>
  );
}

function Chip({ k, photoId }: { k: PhotoKeyword; photoId: string }) {
  const label = keywordLabel(k.keyword);
  return (
    <span className={`kw-chip ${k.suggested ? 'ai' : ''}`} data-testid="kw-chip" data-suggested={k.suggested || undefined}>
      {k.suggested ? (
        <button className="kw-keep" onClick={() => void changeKeywords([photoId], { accept: [k.keyword] })} title={`Keep “${label}” (the AI's suggestion)`}>
          <Sparkles size={10} />
          {label}
        </button>
      ) : (
        <span title={k.keyword.replaceAll('|', ' › ')}>{label}</span>
      )}
      <button className="kw-x" aria-label={k.suggested ? `Dismiss ${label}` : `Remove ${label}`} onClick={() => void changeKeywords([photoId], { remove: [k.keyword] })}>
        <X size={10} />
      </button>
    </span>
  );
}

function CaptionBox({ photoId, text, ai }: { photoId: string; text: string; ai: boolean }) {
  const [value, setValue] = useState(text);
  const save = () => {
    if (value.trim() !== text.trim()) void setCaption([photoId], value);
  };
  return (
    <textarea
      className={`cap-box ${ai ? 'ai' : ''}`}
      aria-label="Caption"
      rows={2}
      value={value}
      placeholder="Who, what, where"
      onChange={(e) => setValue(e.target.value)}
      onBlur={save}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
          e.preventDefault();
          (e.target as HTMLTextAreaElement).blur();
        }
        if (e.key === 'Escape') {
          setValue(text);
          e.stopPropagation();
          (e.target as HTMLTextAreaElement).blur();
        }
      }}
    />
  );
}

/** The K field: Library Keywords that match, then "New keyword … under <Facet>". */
function KeywordInput({ photoId, initial, done }: { photoId: string; initial: string; done: () => void }) {
  const [text, setText] = useState(initial);
  const [hits, setHits] = useState<KeywordCount[]>([]);
  const [on, setOn] = useState(0);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => ref.current?.focus(), []);
  const query = text.replace(/^(people|places|events|activities|subjects)\s*[›|>/]\s*/i, '');
  useEffect(() => {
    let live = true;
    if (!query.trim()) return setHits([]);
    keywordCounts(query.split(/[›|>/]/).pop()!.trim()).then((h) => live && setHits(h));
    return () => {
      live = false;
    };
  }, [query]);
  const parsed = parseKeyword(text);
  const fresh = parsed && !hits.some((h) => h.keyword.toLowerCase() === keywordPath(parsed).toLowerCase()) ? keywordPath(parsed) : null;
  const options = [...hits.map((h) => h.keyword), ...(fresh ? [fresh] : [])];
  const add = (keyword: string) => {
    void changeKeywords([photoId], { add: [keyword] });
    setText('');
    setOn(0);
  };
  return (
    <div className="kw-input-wrap">
      <label className="kw-input">
        <Plus size={12} />
        <input
          ref={ref}
          aria-label="Add keyword"
          value={text}
          placeholder="Keyword, or Places › Portugal › Lisbon"
          onChange={(e) => {
            setText(e.target.value);
            setOn(0);
          }}
          onBlur={() => setTimeout(done, 120)}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown') setOn((i) => Math.min(options.length - 1, i + 1));
            else if (e.key === 'ArrowUp') setOn((i) => Math.max(0, i - 1));
            else if (e.key === 'Enter' && options[on]) add(options[on]);
            else if (e.key === 'Escape') {
              e.stopPropagation();
              done();
            } else return;
            e.preventDefault();
          }}
        />
        <Kbd quiet>↵</Kbd>
      </label>
      {options.length > 0 && (
        <div className="kw-menu" role="listbox" aria-label="Keywords">
          {hits.map((h, i) => (
            <div key={h.keyword} role="option" aria-selected={i === on} className={`kw-opt ${i === on ? 'on' : ''}`} onMouseDown={(e) => (e.preventDefault(), add(h.keyword))}>
              <span className="trunc">{h.keyword.replaceAll('|', ' › ')}</span>
              <span className="mono kw-n">{h.photos}</span>
            </div>
          ))}
          {fresh && (
            <>
              {hits.length > 0 && <div className="menu-sep" />}
              <div role="option" aria-selected={on === hits.length} className={`kw-opt ${on === hits.length ? 'on' : ''}`} onMouseDown={(e) => (e.preventDefault(), add(fresh))}>
                <span className="trunc">New “{keywordLabel(fresh)}”</span>
                <span className="kw-n">under {fresh.split('|')[0]}</span>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
