import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { errorText, fileUrl, ipc } from '../lib/ipc'
import { fmtAgo, fmtPrice, fmtScu, fmtTime, parsePriceInput, parseScuInput } from '../lib/format'
import {
  commodityLabel, terminalLabel,
  type Catalog, type CaptureStatus, type Confirmation, type NormRect, type Reading, type Settings, type Side,
} from '../lib/types'
import { Combobox, type Option } from './Combobox'
import { FieldRow, LOW_CONFIDENCE, SizeChips, useFlash } from './Fields'
import { ScanImageButton } from './ScanImageButton'

/**
 * Only fields the player touched. Anything in here wins over OCR, so a later
 * OCR pass on the same row never overwrites an edit.
 */
interface Draft {
  side?: Side
  commodityId?: string
  price?: string
  quantity?: string
  sizes?: number[]
}

interface Resolved {
  side: Side
  terminalId: string | null
  commodityId: string | null
  price: string
  quantity: string
  sizes: number[]
  edited: { side: boolean; commodity: boolean; price: boolean; quantity: boolean; sizes: boolean }
}

interface Props {
  readings: Reading[]
  catalog: Catalog
  settings: Settings | null
  status: CaptureStatus | null
  active: boolean
}

const isTextTarget = (t: EventTarget | null) =>
  t instanceof HTMLInputElement && t.type !== 'checkbox' || t instanceof HTMLTextAreaElement

