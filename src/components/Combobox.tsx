import { useEffect, useId, useMemo, useRef, useState } from 'react'

export interface Option { id: string; label: string; hint?: string }

interface Props {
  options: Option[]
  value: string | null
  onChange: (id: string) => void
  placeholder?: string
  uncertain?: boolean
  inputId?: string
  'aria-label'?: string
}

const MAX_SHOWN = 60

function matches(o: Option, words: string[]) {
  const hay = `${o.label} ${o.hint ?? ''}`.toLowerCase()
  return words.every(w => hay.includes(w))
}

/** Searchable single-select. Arrow keys / Enter / Escape stay inside it while the list is open. */
export function Combobox({ options, value, onChange, placeholder, uncertain, inputId, ...rest }: Props) {
  const listId = useId()
  const selected = options.find(o => o.id === value) ?? null
  const [query, setQuery] = useState<string | null>(null)
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(0)
  const listRef = useRef<HTMLUListElement>(null)

  const shown = useMemo(() => {
    const words = (query ?? '').toLowerCase().split(/\s+/).filter(Boolean)
    return (words.length ? options.filter(o => matches(o, words)) : options).slice(0, MAX_SHOWN)
  }, [options, query])

  useEffect(() => { setActive(0) }, [query])
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-i="${active}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [active])

  const pick = (o: Option) => {
    onChange(o.id)
    setQuery(null)
    setOpen(false)
  }

  return (
    <div className="combo">
      <input
        id={inputId}
        className={`input${uncertain ? ' is-uncertain' : ''}`}
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={open && shown[active] ? `${listId}-${active}` : undefined}
        aria-label={rest['aria-label']}
        placeholder={placeholder}
        value={query ?? selected?.label ?? ''}
        onChange={e => { setQuery(e.target.value); setOpen(true) }}
        onFocus={e => e.target.select()}
        onBlur={() => { setOpen(false); setQuery(null) }}
        onKeyDown={e => {
          if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
            e.preventDefault()
            e.stopPropagation()
            if (!open) { setOpen(true); return }
            setActive(a => Math.max(0, Math.min(shown.length - 1, a + (e.key === 'ArrowDown' ? 1 : -1))))
          } else if (e.key === 'Enter' && open) {
            e.preventDefault()
            e.stopPropagation()
            if (shown[active]) pick(shown[active])
          } else if (e.key === 'Escape' && open) {
            e.stopPropagation()
            setOpen(false)
            setQuery(null)
          } else if (e.key === 'Backspace') {
            e.stopPropagation()
          }
        }}
      />
      {open && (
        <ul className="combo-list" id={listId} role="listbox" ref={listRef}>
          {shown.length === 0 && <li className="combo-empty t-caption">No matches</li>}
          {shown.map((o, i) => (
            <li
              key={o.id}
              id={`${listId}-${i}`}
              data-i={i}
              role="option"
              aria-selected={o.id === value}
              className={`combo-opt${i === active ? ' is-active' : ''}`}
              onMouseDown={e => { e.preventDefault(); pick(o) }}
              onMouseEnter={() => setActive(i)}
            >
              <span>{o.label}</span>
              {o.hint && <span className="combo-hint">{o.hint}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
