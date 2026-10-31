// Confirmed reports waiting to be sent. Persisted, so nothing is lost when the
// app closes or the network drops; reports are retried, never dropped.
//
// One item is one terminal report (a whole screen). Keys allow 60 requests a
// minute; we pace below that, and a 429 holds the whole queue for its
// Retry-After. A 401 pauses sending until the player replaces the key.
// Motherlode rejects reports observed more than 24 hours ago, so those fail
// with an explanation instead of retrying forever.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::api::ApiError;
use crate::model::{OutboxItem, ReportStatus};
use crate::state::AppState;
use crate::util::Guard;

/// 60/min is the limit; leave room for catalog and /me calls.
const MIN_INTERVAL: Duration = Duration::from_millis(1250);
/// Sent reports stay in the status list this long.
const KEEP_SENT_HOURS: i64 = 24;
/// Motherlode's limit on `observedAt` age, less a margin for the request itself.
const MAX_AGE: chrono::Duration = chrono::Duration::minutes(24 * 60 - 2);

pub struct Outbox {
    path: PathBuf,
    items: Vec<OutboxItem>,
    /// Set by a 401; cleared when a new key is stored.
    pub paused_for_auth: bool,
    /// Set by a 429: nothing is sent before this.
    hold_until: Option<DateTime<Utc>>,
}

impl Outbox {
    pub fn load(path: PathBuf) -> Outbox {
        let mut items: Vec<OutboxItem> = crate::util::load_json(&path).unwrap_or_default();
        // A send that was in flight when the app closed goes back in the queue.
        for it in &mut items {
            if it.status == ReportStatus::Sending {
                it.status = ReportStatus::Queued;
            }
        }
        Outbox { path, items, paused_for_auth: false, hold_until: None }
    }

    fn save(&self) {
        crate::util::save_json(&self.path, &self.items);
    }

