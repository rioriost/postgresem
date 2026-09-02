use native_tls::TlsConnector;
use postgres::{CancelToken, Client, Config, config::SslMode};
use postgres_native_tls::MakeTlsConnector;
use std::time::Duration;
use thiserror::Error;

const MAX_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TCP_USER_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_KEEPALIVE_IDLE: Duration = Duration::from_secs(10);
const MAX_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(3);
const MAX_KEEPALIVE_RETRIES: u32 = 3;

#[derive(Debug, Error)]
pub enum ConnectError {
    #[error("invalid PostgreSQL connection configuration")]
    InvalidConfiguration(#[source] postgres::Error),
    #[error("PostgreSQL connection must explicitly set sslmode=require or sslmode=disable")]
    AmbiguousTlsMode,
    #[error("PostgreSQL connect_timeout must be between 1 and 10 seconds")]
    InvalidConnectTimeout,
    #[error("PostgreSQL tcp_user_timeout must not exceed 10 seconds")]
    InvalidTcpUserTimeout,
    #[error("PostgreSQL TCP keepalives must remain enabled")]
    DisabledKeepalives,
    #[error("failed to initialize the PostgreSQL TLS connector")]
    TlsInitialization(#[source] native_tls::Error),
    #[error("failed to establish the PostgreSQL connection")]
    Connection(#[source] postgres::Error),
}

#[derive(Clone)]
pub struct CancelHandle {
    token: CancelToken,
}

impl CancelHandle {
    pub fn cancel(&self) -> Result<(), ConnectError> {
        let connector = TlsConnector::new().map_err(ConnectError::TlsInitialization)?;
        self.token
            .cancel_query(MakeTlsConnector::new(connector))
            .map_err(ConnectError::Connection)
    }
}

pub fn connect(conninfo: &str, password: Option<&str>) -> Result<Client, ConnectError> {
    let mut config = connection_config(conninfo, password)?;
    require_explicit_tls_mode(&config)?;
    require_bounded_connect_timeout(&mut config)?;
    require_bounded_socket_liveness(&mut config)?;
    let connector = TlsConnector::new().map_err(ConnectError::TlsInitialization)?;
    config
        .connect(MakeTlsConnector::new(connector))
        .map_err(ConnectError::Connection)
}

pub fn cancel_handle(client: &Client) -> CancelHandle {
    CancelHandle {
        token: client.cancel_token(),
    }
}

pub fn configure_session_timeouts(
    client: &mut Client,
    statement_timeout: Duration,
    lock_timeout: Duration,
    idle_transaction_timeout: Duration,
) -> Result<(), postgres::Error> {
    for (name, value) in [
        ("statement_timeout", statement_timeout),
        ("lock_timeout", lock_timeout),
        (
            "idle_in_transaction_session_timeout",
            idle_transaction_timeout,
        ),
    ] {
        let milliseconds = format!("{}ms", value.as_millis());
        client.query_one(
            "SELECT pg_catalog.set_config($1, $2, false)",
            &[&name, &milliseconds],
        )?;
    }
    Ok(())
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

fn require_bounded_connect_timeout(config: &mut Config) -> Result<(), ConnectError> {
    match config.get_connect_timeout().copied() {
        None => {
            config.connect_timeout(MAX_CONNECT_TIMEOUT);
            Ok(())
        }
        Some(timeout) if !timeout.is_zero() && timeout <= MAX_CONNECT_TIMEOUT => Ok(()),
        Some(_) => Err(ConnectError::InvalidConnectTimeout),
    }
}

fn require_bounded_socket_liveness(config: &mut Config) -> Result<(), ConnectError> {
    match config.get_tcp_user_timeout().copied() {
        None => {
            config.tcp_user_timeout(MAX_TCP_USER_TIMEOUT);
        }
        Some(timeout) if !timeout.is_zero() && timeout <= MAX_TCP_USER_TIMEOUT => {}
        Some(_) => return Err(ConnectError::InvalidTcpUserTimeout),
    }
    if !config.get_keepalives() {
        return Err(ConnectError::DisabledKeepalives);
    }
    if config.get_keepalives_idle() > MAX_KEEPALIVE_IDLE {
        config.keepalives_idle(MAX_KEEPALIVE_IDLE);
    }
    if config
        .get_keepalives_interval()
        .is_none_or(|interval| interval > MAX_KEEPALIVE_INTERVAL)
    {
        config.keepalives_interval(MAX_KEEPALIVE_INTERVAL);
    }
    if config
        .get_keepalives_retries()
        .is_none_or(|retries| retries > MAX_KEEPALIVE_RETRIES)
    {
        config.keepalives_retries(MAX_KEEPALIVE_RETRIES);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use postgres::config::SslMode;

    use super::{
        ConnectError, connection_config, require_bounded_connect_timeout,
        require_bounded_socket_liveness, require_explicit_tls_mode,
    };

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

    #[test]
    fn connection_configuration_requires_a_bounded_connect_timeout() {
        let mut defaulted =
            connection_config("host=localhost sslmode=disable", None).expect("connection config");
        assert!(require_bounded_connect_timeout(&mut defaulted).is_ok());
        assert_eq!(
            defaulted.get_connect_timeout(),
            Some(&Duration::from_secs(10))
        );

        let mut stricter =
            connection_config("host=localhost sslmode=disable connect_timeout=3", None)
                .expect("bounded connection config");
        assert!(require_bounded_connect_timeout(&mut stricter).is_ok());
        assert_eq!(
            stricter.get_connect_timeout(),
            Some(&Duration::from_secs(3))
        );

        let mut unlimited =
            connection_config("host=localhost sslmode=disable connect_timeout=0", None)
                .expect("connection config");
        assert!(require_bounded_connect_timeout(&mut unlimited).is_ok());
        assert_eq!(
            unlimited.get_connect_timeout(),
            Some(&Duration::from_secs(10))
        );

        let mut excessive =
            connection_config("host=localhost sslmode=disable connect_timeout=11", None)
                .expect("connection config");
        assert!(matches!(
            require_bounded_connect_timeout(&mut excessive),
            Err(ConnectError::InvalidConnectTimeout)
        ));
    }

    #[test]
    fn connection_configuration_requires_bounded_socket_liveness() {
        let mut defaulted =
            connection_config("host=localhost sslmode=disable", None).expect("connection config");
        assert!(require_bounded_socket_liveness(&mut defaulted).is_ok());
        assert_eq!(
            defaulted.get_tcp_user_timeout(),
            Some(&Duration::from_secs(10))
        );
        assert_eq!(defaulted.get_keepalives_idle(), Duration::from_secs(10));
        assert_eq!(
            defaulted.get_keepalives_interval(),
            Some(Duration::from_secs(3))
        );
        assert_eq!(defaulted.get_keepalives_retries(), Some(3));

        let mut stricter = connection_config(
            "host=localhost sslmode=disable tcp_user_timeout=4 keepalives_idle=4 keepalives_interval=2 keepalives_retries=2",
            None,
        )
        .expect("bounded connection config");
        assert!(require_bounded_socket_liveness(&mut stricter).is_ok());
        assert_eq!(
            stricter.get_tcp_user_timeout(),
            Some(&Duration::from_secs(4))
        );
        assert_eq!(stricter.get_keepalives_idle(), Duration::from_secs(4));
        assert_eq!(
            stricter.get_keepalives_interval(),
            Some(Duration::from_secs(2))
        );
        assert_eq!(stricter.get_keepalives_retries(), Some(2));

        for conninfo in [
            "host=localhost sslmode=disable tcp_user_timeout=11",
            "host=localhost sslmode=disable keepalives=0",
        ] {
            let mut invalid =
                connection_config(conninfo, None).expect("connection config should parse");
            assert!(require_bounded_socket_liveness(&mut invalid).is_err());
        }
    }
}
