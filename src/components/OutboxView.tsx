import { useState } from 'react'
import { errorText, ipc } from '../lib/ipc'
import { fmtPrice, fmtScu, fmtTime } from '../lib/format'
import type { OutboxItem, ReportLine, ReportStatus } from '../lib/types'
import { SizeSquares } from './Fields'

const STATUS_LABEL: Record<ReportStatus, string> = { queued: 'Queued', sending: 'Sending', sent: 'Sent', failed: 'Failed' }

export function OutboxSummary({ items }: { items: OutboxItem[] }) {
  const queued = items.filter(i => i.status === 'queued' || i.status === 'sending').length
  const failed = items.filter(i => i.status === 'failed').length
  if (!queued && !failed) return null
  return (
    <span className="t-data-sm outbox-summary">
      {queued > 0 && <span className="st st--queued">{queued} queued</span>}
      {failed > 0 && <span className="st st--failed">{failed} failed</span>}
    </span>
  )
}

function Line({ line, label, error }: { line: ReportLine; label: string; error?: string }) {
  return (
    <li className="report-line">
      <span className="report-line-name">{label || line.commodityId}</span>
      {(line.sellPrice != null || line.sellCargoSizes) && (
        <span className="report-side">
          <span className="tag tag--sell">sell</span>
          {line.sellPrice != null && <span className="t-data-sm">¤{fmtPrice(line.sellPrice)}</span>}
          {line.sellCargoSizes && <SizeSquares sizes={line.sellCargoSizes} />}
        </span>
      )}
      {(line.buyPrice != null || line.stockScu != null || line.buyCargoSizes) && (
        <span className="report-side">
          <span className="tag tag--buy">buy</span>
          {line.buyPrice != null && <span className="t-data-sm">¤{fmtPrice(line.buyPrice)}</span>}
          {line.stockScu != null && <span className="t-data-sm">{fmtScu(line.stockScu)} SCU</span>}
          {line.buyCargoSizes && <SizeSquares sizes={line.buyCargoSizes} />}
        </span>
      )}
      {error && <span className="report-line-err">{error}</span>}
    </li>
  )
}

export function OutboxView({ items }: { items: OutboxItem[] }) {
  const [retryErrors, setRetryErrors] = useState<Record<string, string>>({})
  const retry = (id: string) =>
    ipc.retryReport(id).then(
      () => setRetryErrors(({ [id]: _, ...rest }) => rest),
      e => setRetryErrors(r => ({ ...r, [id]: errorText(e) })),
    )
  if (!items.length) {
    return (
      <div className="empty panel">
        <div className="empty-inner">
          <h2 className="t-h3">Nothing sent yet</h2>
          <p className="t-body empty-text">Confirmed readings appear here with their send status. Reports wait here while you're offline and go out once the connection is back.</p>
        </div>
      </div>
    )
  }
  return (
    <section className="panel outbox">
      <div className="panel-head"><span className="t-label">Reports · last 24 hours</span></div>
      <ul className="report-list">
        {items.map(i => {
          const errors = new Map(i.lineErrors.map(e => [e.index, e.error]))
          return (
            <li key={i.id} className="report">
              <div className="report-head">
                <span className="t-data-sm">{fmtTime(i.report.observedAt)}</span>
                <span className="report-terminal">{i.terminalLabel}</span>
                <span className="t-caption">{i.report.lines.length} {i.report.lines.length === 1 ? 'line' : 'lines'}</span>
                <span className="spacer" />
                <span className={`st st--${i.status}`}>{STATUS_LABEL[i.status]}</span>
                {i.status === 'failed' && (
                  <button type="button" className="btn btn--ghost btn--sm" onClick={() => retry(i.id)}>
                    Retry
                  </button>
                )}
              </div>
              {i.lastError && <p className="t-caption report-err">{i.lastError}</p>}
              {retryErrors[i.id] && <p className="t-caption report-err" role="alert">Retry failed: {retryErrors[i.id]}</p>}
              <ul className="report-lines">
                {i.report.lines.map((l, n) => <Line key={l.commodityId} line={l} label={i.lineLabels[n]} error={errors.get(n)} />)}
              </ul>
            </li>
          )
        })}
      </ul>
    </section>
  )
}