    pub fn items(&self) -> Vec<OutboxItem> {
        let mut v = self.items.clone();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    pub fn push(&mut self, item: OutboxItem) {
        self.items.push(item);
        self.save();
    }

    pub fn retry(&mut self, id: &str) -> Option<OutboxItem> {
        let it = self.items.iter_mut().find(|i| i.id == id && i.status == ReportStatus::Failed)?;
        it.status = ReportStatus::Queued;
        it.next_attempt_at = Utc::now();
        it.last_error = None;
        it.line_errors.clear();
        let out = it.clone();
        self.save();
        Some(out)
    }

    fn prune(&mut self) {
        let cutoff = Utc::now() - chrono::Duration::hours(KEEP_SENT_HOURS);
        let before = self.items.len();
        self.items.retain(|i| i.status != ReportStatus::Sent || i.sent_at.is_none_or(|t| t > cutoff));
        if self.items.len() != before {
            self.save();
        }
    }

    /// Reports too old for Motherlode to accept: failed, with the reason.
    fn expire(&mut self) -> Vec<OutboxItem> {
        let cutoff = Utc::now() - MAX_AGE;
        let mut out = Vec::new();
        for it in &mut self.items {
            if it.status == ReportStatus::Queued && it.report.oldest_observation() < cutoff {
                it.status = ReportStatus::Failed;
                it.last_error = Some("Too old to send: Motherlode only accepts prices seen in the last 24 hours".into());
                out.push(it.clone());
            }
        }
        if !out.is_empty() {
            self.save();
        }
        out
    }

    /// The next item due, marked as sending; or how long to wait.
    fn take_due(&mut self) -> Result<OutboxItem, Option<Duration>> {
        if self.paused_for_auth {
            return Err(None);
        }
        let now = Utc::now();
        if let Some(h) = self.hold_until.filter(|h| *h > now) {
            return Err(Some((h - now).to_std().unwrap_or_default()));
        }
        let next = self
            .items
            .iter_mut()
            .filter(|i| i.status == ReportStatus::Queued)
            .min_by_key(|i| (i.next_attempt_at, i.created_at));
        match next {
            None => Err(None),
            Some(it) if it.next_attempt_at > now => Err(Some((it.next_attempt_at - now).to_std().unwrap_or_default())),
            Some(it) => {
                it.status = ReportStatus::Sending;
                it.attempts += 1;
                Ok(it.clone())
            }
        }
    }

    fn finish(&mut self, id: &str, result: &Result<String, ApiError>) -> Option<OutboxItem> {
        let now = Utc::now();
        let it = self.items.iter_mut().find(|i| i.id == id)?;
        match result {
            Ok(report_id) => {
                it.status = ReportStatus::Sent;
                it.report_id = Some(report_id.clone());
                it.sent_at = Some(now);
                it.last_error = None;
            }
            Err(ApiError::Unauthorized(msg)) => {
                it.status = ReportStatus::Queued;
                it.attempts -= 1;
                it.last_error = Some(format!("Waiting for a valid API key ({msg})"));
                self.paused_for_auth = true;
            }
            Err(ApiError::RateLimited(secs)) => {
                it.status = ReportStatus::Queued;
                it.attempts -= 1;
                it.next_attempt_at = now + chrono::Duration::seconds(*secs as i64);
                it.last_error = Some(format!("Rate limited, retrying in {secs}s"));
                self.hold_until = Some(it.next_attempt_at);
            }
            Err(e) if e.is_retryable() => {
                // 5s, 10s, 20s … capped at 5 minutes.
                let backoff = (5u64 << it.attempts.saturating_sub(1).min(6)).min(300);
                it.status = ReportStatus::Queued;
                it.next_attempt_at = now + chrono::Duration::seconds(backoff as i64);
                it.last_error = Some(e.to_string());
            }
            Err(ApiError::Rejected { message, lines, .. }) => {
                it.status = ReportStatus::Failed;
                it.last_error = Some(message.clone());
                it.line_errors = lines.clone();
            }
            Err(e) => {
                it.status = ReportStatus::Failed;
                it.last_error = Some(e.to_string());
            }
        }
        let out = it.clone();
        self.save();
        Some(out)
    }
}

/// Wakes the sender when something is queued or the key changes.
pub type Wake = Arc<Notify>;

/// Background sender. Runs for the life of the app.
pub async fn run(app: AppHandle, wake: Wake) {
    let mut last_prune = Utc::now() - chrono::Duration::hours(1);
    let mut cached: Option<((String, String), crate::api::Client)> = None;
    loop {
        let state = app.state::<AppState>();
        if Utc::now() - last_prune > chrono::Duration::minutes(10) {
            state.outbox.guard().prune();
            // Readings nobody reviewed for a day can't be sent any more either.
            let stale = state.readings.guard().prune_older_than(Utc::now() - MAX_AGE);
            let crops = state.crops();
            for r in &stale {
                crops.discard(r);
                let _ = app.emit("reading-removed", &r.id);
            }
            if !stale.is_empty() {
                log::info!("removed {} readings older than 24 hours from the queue", stale.len());
            }
            last_prune = Utc::now();
        }
        let expired = state.outbox.guard().expire();
        for it in &expired {
            let _ = app.emit("outbox", it);
        }
        // One client (and so one connection pool) for as long as the server and
        // key stay the same.
        let client = match state.api_target() {
            None => None,
            Some(target) => {
                if cached.as_ref().is_none_or(|(t, _)| *t != target) {
                    cached = Some((target.clone(), crate::api::Client::new(&target.0, &target.1)));
                }
                cached.as_ref().map(|(_, c)| c.clone())
            }
        };
        let due = match client {
            None => Err(None),
            Some(_) => state.outbox.guard().take_due(),
        };
        let item = match due {
            Ok(item) => item,
            Err(wait) => {
                let wait = wait.unwrap_or(Duration::from_secs(300)).max(Duration::from_millis(200));
                let _ = tokio::time::timeout(wait, wake.notified()).await;
                continue;
            }
        };
        let _ = app.emit("outbox", &item);

        let result = client.unwrap().post_terminal_report(&item.report).await;
        if let Err(e) = &result {
            log::warn!("report {} ({}): {e}", item.id, item.terminal_label);
        }
        let updated = state.outbox.guard().finish(&item.id, &result);
        if let Some(u) = updated {
            let _ = app.emit("outbox", &u);
        }
        if matches!(result, Err(ApiError::Unauthorized(_))) {
            let _ = app.emit("auth-required", ());
        }
        tokio::time::sleep(MIN_INTERVAL).await;
    }
}
