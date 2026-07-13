use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsOperation {
    Account,
    Match,
    Participants,
    Settlement,
    Warehouse,
    Ledger,
    UnknownRoute,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsAuditOutcome {
    Success,
    Unauthorized,
    RateLimited,
    InvalidRequest,
    NotFound,
    Unavailable,
    MethodRejected,
}

/// 运维审计刻意不携带 subject ID、钱包、凭据、查询参数或 SQL。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpsAuditEvent {
    pub request_id: Uuid,
    pub recorded_at: OffsetDateTime,
    pub operation: OpsOperation,
    pub outcome: OpsAuditOutcome,
    pub method: String,
    pub status: u16,
    pub returned_count: Option<usize>,
    pub duration_ms: u64,
}

pub trait OpsAuditSink: Send + Sync {
    fn record(&self, event: OpsAuditEvent);
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StderrAuditSink;

impl OpsAuditSink for StderrAuditSink {
    fn record(&self, event: OpsAuditEvent) {
        match serde_json::to_string(&event) {
            Ok(encoded) => eprintln!("{encoded}"),
            Err(_) => eprintln!(r#"{{"operation":"ops_audit","outcome":"encode_failed"}}"#),
        }
    }
}
