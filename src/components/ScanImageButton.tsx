import { useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { errorText, ipc } from '../lib/ipc'

/** Runs a saved screenshot through the same pipeline as live capture. */
export function ScanImageButton() {
  const [msg, setMsg] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function pick() {
    const path = await open({ multiple: false, filters: [{ name: 'Screenshots', extensions: ['png', 'jpg', 'jpeg', 'bmp', 'webp'] }] })
    if (typeof path !== 'string') return
    setBusy(true)
    setMsg('Reading…')
    try {
      const o = await ipc.scanImage(path)
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
