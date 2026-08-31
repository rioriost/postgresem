use std::env;

use postgres::{Config, NoTls};
use serde_json::Value;
use thiserror::Error;

const MAX_WINDOW_HOURS: u32 = 24 * 365;

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("report window must be between 1 and {MAX_WINDOW_HOURS} hours")]
    InvalidWindow,
    #[error("environment variable name is invalid: {0}")]
    InvalidEnvironmentVariableName(String),
    #[error("required environment variable {0} is not set")]
    MissingEnvironmentVariable(String),
    #[error("required environment variable {0} is not valid Unicode")]
    InvalidEnvironmentVariable(String),
    #[error("failed to connect using audit URL environment variable {0}")]
    AuditConnect(String),
    #[error("beta operational report is unavailable; apply current migrations")]
    ReportUnavailable,
    #[error("beta operational report returned an invalid JSON value")]
    InvalidReport,
}

pub fn beta(
    audit_database_url_variable: &str,
    audit_password_variable: &str,
    window_hours: u32,
) -> Result<Value, ReportError> {
    if !(1..=MAX_WINDOW_HOURS).contains(&window_hours) {
        return Err(ReportError::InvalidWindow);
    }
    for variable in [audit_database_url_variable, audit_password_variable] {
        if !valid_environment_variable_name(variable) {
            return Err(ReportError::InvalidEnvironmentVariableName(
                variable.to_owned(),
            ));
        }
    }
    let conninfo = required_environment(audit_database_url_variable)?;
    let password = required_environment(audit_password_variable)?;
    let mut config = conninfo
        .parse::<Config>()
        .map_err(|_| ReportError::AuditConnect(audit_database_url_variable.to_owned()))?;
    config.password(password);
    let mut client = config
        .connect(NoTls)
        .map_err(|_| ReportError::AuditConnect(audit_database_url_variable.to_owned()))?;
    let window_hours = i32::try_from(window_hours).map_err(|_| ReportError::InvalidWindow)?;
    let row = client
        .query_one(
            "
            SELECT semantic.beta_operational_report(
              clock_timestamp() - make_interval(hours => $1)
            )
            ",
            &[&window_hours],
        )
        .map_err(|_| ReportError::ReportUnavailable)?;
    row.try_get(0).map_err(|_| ReportError::InvalidReport)
}

fn required_environment(variable: &str) -> Result<String, ReportError> {
    env::var(variable).map_err(|error| match error {
        env::VarError::NotPresent => ReportError::MissingEnvironmentVariable(variable.to_owned()),
        env::VarError::NotUnicode(_) => {
            ReportError::InvalidEnvironmentVariable(variable.to_owned())
        }
    })
}

fn valid_environment_variable_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::{MAX_WINDOW_HOURS, ReportError, beta};

    #[test]
    fn beta_report_rejects_invalid_windows_before_reading_environment() {
        assert!(matches!(
            beta("DATABASE_URL", "PASSWORD", 0),
            Err(ReportError::InvalidWindow)
        ));
        assert!(matches!(
            beta("DATABASE_URL", "PASSWORD", MAX_WINDOW_HOURS + 1),
            Err(ReportError::InvalidWindow)
        ));
    }

    #[test]
    fn beta_report_rejects_invalid_environment_names() {
        assert!(matches!(
            beta("INVALID-NAME", "PASSWORD", 24),
            Err(ReportError::InvalidEnvironmentVariableName(variable))
                if variable == "INVALID-NAME"
        ));
    }
}
