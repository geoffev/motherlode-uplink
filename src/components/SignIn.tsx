import { useState } from 'react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { errorText, ipc } from '../lib/ipc'
import type { Session, Settings } from '../lib/types'

interface Props {
  settings: Settings | null
  rejected?: boolean
  onSignedIn: (s: Session) => void
}

export function SignIn({ settings, rejected, onSignedIn }: Props) {
  const [key, setKey] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const base = settings?.apiBase ?? 'https://cryon.rocks'

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      onSignedIn(await ipc.signIn(key))
      setKey('')
    } catch (err) {
      setError(errorText(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="gate">
      <form className="panel gate-card" onSubmit={submit}>
        <div className="panel-head"><span className="t-label">{rejected ? 'Key rejected' : 'Sign in'}</span></div>
        <div className="gate-body">
          <h1 className="t-h2">{rejected ? 'Your API key stopped working' : 'Connect to Motherlode'}</h1>
          <p className="t-body gate-text">
            {rejected
              ? 'Motherlode no longer accepts the saved key (it may have been revoked). Reports are held in the queue until you add a new one.'
              : 'Create an API key on Motherlode under Profile → Developer → API keys, then paste it here. It is stored in your system keychain.'}
          </p>
          <label className="t-label" htmlFor="api-key">API key</label>
          <input
            id="api-key"
            className="input"
            type="password"
            autoComplete="off"
            spellCheck={false}
            placeholder="mlk_…"
            value={key}
            onChange={e => setKey(e.target.value)}
            autoFocus
          />
          {error && <p className="form-error" role="alert">{error}</p>}
          <div className="gate-actions">
            <button type="button" className="btn btn--ghost" onClick={() => openUrl(`${base}/profile/developer`)}>Get an API key</button>
            <button type="submit" className="btn btn--primary" disabled={busy || !key.trim()}>
              {busy ? 'Checking…' : 'Sign in'}
            </button>
          </div>
          <p className="t-caption">Server: {base}</p>
        </div>
      </form>
    </div>
  )
}

export function NotEnrolled({ session, settings, onRefresh, onSignOut }: {
  session: Session
  settings: Settings | null
  onRefresh: () => Promise<void>
  onSignOut: () => void
}) {
  const [busy, setBusy] = useState(false)
  const base = settings?.apiBase ?? 'https://cryon.rocks'
  return (
    <div className="gate">
      <div className="panel gate-card">
        <div className="panel-head"><span className="t-label">Data Surveyors</span></div>
        <div className="gate-body">
          <h1 className="t-h2">Join the Data Surveyors on Motherlode</h1>
          <p className="t-body gate-text">
            Signed in as <strong>{session.me?.displayName}</strong>. Price reporting is open to Data Surveyors members,
            the players who fill in Motherlode's missing price data. Sign-up isn't open yet; once your account is
            enabled, this screen turns into the reporter.
          </p>
          <div className="gate-actions">
            <button type="button" className="btn btn--ghost" onClick={onSignOut}>Sign out</button>
            <button type="button" className="btn btn--secondary" onClick={() => openUrl(base)}>Open Motherlode</button>
            <button
              type="button"
              className="btn btn--primary"
              disabled={busy}
              onClick={async () => { setBusy(true); try { await onRefresh() } finally { setBusy(false) } }}
            >
              Check again
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