export function ReviewView({ readings, catalog, settings, status, active }: Props) {
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [drafts, setDrafts] = useState<Record<string, Draft>>({})
  /** The terminal is chosen per terminal screen: it applies to every reading from it. */
  const [screenTerminal, setScreenTerminal] = useState<Record<string, string>>({})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const lastIndex = useRef(0)

  const selected = readings.find(r => r.id === selectedId) ?? readings[Math.min(lastIndex.current, readings.length - 1)] ?? null
  useEffect(() => {
    if (selected) {
      lastIndex.current = readings.indexOf(selected)
      if (selected.id !== selectedId) setSelectedId(selected.id)
    }
  }, [selected, selectedId, readings])
  useEffect(() => setError(null), [selectedId])

  const commodityOptions = useMemo<Option[]>(
    () => catalog.commodities.map(c => ({ id: c.id, label: commodityLabel(c) })),
    [catalog.commodities],
  )
  const terminalOptions = useMemo<Option[]>(
    () => catalog.terminals.map(t => ({ id: t.id, label: terminalLabel(t), hint: t.system })),
    [catalog.terminals],
  )
  const commodityName = useCallback(
    (id: string | null) => (id ? catalog.commodities.find(c => c.id === id) : undefined),
    [catalog.commodities],
  )

  const resolve = useCallback((r: Reading): Resolved => {
    const d = drafts[r.id] ?? {}
    return {
      side: d.side ?? r.side,
      terminalId: screenTerminal[r.screenId] ?? r.terminalId ?? settings?.lastTerminalId ?? null,
      commodityId: d.commodityId ?? r.commodity.value,
      price: d.price ?? (r.price.value != null ? fmtPrice(r.price.value) : ''),
      quantity: d.quantity ?? (r.quantity.value != null ? fmtScu(r.quantity.value) : ''),
      sizes: d.sizes ?? r.sizes.value ?? [],
      edited: {
        side: d.side !== undefined,
        commodity: d.commodityId !== undefined,
        price: d.price !== undefined,
        quantity: d.quantity !== undefined,
        sizes: d.sizes !== undefined,
      },
    }
  }, [drafts, screenTerminal, settings?.lastTerminalId])

  const edit = (id: string, patch: Draft) => setDrafts(d => ({ ...d, [id]: { ...d[id], ...patch } }))

  function toConfirmation(r: Reading): Confirmation | string {
    const v = resolve(r)
    const name = commodityName(v.commodityId)
    if (!v.terminalId) return 'Pick the terminal these prices are from'
    if (!name) return r.commodityCandidates.length ? `“${r.commodity.text}” matches several commodities: pick one` : 'Pick the commodity'
    const label = commodityLabel(name)
    const price = parsePriceInput(v.price)
    if (Number.isNaN(price)) return `${label}: the price isn't a number`
    // Sell rows (IN DEMAND): only the price is the terminal's.
    if (v.side === 'sell') {
      if (price == null) return `${label}: enter the price`
      return { readingId: r.id, terminalId: v.terminalId, commodityId: name.id, side: 'sell', price, quantity: null, sizes: [] }
    }
    const quantity = parseScuInput(v.quantity)
    if (Number.isNaN(quantity)) return `${label}: the stock isn't a number`
    if (price == null && quantity == null && !v.sizes.length) return `${label}: nothing to report`
    return { readingId: r.id, terminalId: v.terminalId, commodityId: name.id, side: 'buy', price, quantity, sizes: v.sizes }
  }

  async function confirm(list: Reading[]) {
    const items: Confirmation[] = []
    for (const r of list) {
      const c = toConfirmation(r)
      if (typeof c === 'string') {
        setSelectedId(r.id)
        setError(c)
        return
      }
      items.push(c)
    }
    setBusy(true)
    try {
      await ipc.confirmReadings(items)
      setDrafts(d => {
        const next = { ...d }
        for (const c of items) delete next[c.readingId]
        return next
      })
      setError(null)
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy(false)
    }
  }

  async function reject(r: Reading) {
    await ipc.rejectReading(r.id)
    setDrafts(d => {
      const next = { ...d }
      delete next[r.id]
      return next
    })
  }

  const sameScreen = selected ? readings.filter(r => r.screenId === selected.screenId) : []

  // Keyboard: Enter confirms, Shift+Enter confirms the whole screen,
  // Backspace rejects, arrows move through the queue, Tab moves through fields.
  const keyState = useRef({ selected, sameScreen, readings, busy })
  keyState.current = { selected, sameScreen, readings, busy }
  useEffect(() => {
    if (!active) return
    const onKey = (e: KeyboardEvent) => {
      const { selected, sameScreen, readings, busy } = keyState.current
      if (!selected || busy || e.ctrlKey || e.metaKey || e.altKey) return
      const t = e.target as HTMLElement | null
      if (e.key === 'Enter') {
        if (t?.closest('[data-native-enter]')) return
        e.preventDefault()
        confirm(e.shiftKey ? sameScreen : [selected])
      } else if (e.key === 'Backspace' && !isTextTarget(t)) {
        e.preventDefault()
        reject(selected)
      } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        const i = readings.indexOf(selected) + (e.key === 'ArrowDown' ? 1 : -1)
        if (readings[i]) setSelectedId(readings[i].id)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  if (!readings.length) return <EmptyQueue status={status} />

  return (
    <div className="review">
      <aside className="panel queue" aria-label="Review queue">
        <div className="panel-head">
          <span className="t-label">Queue · {readings.length}</span>
        </div>
        <ul className="queue-list" role="listbox" aria-label="Readings">
          {readings.map((r, i) => {
            const v = resolve(r)
            const c = commodityName(v.commodityId)
            const t = catalog.terminals.find(x => x.id === v.terminalId)
            const checked = v.side === 'sell' ? [r.commodity, r.price] : [r.commodity, r.price, r.quantity]
            const low = checked.some(f => f.confidence < LOW_CONFIDENCE)
            const newScreen = i > 0 && readings[i - 1].screenId !== r.screenId
            return (
              <li key={r.id} className={newScreen ? 'queue-gap' : undefined}>
                <button
                  type="button"
                  role="option"
                  aria-selected={r.id === selected?.id}
                  className="queue-item"
                  onClick={() => setSelectedId(r.id)}
                  data-native-enter
                >
                  <span className="queue-item-top">
                    <span className={`tag tag--${v.side}`}>{v.side}</span>
                    <span className="queue-name">
                      {c ? commodityLabel(c) : r.commodityCandidates.length ? `${r.commodity.text} · pick one` : r.commodity.text || 'Unknown commodity'}
                    </span>
                    {low && <span className="queue-low" title="Some fields were hard to read">●</span>}
                  </span>
                  <span className="queue-item-sub t-caption">
                    <span>{t ? terminalLabel(t) : 'No terminal'}</span>
                    <span className="t-data-sm">{fmtTime(r.createdAt)}</span>
                  </span>
                </button>
              </li>
            )
          })}
        </ul>
      </aside>

      {selected && (
        <Detail
          key={selected.id}
          reading={selected}
          resolved={resolve(selected)}
          sameScreenCount={sameScreen.length}
          commodityOptions={commodityOptions}
          terminalOptions={terminalOptions}
          error={error}
          busy={busy}
          onEdit={patch => edit(selected.id, patch)}
          onTerminal={id => setScreenTerminal(s => ({ ...s, [selected.screenId]: id }))}
          onConfirm={() => confirm([selected])}
          onConfirmAll={() => confirm(sameScreen)}
          onReject={() => reject(selected)}
        />
      )}
    </div>
  )
}

interface DetailProps {
  reading: Reading
  resolved: Resolved
  sameScreenCount: number
  commodityOptions: Option[]
  terminalOptions: Option[]
  error: string | null
  busy: boolean
  onEdit: (d: Draft) => void
  onTerminal: (id: string) => void
  onConfirm: () => void
  onConfirmAll: () => void
  onReject: () => void
}

function Detail(p: DetailProps) {
  const { reading: r, resolved: v } = p
  const [hover, setHover] = useState<string | null>(null)
  /** The screenshot, or one field's crop, open in the enlarged view. */
  const [enlarged, setEnlarged] = useState<null | 'frame' | { src: string; label: string }>(null)
  const expand = (src: string, label: string) => setEnlarged({ src, label })
  const priceFlash = useFlash(v.edited.price ? null : r.price.value)
  const qtyFlash = useFlash(v.edited.quantity ? null : r.quantity.value)
  const sell = v.side === 'sell'
  const uncertain = (conf: number, edited: boolean) => !edited && conf < LOW_CONFIDENCE

  const boxes: FrameBox[] = [
    { key: 'commodity', label: 'Commodity', rect: r.commodity.rect },
    { key: 'price', label: 'Price', rect: r.price.rect },
    ...(sell ? [] : [
      { key: 'quantity', label: 'Stock', rect: r.quantity.rect },
      { key: 'sizes', label: 'Sizes', rect: r.sizes.rect },
    ]),
  ]
  const hot = (key: string) => ({ onMouseEnter: () => setHover(key), onMouseLeave: () => setHover(null), onFocus: () => setHover(key) })
  const options = r.commodityCandidates.length
    ? p.commodityOptions.filter(o => r.commodityCandidates.includes(o.id))
    : p.commodityOptions

  return (
    <section className="panel detail" aria-label="Reading">
      <div className="panel-head">
        <span className="t-label">
          Reading · {fmtAgo(r.createdAt)}
          {r.revision > 0 && <> · re-read {r.revision}×</>}
        </span>
        <div className="chips" role="group" aria-label="Which list the row is from">
          {(['sell', 'buy'] as const).map(s => (
            <button key={s} type="button" className="chip chip--text" aria-pressed={v.side === s} onClick={() => p.onEdit({ side: s })}>
              {s === 'sell' ? 'Sell · in demand' : 'Buy · in stock'}
            </button>
          ))}
        </div>
      </div>

      <div className="detail-body">
        {r.frame ? (
          <button type="button" className="frame frame--button" onClick={() => setEnlarged('frame')} aria-label="Enlarge the screenshot" data-native-enter>
            <Frame reading={r} boxes={boxes} hover={hover} />
          </button>
        ) : (
          <div className="frame"><div className="frame-missing t-caption">No frame kept</div></div>
        )}
        {enlarged && enlarged !== 'frame' && (
          <CropLightbox src={enlarged.src} label={enlarged.label} onClose={() => setEnlarged(null)} />
        )}
        {enlarged === 'frame' && (
          <Lightbox aspect={(r.frameWidth || 16) / (r.frameHeight || 9)} label="Screenshot" onClose={() => setEnlarged(null)}>
            <Frame reading={r} boxes={boxes} hover={hover} />
          </Lightbox>
        )}

        <div className="fields">
          <div {...hot('commodity')}>
            <FieldRow label="Commodity" htmlFor="f-commodity" crop={r.commodity.crop} confidence={r.commodity.confidence} edited={v.edited.commodity} onExpand={expand}>
              <Combobox
                inputId="f-commodity"
                options={options}
                value={v.commodityId}
                onChange={id => p.onEdit({ commodityId: id })}
                placeholder={
                  r.commodityCandidates.length ? `“${r.commodity.text}” matches ${r.commodityCandidates.length}: pick one`
                    : r.commodity.text ? `Read as “${r.commodity.text}”` : 'Search commodities'
                }
                uncertain={uncertain(r.commodity.confidence, v.edited.commodity)}
              />
            </FieldRow>
          </div>
          <div {...hot('price')}>
            <FieldRow label={sell ? 'Price it pays' : 'Price it charges'} htmlFor="f-price" crop={r.price.crop} confidence={r.price.confidence} edited={v.edited.price} onExpand={expand}>
              <div className="input-unit">
                <input
                  id="f-price"
                  key={priceFlash?.n}
                  className={`input${uncertain(r.price.confidence, v.edited.price) ? ' is-uncertain' : ''} ${priceFlash?.cls ?? ''}`}
                  inputMode="numeric"
                  autoComplete="off"
                  value={v.price}
                  onChange={e => p.onEdit({ price: e.target.value })}
                />
                <span className="t-data-sm">aUEC/SCU</span>
              </div>
            </FieldRow>
          </div>

          {sell ? (
            <p className="t-caption field-note">
              From the IN DEMAND list
            </p>
          ) : (
            <>
              <div {...hot('quantity')}>
                <FieldRow label="Stock" htmlFor="f-qty" crop={r.quantity.crop} confidence={r.quantity.confidence} edited={v.edited.quantity} onExpand={expand}>
                  <div className="input-unit">
                    <input
                      id="f-qty"
                      key={qtyFlash?.n}
                      className={`input${uncertain(r.quantity.confidence, v.edited.quantity) ? ' is-uncertain' : ''} ${qtyFlash?.cls ?? ''}`}
                      inputMode="decimal"
                      autoComplete="off"
                      value={v.quantity}
                      onChange={e => p.onEdit({ quantity: e.target.value })}
                    />
                    <span className="t-data-sm">SCU</span>
                  </div>
                </FieldRow>
              </div>
              <div {...hot('sizes')}>
                <FieldRow label="Sizes it has" crop={r.sizes.crop} confidence={r.sizes.confidence} edited={v.edited.sizes} onExpand={expand}>
                  <SizeChips value={v.sizes} onChange={sizes => p.onEdit({ sizes })} uncertain={uncertain(r.sizes.confidence, v.edited.sizes)} />
                </FieldRow>
              </div>
            </>
          )}

          <div className="field-row field-row--terminal">
            <label className="t-label field-label" htmlFor="f-terminal">Terminal</label>
            <div className="field-terminal">
              <Combobox
                inputId="f-terminal"
                options={p.terminalOptions}
                value={v.terminalId}
                onChange={p.onTerminal}
                placeholder="Search terminals: “Orison”, “Pyro Gateway”…"
              />
              <span className="t-caption">
                {r.terminalSource === 'screen' && `Matched “${r.locationText ?? ''}” on screen. `}
                {r.terminalSource !== 'screen' && r.locationText && `Read “${r.locationText}” on screen, which matched no terminal. `}
                {r.terminalSource === 'last' && 'Showing your last terminal. '}
                {p.sameScreenCount > 1 && `Applies to all ${p.sameScreenCount} readings from this screen.`}
              </span>
            </div>
          </div>
        </div>
      </div>

      {p.error && <div className="detail-error" role="alert">{p.error}</div>}

      <div className="detail-actions">
        <button type="button" className="btn btn--danger" onClick={p.onReject} disabled={p.busy} data-native-enter>
          Reject <kbd>⌫</kbd>
        </button>
        <span className="spacer" />
        {p.sameScreenCount > 1 && (
          <button type="button" className="btn btn--secondary" onClick={p.onConfirmAll} disabled={p.busy} data-native-enter>
            Confirm all {p.sameScreenCount} <kbd>⇧ Enter</kbd>
          </button>
        )}
        <button type="button" className="btn btn--primary" onClick={p.onConfirm} disabled={p.busy} data-native-enter>
          Confirm &amp; send <kbd>Enter</kbd>
        </button>
      </div>
    </section>
  )
}

interface FrameBox { key: string; label: string; rect: NormRect | null }

/** The captured screen with the read regions outlined. */
function Frame({ reading: r, boxes, hover }: { reading: Reading; boxes: FrameBox[]; hover: string | null }) {
  return (
    <span className="frame-inner" style={{ aspectRatio: `${r.frameWidth || 16} / ${r.frameHeight || 9}` }}>
      <img src={fileUrl(r.frame)} alt="Captured terminal screen" />
      {boxes.map(b => b.rect && (
        <span
          key={b.key}
          className={`frame-box${hover === b.key ? ' is-hot' : ''}`}
          style={{ left: `${b.rect.x * 100}%`, top: `${b.rect.y * 100}%`, width: `${b.rect.w * 100}%`, height: `${b.rect.h * 100}%` }}
          aria-hidden="true"
        >
          <span>{b.label}</span>
        </span>
      ))}
    </span>
  )
}

/** Full-window view of the screenshot. Escape, the close button or a click outside closes it. */
/** One field's crop, enlarged to fit the window; pixels stay sharp so misread digits are easy to check. */
function CropLightbox({ src, label, onClose }: { src: string; label: string; onClose: () => void }) {
  const [aspect, setAspect] = useState(4)
  return (
    <Lightbox aspect={aspect} label={`${label} as captured`} onClose={onClose}>
      <img
        className="lightbox-crop"
        src={fileUrl(src)}
        alt={`${label} as captured`}
        onLoad={e => {
          const img = e.currentTarget
          if (img.naturalWidth && img.naturalHeight) setAspect(img.naturalWidth / img.naturalHeight)
        }}
      />
    </Lightbox>
  )
}

function Lightbox({ aspect, label, onClose, children }: { aspect: number; label: string; onClose: () => void; children: React.ReactNode }) {
  const closeRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null
    closeRef.current?.focus()
    // Capture phase: the review shortcuts (Enter, Backspace, arrows) stay off while it's open.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      if (e.key !== 'Tab') {
        e.stopImmediatePropagation()
        if (e.key !== 'Enter' && e.key !== ' ') e.preventDefault()
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      opener?.focus()
    }
  }, [onClose])

  return (
    <div className="lightbox" role="dialog" aria-modal="true" aria-label={label} onClick={onClose}>
      <span className="t-label lightbox-title">{label}</span>
      <div className="lightbox-frame" style={{ width: `min(100%, calc((100vh - 72px) * ${aspect}))` }} onClick={e => e.stopPropagation()}>
        {children}
      </div>
      <button ref={closeRef} type="button" className="btn btn--secondary lightbox-close" onClick={onClose} aria-label="Close the screenshot">
        Close <kbd>Esc</kbd>
      </button>
    </div>
  )
}

function EmptyQueue({ status }: { status: CaptureStatus | null }) {
  return (
    <div className="empty panel">
      <div className="empty-inner">
        <h2 className="t-h3">No readings waiting</h2>
        <p className="t-body empty-text">
          Open a commodity terminal in game and hold still for a moment. Each commodity on the screen shows up here
          for you to check before it's sent.
        </p>
        {status?.state === 'paused' && <p className="t-caption">Watching is paused. Resume it from the header or the tray.</p>}
        {status?.state === 'noGame' && <p className="t-caption">{status.message}</p>}
        <div className="empty-actions">
          <ScanImageButton />
          {import.meta.env.DEV && (
            <button type="button" className="btn btn--ghost" onClick={() => ipc.injectDemoReading()}>Add demo reading</button>
          )}
        </div>
      </div>
    </div>
  )
}
