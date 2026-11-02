import { useState } from 'react'
import { errorText, ipc } from '../lib/ipc'

export function ScanImageButton() {
  const [msg, setMsg] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function pick() {
    setBusy(true)
    setMsg('Reading…')
    try {
      const o = await ipc.scanImage()
      if (!o) { setMsg(null); return }
      if (!o.terminalFound) setMsg('No commodity terminal found in that image.')
      else if (!o.newReadings && !o.updatedReadings) setMsg(`Terminal found, ${o.rows} rows, all already in the queue or sent.`)
      else setMsg(`Terminal found: ${o.newReadings} new, ${o.updatedReadings} updated.`)
    } catch (e) {
      setMsg(errorText(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <span className="scan-image">
      <button type="button" className="btn btn--secondary" onClick={pick} disabled={busy}>Read a screenshot…</button>
      {msg && <span className="t-caption" role="status">{msg}</span>}
    </span>
  )
}
