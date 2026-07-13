use std::{env, io};

const CRASH_POINT_ENV: &str = "EXTRACTION_E2E_SETTLEMENT_CRASH";
const CRASH_EXIT_CODE: i32 = 86;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SettlementCrashPoint {
    BeforeCommit,
    AfterCommit,
}

impl SettlementCrashPoint {
    const fn as_str(self) -> &'static str {
        match self {
            Self::BeforeCommit => "before_commit",
            Self::AfterCommit => "after_commit",
        }
    }
}

pub(crate) fn validate_configuration() -> io::Result<()> {
    configured_point().map(|_| ()).map_err(|message| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid {CRASH_POINT_ENV}: {message}"),
        )
    })
}

pub(super) fn exit_if_configured(point: SettlementCrashPoint) {
    if configured_point().ok().flatten() != Some(point) {
        return;
    }
    eprintln!(
        "{{\"event\":\"e2e_settlement_crash\",\"point\":\"{}\"}}",
        point.as_str()
    );
    // exit 不运行析构，数据库连接会被操作系统关闭；这才会真实验证未提交事务回滚。
    std::process::exit(CRASH_EXIT_CODE);
}

fn configured_point() -> Result<Option<SettlementCrashPoint>, &'static str> {
    match env::var(CRASH_POINT_ENV) {
        Ok(value) => parse_point(Some(&value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err("value must be UTF-8"),
    }
}

fn parse_point(value: Option<&str>) -> Result<Option<SettlementCrashPoint>, &'static str> {
    match value {
        None | Some("") => Ok(None),
        Some("before_commit") => Ok(Some(SettlementCrashPoint::BeforeCommit)),
        Some("after_commit") => Ok(Some(SettlementCrashPoint::AfterCommit)),
        Some(_) => Err("expected before_commit or after_commit"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_named_transaction_boundaries() {
        assert_eq!(parse_point(None), Ok(None));
        assert_eq!(parse_point(Some("")), Ok(None));
        assert_eq!(
            parse_point(Some("before_commit")),
            Ok(Some(SettlementCrashPoint::BeforeCommit))
        );
        assert_eq!(
            parse_point(Some("after_commit")),
            Ok(Some(SettlementCrashPoint::AfterCommit))
        );
        assert!(parse_point(Some("commit")).is_err());
    }
}
