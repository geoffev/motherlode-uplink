// Motherlode's public API. The key never leaves Rust: the UI
// asks the core to make calls, it doesn't see the key.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::model::{Commodity, CommodityTerminal, LineError, Me, TerminalReport};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// 401: the key is invalid or revoked. Stop sending until the player replaces it.
    #[error("{0}")]
    Unauthorized(String),
    /// 403: no `market-reports:write` scope, or not in the Survey Corps.
    #[error("{0}")]
    Forbidden(String),
    /// 429: try again after this many seconds.
    #[error("Rate limited, retrying in {0}s")]
    RateLimited(u64),
    /// Other 4xx: the request itself is wrong, retrying won't help. A 400 on a
    /// report says which lines were rejected.
    #[error("{message}")]
    Rejected { status: u16, message: String, lines: Vec<LineError> },
    /// 5xx: Motherlode's side; worth retrying.
    #[error("{message}")]
    Server { status: u16, message: String },
    /// No connection, DNS, timeout…
    #[error("Can't reach Motherlode: {0}")]
    Network(String),
    #[error("Unexpected response from Motherlode: {0}")]
    Decode(String),
}

impl ApiError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, ApiError::RateLimited(_) | ApiError::Server { .. } | ApiError::Network(_))
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
    #[serde(default)]
    lines: Vec<LineError>,
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    key: String,
}

impl Client {
    pub fn new(base: &str, key: &str) -> Client {
        let http = reqwest::Client::builder()
            .user_agent(concat!("MotherlodePriceReporter/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("HTTP client");
        Client { http, base: base.trim_end_matches('/').to_string(), key: key.to_string() }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/public/v1{}", self.base, path)
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let res = self.http.get(self.url(path)).bearer_auth(&self.key).send().await;
        Self::decode(res).await
    }

    async fn decode<T: DeserializeOwned>(res: reqwest::Result<reqwest::Response>) -> Result<T, ApiError> {
        let res = res.map_err(|e| ApiError::Network(e.without_url().to_string()))?;
        let status = res.status();
        if status.is_success() {
            return res.json::<T>().await.map_err(|e| ApiError::Decode(e.without_url().to_string()));
        }
        let retry_after = res
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let text = res.text().await.unwrap_or_default();
        let (message, lines) = serde_json::from_str::<ErrorBody>(&text)
            .map(|b| (b.error, b.lines))
            .unwrap_or_else(|_| (format!("HTTP {}", status.as_u16()), vec![]));
        Err(match status.as_u16() {
            401 => ApiError::Unauthorized(message),
            403 => ApiError::Forbidden(message),
            429 => ApiError::RateLimited(retry_after.unwrap_or(60).max(1)),
            s if s >= 500 => ApiError::Server { status: s, message },
            s => ApiError::Rejected { status: s, message, lines },
        })
    }

    pub async fn me(&self) -> Result<Me, ApiError> {
        self.get("/me").await
    }

    pub async fn commodities(&self) -> Result<Vec<Commodity>, ApiError> {
        self.get("/commodities").await
    }

    pub async fn commodity_terminals(&self) -> Result<Vec<CommodityTerminal>, ApiError> {
        self.get("/commodity-terminals").await
    }

    /// Files everything read from one terminal screen; returns Motherlode's batch id.
    pub async fn post_terminal_report(&self, report: &TerminalReport) -> Result<String, ApiError> {
        let res = self.http.post(self.url("/terminal-reports")).bearer_auth(&self.key).json(report).send().await;
        Self::decode::<Created>(res).await.map(|c| c.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ReportLine;

    /// Runs against the mock server: `MOTHERLODE_MOCK_URL=http://localhost:3999 cargo test api::`.
    #[tokio::test]
    async fn round_trip_against_mock() {
        let Ok(base) = std::env::var("MOTHERLODE_MOCK_URL") else { return };
        let c = Client::new(&base, "mlk_demo_member");
        assert!(c.me().await.unwrap().can_report());
        assert!(c.commodities().await.unwrap().iter().any(|x| x.game_name.as_deref() == Some("Saldynium (Ore)")));
        assert!(!c.commodity_terminals().await.unwrap().is_empty());

        let line = |id: &str| ReportLine { commodity_id: id.into(), sell_price: Some(135876), ..Default::default() };
        let ok = TerminalReport { terminal_id: "commodity:admin@orison".into(), observed_at: chrono::Utc::now(), lines: vec![line("savrilium")] };
        assert!(c.post_terminal_report(&ok).await.unwrap().starts_with("rep_"));

        let bad = TerminalReport { lines: vec![line("savrilium"), line("nope")], ..ok };
        match c.post_terminal_report(&bad).await {
            Err(ApiError::Rejected { status: 400, lines, .. }) => assert_eq!(lines[0].index, 1),
            other => panic!("expected a 400 with line errors, got {other:?}"),
        }
        assert!(matches!(Client::new(&base, "mlk_wrong").me().await, Err(ApiError::Unauthorized(_))));
    }
}
