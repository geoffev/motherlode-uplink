import { useEffect, useRef, useState, type ReactNode } from 'react'
import { fileUrl } from '../lib/ipc'
import { CARGO_SIZES } from '../lib/types'

/** Below this OCR confidence a field gets the gold underline. */
export const LOW_CONFIDENCE = 0.6

/** "flash-up" / "flash-down" for ~1.2s after a live value changes. */
export function useFlash(value: number | null | undefined) {
  const prev = useRef(value)
  const [flash, setFlash] = useState<{ cls: string; n: number } | null>(null)
  useEffect(() => {
    const p = prev.current
    prev.current = value
    if (p == null || value == null || p === value) return
    setFlash(f => ({ cls: value > p ? 'flash-up' : 'flash-down', n: (f?.n ?? 0) + 1 }))
    const t = setTimeout(() => setFlash(null), 1300)
    return () => clearTimeout(t)
  }, [value])
  return flash
}

interface RowProps {
  label: string
  htmlFor?: string
  crop: string | null
  confidence: number
  edited: boolean
  onExpand?: (src: string, label: string) => void
  children: ReactNode
}

export function FieldRow({ label, htmlFor, crop, confidence, edited, onExpand, children }: RowProps) {
  const low = !edited && confidence < LOW_CONFIDENCE
  return (
    <div className="field-row">
      <label className="t-label field-label" htmlFor={htmlFor}>
        {label}
        {edited && <span className="field-edited">edited</span>}
      </label>
      {crop && onExpand ? (
        <button
          type="button"
          className="field-crop field-crop--button"
          onClick={() => onExpand(crop, label)}
          aria-label={`Enlarge the ${label.toLowerCase()} crop`}
          data-native-enter
        >
          <img src={fileUrl(crop)} alt={`${label} as captured`} />
        </button>
      ) : (
        <div className="field-crop">
          {crop ? <img src={fileUrl(crop)} alt={`${label} as captured`} /> : <span className="t-caption">no crop</span>}
        </div>
      )}
      <div className="field-edit">
        {children}
        <span className={`field-conf t-data-sm${low ? ' is-low' : ''}`} title="OCR confidence">
          {edited ? '' : `${Math.round(confidence * 100)}%`}
        </span>
      </div>
    </div>
  )
}

export function SizeChips({ value, onChange, uncertain }: { value: number[]; onChange: (v: number[]) => void; uncertain?: boolean }) {
  return (
    <div className={`chips${uncertain ? ' is-uncertain' : ''}`} role="group" aria-label="Container sizes (SCU)">
      {CARGO_SIZES.map(s => {
        const on = value.includes(s)
        return (
          <button
            key={s}
            type="button"
            className="chip"
            aria-pressed={on}
            onClick={() => onChange(on ? value.filter(v => v !== s) : [...value, s].sort((a, b) => a - b))}
          >
            {s}
          </button>
        )
      })}
    </div>
  )
}

export function SizeSquares({ sizes }: { sizes: number[] }) {
  if (!sizes.length) return <span className="t-caption">—</span>
  return (
    <span className="size-squares" aria-label={`Sizes ${sizes.join(', ')} SCU`}>
      {sizes.map(s => <span key={s} className="size-sq">{s}</span>)}
    </span>
  )
}

export function Checkbox({ checked, onChange, children }: { checked: boolean; onChange: (v: boolean) => void; children: ReactNode }) {
  return (
    <label className="check">
      <input type="checkbox" checked={checked} onChange={e => onChange(e.target.checked)} />
      <span className="check-box" aria-hidden="true" />
      <span className="t-body">{children}</span>
    </label>
  )
}
