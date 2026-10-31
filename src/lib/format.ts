const int = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })
const scu = new Intl.NumberFormat('en-US', { maximumFractionDigits: 2 })

export const fmtPrice = (n: number | null | undefined) => (n == null ? '—' : int.format(n))
export const fmtScu = (n: number | null | undefined) => (n == null ? '—' : scu.format(n))

export function fmtTime(iso: string): string {
  const d = new Date(iso)
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

export function fmtAgo(iso: string): string {
  const s = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000)
  if (s < 60) return `${Math.round(s)}s ago`
  if (s < 3600) return `${Math.round(s / 60)}m ago`
  if (s < 86400) return `${Math.round(s / 3600)}h ago`
  return new Date(iso).toLocaleDateString()
}

/** Typed text → whole aUEC. Accepts "10,138", "10138", "10.1k". */
export function parsePriceInput(s: string): number | null {
  const n = parseNumberInput(s)
  return n == null ? null : Math.round(n)
}

export function parseScuInput(s: string): number | null {
  const n = parseNumberInput(s)
  return n == null ? null : Math.round(n * 100) / 100
}

function parseNumberInput(s: string): number | null {
  const t = s.trim().replace(/[\s,]/g, '').toLowerCase()
  if (!t) return null
  const m = /^(\d+(?:\.\d+)?)(k|m)?$/.exec(t)
  if (!m) return NaN
  return parseFloat(m[1]) * (m[2] === 'k' ? 1e3 : m[2] === 'm' ? 1e6 : 1)
}
