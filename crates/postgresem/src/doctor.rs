use std::process::{Command, Output};

use serde::Serialize;
use thiserror::Error;

const DOCTOR_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub schema_version: String,
    pub postgresem_version: String,
    pub operating_system: &'static str,
    pub architecture: &'static str,
    pub apple_container: RuntimeReport,
    pub docker: RuntimeReport,
    pub usable_container_runtime: bool,
}

#[derive(Debug, Serialize)]
pub struct RuntimeReport {
    pub available: bool,
    pub engine_version: Option<String>,
    pub compose_version: Option<String>,
}

#[derive(Debug, Error)]
pub enum DoctorError {
    #[error("neither Apple Container with container-compose nor Docker with Compose is available")]
    NoContainerRuntime,
}

pub fn inspect() -> DoctorReport {
    let apple_container = runtime_report(
        command_version("container", &["--version"]),
        command_version("container-compose", &["--version"]),
    );
    let docker = runtime_report(
        command_version("docker", &["--version"]),
        command_version("docker", &["compose", "version"]),
    );
    DoctorReport {
        schema_version: DOCTOR_SCHEMA_VERSION.to_owned(),
        postgresem_version: env!("CARGO_PKG_VERSION").to_owned(),
        operating_system: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        usable_container_runtime: apple_container.available || docker.available,
        apple_container,
        docker,
    }
}

pub fn require_runtime(report: &DoctorReport) -> Result<(), DoctorError> {
    if report.usable_container_runtime {
        Ok(())
    } else {
        Err(DoctorError::NoContainerRuntime)
    }
}

fn runtime_report(
    engine_version: Option<String>,
    compose_version: Option<String>,
) -> RuntimeReport {
    RuntimeReport {
        available: engine_version.is_some() && compose_version.is_some(),
        engine_version,
        compose_version,
    }
}

fn command_version(program: &str, arguments: &[&str]) -> Option<String> {
    match Command::new(program).args(arguments).output() {
        Ok(output) if output.status.success() => output_text(&output),
        Ok(_) | Err(_) => None,
    }
}

fn output_text(output: &Output) -> Option<String> {
    let bytes = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let text = String::from_utf8_lossy(bytes).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::{DoctorReport, RuntimeReport, require_runtime};

    #[test]
    fn runtime_is_required_but_either_supported_stack_is_accepted() {
        let mut report = DoctorReport {
            schema_version: "1".to_owned(),
            postgresem_version: "0.1.0".to_owned(),
            operating_system: "linux",
            architecture: "x86_64",
            apple_container: RuntimeReport {
                available: false,
                engine_version: None,
                compose_version: None,
            },
            docker: RuntimeReport {
                available: false,
                engine_version: None,
                compose_version: None,
            },
            usable_container_runtime: false,
        };
        assert!(require_runtime(&report).is_err());
        report.docker.available = true;
        report.usable_container_runtime = true;
        assert!(require_runtime(&report).is_ok());
    }
}
