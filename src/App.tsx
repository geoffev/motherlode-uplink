import { useEffect, useState } from 'react'
import mark from './assets/mark.svg'
import { errorText, ipc } from './lib/ipc'
import { useAppData } from './lib/useAppData'
import { canReport, type CaptureStatus } from './lib/types'
import { FaqView } from './components/FaqView'
import { OutboxSummary, OutboxView } from './components/OutboxView'
import { ReviewView } from './components/ReviewView'
import { SettingsView } from './components/SettingsView'
import { NotEnrolled, SignIn } from './components/SignIn'
import { OcrSetupBanner } from './components/OcrSetup'

type Tab = 'queue' | 'sent' | 'settings' | 'faq'

const STATUS_TEXT: Record<CaptureStatus['state'], string> = {
  paused: 'Paused',
  idle: 'Idle',
  noGame: 'Not recording',
  watching: 'Watching',
  reading: 'Reading terminal…',
  terminal: 'Terminal in view',
  error: 'Capture problem',
}

export default function App() {
  const data = useAppData()
  const { session, settings, status } = data
  // A #sent / #settings / #faq link opens on that tab.
  const [tab, setTab] = useState<Tab>(() => {
    const h = window.location.hash.slice(1)
    return h === 'sent' || h === 'settings' || h === 'faq' ? h : 'queue'
  })
  const [replacingKey, setReplacingKey] = useState(false)
  /** An action that failed (sign out, catalog refresh), shown until dismissed. */
  const [notice, setNotice] = useState<string | null>(null)
  const [ocrMissing, setOcrMissing] = useState(false)

  // Re-checked whenever the Tesseract path changes.
  useEffect(() => {
    ipc.getAppInfo().then(i => setOcrMissing(!!i.ocrError))
  }, [settings?.tesseractPath])

  if (!session) return <div className="boot" />

  const signOut = async () => {
    try { data.setSession(await ipc.signOut()) } catch (e) { setNotice(errorText(e)) }
  }

  const noticeBar = notice && (
    <div className="banner banner--error" role="alert">
      {notice}
      <span className="spacer" />
      <button type="button" className="btn btn--ghost btn--sm" onClick={() => setNotice(null)}>Dismiss</button>
    </div>
  )

  if (!session.signedIn || replacingKey || session.keyRejected) {
    return (
      <SignIn
        settings={settings}
        rejected={session.keyRejected && !replacingKey}
        onSignedIn={s => { data.setSession(s); setReplacingKey(false) }}
      />
    )
  }
  if (session.me && !canReport(session.me)) {
    return (
      <>
        {noticeBar}
        <NotEnrolled
          session={session}
          settings={settings}
          onSignOut={signOut}
          onRefresh={async () => { try { data.setSession(await ipc.refreshSession()) } catch { /* shown on next try */ } }}
        />
      </>
    )
  }

  const tabs: { id: Tab; label: string; count?: number }[] = [
    { id: 'queue', label: 'Review', count: data.readings.length },
    { id: 'sent', label: 'Sent' },
    { id: 'settings', label: 'Settings' },
    { id: 'faq', label: 'FAQ' },
  ]
  const paused = status?.state === 'paused'

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <img className="brand-mark" src={mark} alt="" aria-hidden="true" />
          <span className="brand-name">Motherlode</span>
          <span className="t-label">UPLINK</span>
        </div>
        <nav className="tabs" aria-label="Sections">
          {tabs.map(t => (
            <button key={t.id} type="button" className="tab" aria-current={tab === t.id ? 'page' : undefined} onClick={() => setTab(t.id)}>
              {t.label}
              {!!t.count && <span className="tab-count t-data-sm">{t.count}</span>}
            </button>
          ))}
        </nav>
        <div className="topbar-right">
          <OutboxSummary items={data.outbox} />
          {status && (
            <span className={`capture capture--${status.state}`} title={[status.source, status.message].filter(Boolean).join(' · ')}>
              <span className="capture-dot" aria-hidden="true" />
              <span className="t-label">{STATUS_TEXT[status.state]}</span>
            </span>
          )}
          <button type="button" className="btn btn--secondary btn--sm" onClick={() => ipc.setCaptureEnabled(paused).then(data.setSettings)}>
            {paused ? 'Resume' : 'Pause'}
          </button>
          <span className="t-caption who">{session.me?.displayName ?? 'Offline'}</span>
        </div>
      </header>

      {status?.state === 'noGame' && (
        <div className="banner" role="status">{status.message}</div>
      )}
      {noticeBar}
      {status?.state === 'error' && status.message && (
        <div className="banner banner--error" role="status">{status.message}</div>
      )}
      {ocrMissing && settings && <OcrSetupBanner settings={settings} onSaved={data.setSettings} />}
      {!data.catalog.commodities.length && (
        <div className="banner" role="status">
          Game data hasn't loaded yet, so commodities and terminals can't be matched.
          <button type="button" className="btn btn--ghost btn--sm" onClick={() => ipc.refreshCatalog().then(() => setNotice(null), e => setNotice(errorText(e)))}>Retry</button>
        </div>
      )}

      <main className="main">
        {/* Stays mounted so unsent edits survive a trip to another tab. */}
        <div className="main-view" hidden={tab !== 'queue'}>
          <ReviewView readings={data.readings} catalog={data.catalog} settings={settings} status={status} active={tab === 'queue'} />
        </div>
        {tab === 'sent' && <OutboxView items={data.outbox} />}
        {tab === 'settings' && settings && (
          <SettingsView
            settings={settings}
            session={session}
            catalog={data.catalog}
            onSaved={data.setSettings}
            onSignOut={signOut}
            onReplaceKey={() => setReplacingKey(true)}
          />
        )}
        {tab === 'faq' && <FaqView />}
      </main>
    </div>
  )
}
