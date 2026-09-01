use native_tls::TlsConnector;
use postgres::{Client, Config, config::SslMode};
use postgres_native_tls::MakeTlsConnector;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConnectError {
    #[error("invalid PostgreSQL connection configuration")]
    InvalidConfiguration(#[source] postgres::Error),
    #[error("PostgreSQL connection must explicitly set sslmode=require or sslmode=disable")]
    AmbiguousTlsMode,
    #[error("failed to initialize the PostgreSQL TLS connector")]
    TlsInitialization(#[source] native_tls::Error),
    #[error("failed to establish the PostgreSQL connection")]
    Connection(#[source] postgres::Error),
}

pub fn connect(conninfo: &str, password: Option<&str>) -> Result<Client, ConnectError> {
    let config = connection_config(conninfo, password)?;
    require_explicit_tls_mode(&config)?;
    let connector = TlsConnector::new().map_err(ConnectError::TlsInitialization)?;
    config
        .connect(MakeTlsConnector::new(connector))
        .map_err(ConnectError::Connection)
}

pub fn connection_config(conninfo: &str, password: Option<&str>) -> Result<Config, ConnectError> {
    let mut config = conninfo
        .parse::<Config>()
        .map_err(ConnectError::InvalidConfiguration)?;
    if let Some(password) = password {
        config.password(password);
    }
    Ok(config)
}

fn require_explicit_tls_mode(config: &Config) -> Result<(), ConnectError> {
    if config.get_ssl_mode() == SslMode::Prefer {
        Err(ConnectError::AmbiguousTlsMode)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use postgres::config::SslMode;

    use super::{ConnectError, connection_config, require_explicit_tls_mode};

    #[test]
    fn connection_requires_an_explicit_tls_mode() {
        assert!(matches!(
            require_explicit_tls_mode(
                &connection_config("host=localhost", None).expect("connection config")
            ),
            Err(ConnectError::AmbiguousTlsMode)
        ));
        let required = connection_config("host=localhost sslmode=require", None)
            .expect("explicit TLS mode should parse");
        assert_eq!(required.get_ssl_mode(), SslMode::Require);
        assert!(require_explicit_tls_mode(&required).is_ok());

        let disabled = connection_config("host=localhost sslmode=disable", None)
            .expect("explicit local plaintext mode should parse");
        assert_eq!(disabled.get_ssl_mode(), SslMode::Disable);
        assert!(require_explicit_tls_mode(&disabled).is_ok());
    }

    #[test]
    fn connection_configuration_applies_passwords_without_conninfo_quoting() {
        let special_password = br#"quote' and backslash\ password"#;
        let configured = connection_config(
            "host=localhost dbname=postgresem user=runtime sslmode=disable",
            Some(std::str::from_utf8(special_password).expect("ASCII password")),
        )
        .expect("passwordless conninfo parses");
        assert_eq!(configured.get_password(), Some(special_password.as_slice()));

        let embedded = connection_config(
            "host=localhost dbname=postgresem user=runtime password=embedded sslmode=disable",
            None,
        )
        .expect("complete connection info parses");
        assert_eq!(embedded.get_password(), Some(b"embedded".as_slice()));
    }
}
