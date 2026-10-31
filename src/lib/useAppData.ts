import { useCallback, useEffect, useState } from 'react'
import { ipc, on } from './ipc'
import type { CaptureStatus, Catalog, OutboxItem, Reading, Session, Settings } from './types'

const byNewest = <T extends { createdAt: string }>(a: T, b: T) => b.createdAt.localeCompare(a.createdAt)

function upsert<T extends { id: string; createdAt: string }>(list: T[], item: T): T[] {
  const i = list.findIndex(x => x.id === item.id)
  const next = i < 0 ? [...list, item] : list.map(x => (x.id === item.id ? item : x))
  return next.sort(byNewest)
}

export function useAppData() {
  const [session, setSession] = useState<Session | null>(null)
  const [catalog, setCatalog] = useState<Catalog>({ commodities: [], terminals: [], fetchedAt: null })
  const [readings, setReadings] = useState<Reading[]>([])
  const [outbox, setOutbox] = useState<OutboxItem[]>([])
  const [status, setStatus] = useState<CaptureStatus | null>(null)
  const [settings, setSettings] = useState<Settings | null>(null)

  useEffect(() => {
    const subs = [
      on('reading', r => setReadings(l => upsert(l, r))),
      on('reading-removed', id => setReadings(l => l.filter(r => r.id !== id))),
      on('outbox', o => setOutbox(l => upsert(l, o))),
      on('capture-status', setStatus),
      on('session', setSession),
      on('catalog', setCatalog),
      on('settings', setSettings),
      on('auth-required', () => ipc.getSession().then(setSession)),
    ]
    ipc.getSession().then(setSession)
    ipc.getCatalog().then(setCatalog)
    ipc.listReadings().then(setReadings)
    ipc.listOutbox().then(setOutbox)
    ipc.getCaptureStatus().then(setStatus)
    ipc.getSettings().then(setSettings)
    return () => { subs.forEach(p => p.then(un => un())) }
  }, [])

  const reloadOutbox = useCallback(() => ipc.listOutbox().then(setOutbox), [])

  return { session, setSession, catalog, setCatalog, readings, outbox, reloadOutbox, status, settings, setSettings }
}
