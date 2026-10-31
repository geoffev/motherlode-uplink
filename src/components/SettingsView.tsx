import { useEffect, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { errorText, ipc } from '../lib/ipc'
import type { AppInfo, Catalog, Session, Settings } from '../lib/types'
import { Checkbox } from './Fields'
import { pickTesseract } from './OcrSetup'

const URL_PRESETS = [
  { label: 'Production', url: 'https://cryon.rocks' },
  { label: 'Local dev', url: 'http://localhost:3000' },
  { label: 'Mock server', url: 'http://localhost:3999' },
]

interface Props {
  settings: Settings
  session: Session
  catalog: Catalog
  onSaved: (s: Settings) => void
  onSignOut: () => void
  onReplaceKey: () => void
}

export function SettingsView({ settings, session, catalog, onSaved, onSignOut, onReplaceKey }: Props) {
  const [draft, setDraft] = useState(settings)
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null)
  const [refreshing, setRefreshing] = useState(false)

  useEffect(() => setDraft(settings), [settings])
  useEffect(() => {
    ipc.getAppInfo().then(setInfo)
  }, [])

  const dirty = JSON.stringify(draft) !== JSON.stringify(settings)
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setDraft(d => ({ ...d, [k]: v }))

  async function save() {
    try {
      const s = await ipc.saveSettings(draft)
      onSaved(s)
      setInfo(await ipc.getAppInfo())
      setMsg({ ok: true, text: 'Saved' })
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) })
    }
  }

  return (
    <div className="settings">
      <section className="panel">
        <div className="panel-head"><span className="t-label">Account</span></div>
        <div className="settings-body">
          <div className="setting">
            <span className="t-label">API key</span>
            <div className="setting-inline">
              <span className="t-data">{session.maskedKey}</span>
              <button type="button" className="btn btn--secondary btn--sm" onClick={onReplaceKey}>Replace</button>
              <button type="button" className="btn btn--ghost btn--sm" onClick={onSignOut}>Sign out</button>
            </div>
            <span className="t-caption">
              {session.me ? <>Signed in as {session.me.displayName}{session.me.surveyCorps ? ' · Survey Corps' : ''}.</> : 'Not verified yet.'} Stored in your system keychain.
            </span>
          </div>
          {/* Release builds talk to production only; the choice is for development. */}
          {info?.serverLocked ?? true ? (
            <div className="setting">
              <span className="t-label">Server</span>
              <span className="t-data">{draft.apiBase}</span>
            </div>
          ) : (
            <div className="setting">
              <label className="t-label" htmlFor="s-url">Motherlode URL</label>
              <input id="s-url" className="input" value={draft.apiBase} onChange={e => set('apiBase', e.target.value)} spellCheck={false} />
              <div className="chips">
                {URL_PRESETS.map(p => (
                  <button key={p.url} type="button" className="chip chip--text" aria-pressed={draft.apiBase === p.url} onClick={() => set('apiBase', p.url)}>
                    {p.label}
                  </button>
                ))}
              </div>
            </div>
          )}
          <div className="setting">
            <span className="t-label">Game data</span>
            <div className="setting-inline">
              <span className="t-caption">
                {catalog.commodities.length} commodities, {catalog.terminals.length} terminals
                {catalog.fetchedAt && <> · updated {new Date(catalog.fetchedAt).toLocaleString()}</>}
              </span>
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                disabled={refreshing}
                onClick={async () => {
                  setRefreshing(true)
                  try { await ipc.refreshCatalog(); setMsg({ ok: true, text: 'Game data updated' }) }
                  catch (e) { setMsg({ ok: false, text: errorText(e) }) }
                  finally { setRefreshing(false) }
                }}
              >
                Refresh
              </button>
            </div>
          </div>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head"><span className="t-label">Capture</span></div>
        <div className="settings-body">
          <div className="setting">
            <span className="t-label">What's recorded</span>
            <span className="t-body">Only the Star Citizen window (StarCitizen.exe). When the game isn't running or is minimised, nothing is recorded.</span>
          </div>
          <div className="setting">
            <span className="t-label">Capture rate</span>
            <div className="chips" role="group" aria-label="Capture rate">
              {[0.5, 1, 2].map(f => (
                <button key={f} type="button" className="chip" aria-pressed={draft.captureFps === f} onClick={() => set('captureFps', f)}>
                  {f} fps
                </button>
              ))}
            </div>
            <span className="t-caption">Frames are checked at this rate on a small copy; OCR only runs once the picture holds still.</span>
          </div>
          <div className="setting">
            <Checkbox checked={draft.startWithComputer} onChange={v => set('startWithComputer', v)}>Start with my computer (in the tray)</Checkbox>
          </div>
          <div className="setting">
            <label className="t-label" htmlFor="s-tess">Tesseract (stand-in OCR)</label>
            <div className="setting-inline">
              <input
                id="s-tess"
                className="input"
                placeholder="Found automatically on PATH"
                value={draft.tesseractPath ?? ''}
                onChange={e => set('tesseractPath', e.target.value || null)}
              />
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                onClick={async () => { const p = await pickTesseract(); if (p) set('tesseractPath', p) }}
              >
                Browse
              </button>
            </div>
            {info && (
              <span className={`t-caption${info.ocrError ? ' is-error' : ''}`}>
                {info.ocrError ?? `Using ${info.ocrEngine}`}
              </span>
            )}
          </div>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head"><span className="t-label">Crops</span></div>
        <div className="settings-body">
          <div className="setting">
            <span className="t-label">Kept in</span>
            <div className="setting-inline">
              <span className="t-data-sm path">{draft.cropsDir ?? info?.cropsDir ?? '…'}</span>
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                onClick={async () => { const p = await open({ directory: true }); if (typeof p === 'string') set('cropsDir', p) }}
              >
                Change
              </button>
              {draft.cropsDir && <button type="button" className="btn btn--ghost btn--sm" onClick={() => set('cropsDir', null)}>Default</button>}
            </div>
          </div>
          <div className="setting">
            <label className="t-label" htmlFor="s-keep">Keep for (days)</label>
            <input
              id="s-keep"
              className="input input--short"
              inputMode="numeric"
              value={draft.cropRetentionDays}
              onChange={e => set('cropRetentionDays', Math.max(1, parseInt(e.target.value, 10) || 1))}
            />
            <span className="t-caption">Crops of confirmed readings, with the values you confirmed, help train the OCR model. They stay on this computer.</span>
          </div>
        </div>
      </section>

      <div className="settings-actions">
        {msg && <span className={`t-caption${msg.ok ? '' : ' is-error'}`} role="status">{msg.text}</span>}
        <span className="spacer" />
        <button type="button" className="btn btn--ghost" disabled={!dirty} onClick={() => setDraft(settings)}>Discard</button>
        <button type="button" className="btn btn--primary" disabled={!dirty} onClick={save}>Save settings</button>
      </div>
      {info && <p className="t-caption settings-foot">Version {info.version} · data in {info.dataDir}</p>}
    </div>
  )
}
