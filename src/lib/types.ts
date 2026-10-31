// Mirrors src-tauri/src/model.rs (serialised camelCase).

export type Side = 'buy' | 'sell'

export const CARGO_SIZES = [1, 2, 4, 8, 16, 24, 32] as const

export const MARKET_REPORTS_WRITE = 'market-reports:write'

export interface NormRect { x: number; y: number; w: number; h: number }

export interface Field<T> {
  value: T | null
  text: string
  confidence: number
  crop: string | null
  rect: NormRect | null
}

export interface Reading {
  id: string
  screenId: string
  createdAt: string
  updatedAt: string
  revision: number
  /** sell = IN DEMAND (price only), buy = IN STOCK */
  side: Side
  terminalId: string | null
  terminalSource: 'screen' | 'last' | null
  /** The location as read under YOUR INVENTORIES. */
  locationText: string | null
  commodity: Field<string>
  /** Several commodities share the name read; the player picks one. */
  commodityCandidates: string[]
  price: Field<number>
  /** Buy side only. */
  quantity: Field<number>
  sizes: Field<number[]>
  frame: string | null
  frameWidth: number
  frameHeight: number
}

export interface Confirmation {
  readingId: string
  terminalId: string
  commodityId: string
  side: Side
  price: number | null
  quantity: number | null
  sizes: number[]
}

export type ReportStatus = 'queued' | 'sending' | 'sent' | 'failed'

export interface ReportLine {
  commodityId: string
  sellPrice?: number
  demandScu?: number
  sellCargoSizes?: number[]
  buyPrice?: number
  stockScu?: number
  buyCargoSizes?: number[]
}

export interface TerminalReport { terminalId: string; observedAt: string; lines: ReportLine[] }

export interface OutboxItem {
  id: string
  readingIds: string[]
  terminalLabel: string
  /** Commodity names, in the order of report.lines. */
  lineLabels: string[]
  report: TerminalReport
  status: ReportStatus
  attempts: number
  lastError: string | null
  lineErrors: { index: number; error: string }[]
  nextAttemptAt: string
  createdAt: string
  sentAt: string | null
  reportId: string | null
}

export interface Me { id: string; displayName: string; surveyCorps: boolean; scopes: string[] }

export const canReport = (me: Me) => me.scopes.includes(MARKET_REPORTS_WRITE)

export interface Session {
  signedIn: boolean
  maskedKey: string | null
  me: Me | null
  keyRejected: boolean
}

export interface Commodity { id: string; name: string; displayName: string | null; gameName: string | null }

export interface CommodityTerminal {
  id: string
  name: string
  location: string
  place: string | null
  system: string
}

export interface Catalog {
  commodities: Commodity[]
  terminals: CommodityTerminal[]
  fetchedAt: string | null
}

export interface Settings {
  apiBase: string
  captureEnabled: boolean
  captureFps: number
  startWithComputer: boolean
  cropsDir: string | null
  cropRetentionDays: number
  lastTerminalId: string | null
  tesseractPath: string | null
}

/** noGame: Star Citizen isn't running (or is minimised), so nothing is recorded. */
export type CaptureState = 'paused' | 'idle' | 'noGame' | 'watching' | 'reading' | 'terminal' | 'error'

export interface CaptureStatus { state: CaptureState; message: string | null; source: string }

export interface ScanOutcome {
  terminalFound: boolean
  rows: number
  newReadings: number
  updatedReadings: number
  duplicates: number
}

export interface AppInfo {
  version: string
  dataDir: string
  cropsDir: string
  ocrEngine: string
  ocrError: string | null
  autostart: boolean
  /** Release builds: the Motherlode server is fixed. */
  serverLocked: boolean
}

export const commodityLabel = (c: Commodity) => c.displayName || c.name
export const terminalLabel = (t: CommodityTerminal) => `${t.location} - ${t.name}`
