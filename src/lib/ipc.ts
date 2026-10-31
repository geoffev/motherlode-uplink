import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AppInfo, CaptureStatus, Catalog, Confirmation, OutboxItem, Reading, ScanOutcome, Session, Settings,
} from './types'

export const ipc = {
  getSession: () => invoke<Session>('get_session'),
  signIn: (key: string) => invoke<Session>('sign_in', { key }),
  signOut: () => invoke<Session>('sign_out'),
  refreshSession: () => invoke<Session>('refresh_session'),

  getCatalog: () => invoke<Catalog>('get_catalog'),
  refreshCatalog: () => invoke<Catalog>('refresh_catalog'),

  listReadings: () => invoke<Reading[]>('list_readings'),
  confirmReadings: (items: Confirmation[]) => invoke<OutboxItem[]>('confirm_readings', { items }),
  rejectReading: (id: string) => invoke<void>('reject_reading', { id }),

  listOutbox: () => invoke<OutboxItem[]>('list_outbox'),
  retryReport: (id: string) => invoke<OutboxItem>('retry_report', { id }),

  getCaptureStatus: () => invoke<CaptureStatus>('get_capture_status'),
  setCaptureEnabled: (enabled: boolean) => invoke<Settings>('set_capture_enabled', { enabled }),
  scanImage: (path: string) => invoke<ScanOutcome>('scan_image', { path }),

  getAppInfo: () => invoke<AppInfo>('get_app_info'),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<Settings>('save_settings', { settings }),
}

export interface Events {
  reading: Reading
  'reading-removed': string
  outbox: OutboxItem
  'capture-status': CaptureStatus
  session: Session
  catalog: Catalog
  settings: Settings
  'auth-required': null
}

export function on<K extends keyof Events>(name: K, fn: (payload: Events[K]) => void): Promise<UnlistenFn> {
  return listen<Events[K]>(name, e => fn(e.payload))
}

export const fileUrl = (path: string | null) => (path ? convertFileSrc(path) : undefined)

/** Commands reject with the Rust error string. */
export const errorText = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e))
