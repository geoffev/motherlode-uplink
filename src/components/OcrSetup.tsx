import { useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { openUrl } from '@tauri-apps/plugin-opener'
import { errorText, ipc } from '../lib/ipc'
import type { Settings } from '../lib/types'

export const TESSERACT_HELP_URL = 'https://tesseract-ocr.github.io/tessdoc/Installation.html'

const isWindows = navigator.userAgent.includes('Windows')

export async function pickTesseract(): Promise<string | null> {
  const path = await open({
    multiple: false,
    title: isWindows ? 'Find tesseract.exe' : 'Find the tesseract program',
    filters: isWindows ? [{ name: 'Tesseract', extensions: ['exe'] }] : undefined,
  })
  return typeof path === 'string' ? path : null
}

export function OcrSetupBanner({ settings, onSaved }: { settings: Settings; onSaved: (s: Settings) => void }) {
  const [error, setError] = useState<string | null>(null)

  async function browse() {
    const path = await pickTesseract()
    if (!path) return
    try {
      onSaved(await ipc.saveSettings({ ...settings, tesseractPath: path }))
      setError(null)
    } catch (e) {
      setError(errorText(e))
    }
  }

  return (
    <div className="banner banner--setup" role="status">
      <span>
        <strong>Tesseract isn't set up</strong>, so terminal prices can't be read yet.
        {isWindows ? ' Install it, then point the app at tesseract.exe.' : ' Install it from your package manager (tesseract or tesseract-ocr).'}
        {error && <span className="is-error"> {error}</span>}
      </span>
      <span className="spacer" />
      <button type="button" className="btn btn--ghost btn--sm" onClick={() => openUrl(TESSERACT_HELP_URL)}>How to install</button>
      <button type="button" className="btn btn--primary btn--sm" onClick={browse}>{isWindows ? 'Browse for tesseract.exe' : 'Browse…'}</button>
    </div>
  )
}
