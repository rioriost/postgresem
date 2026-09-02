//! Authority configuration loading and bearer JWT verification for the
//! authenticated MCP HTTP adapter.
//!
//! This module owns the strict, fail-closed interpretation of
//! `schemas/mcp-http/v1.authority.schema.json`. It performs no network I/O and
//! no database work: the JWKS document and the principal pseudonym key are read
//! from local files that the operator provisions as read-only secrets.
//!
//! Nothing here weakens PostgreSQL authorization. A verified identity only
//! selects a preconfigured role name; PostgreSQL membership, `GRANT`, and RLS
//! remain the final boundary.
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fmt, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{AlgorithmParameters, EllipticCurve, JwkSet, KeyOperations, PublicKeyUse},
};
use serde::Deserialize;
use serde_json::Value;
use sha2::Sha256;
use url::Url;

/// Maximum accepted size of the authority document itself.
const MAX_AUTHORITY_FILE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum accepted size of the local JWKS document.
const MAX_JWKS_FILE_BYTES: usize = 1024 * 1024;
/// Minimum accepted principal pseudonym key length.
const MIN_HMAC_KEY_BYTES: usize = 32;
/// Maximum accepted principal pseudonym key length.
const MAX_HMAC_KEY_BYTES: usize = 4096;
/// Maximum number of top level claims accepted in an access token.
const MAX_TOKEN_CLAIMS: usize = 64;
/// Maximum number of entries accepted in an `aud` array.
const MAX_AUDIENCE_ENTRIES: usize = 16;
/// Maximum number of scope tokens accepted from an access token.
const MAX_TOKEN_SCOPES: usize = 64;
/// Maximum accepted `sub` length, matching the schema principal subject bound.
const MAX_SUBJECT_BYTES: usize = 512;
/// Maximum accepted `iss` length, matching the schema issuer bound.
const MAX_ISSUER_BYTES: usize = 2048;
/// Maximum accepted URI length in the authority document.
const MAX_URI_BYTES: usize = 2048;
/// Maximum accepted filesystem path length in the authority document.
const MAX_PATH_BYTES: usize = 4096;
/// Maximum accepted JWK count in the local JWKS document.
const MAX_JWKS_KEYS: usize = 64;
/// Domain separation label for the audit principal pseudonym.
const PSEUDONYM_DOMAIN: &[u8] = b"postgresem.mcp-http.principal-pseudonym.v1";

type HmacSha256 = Hmac<Sha256>;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Stable category of an authority configuration failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConfigErrorKind {
    /// A configured file could not be read.
    Io,
    /// A configured file was not well formed JSON, or contained unknown fields.
    Parse,
    /// A value violated the authority JSON Schema.
    Schema,
    /// Individually valid values were mutually inconsistent.
    Semantic,
    /// A key material file was unusable for token verification or pseudonyms.
    Key,
}

impl ConfigErrorKind {
    /// Stable machine readable code for logs and tests.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Io => "authority_io",
            Self::Parse => "authority_parse",
            Self::Schema => "authority_schema",
            Self::Semantic => "authority_semantic",
            Self::Key => "authority_key",
        }
    }
}

/// Authority configuration failure.
///
/// The `Display` output never contains file contents, key material, or
/// principal subjects; it names the offending field only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    kind: ConfigErrorKind,
    reason: &'static str,
    index: Option<usize>,
}

impl ConfigError {
    fn new(kind: ConfigErrorKind, reason: &'static str) -> Self {
        Self {
            kind,
            reason,
            index: None,
        }
    }

    fn io(reason: &'static str) -> Self {
        Self::new(ConfigErrorKind::Io, reason)
    }

    fn parse(reason: &'static str) -> Self {
        Self::new(ConfigErrorKind::Parse, reason)
    }

    fn schema(reason: &'static str) -> Self {
        Self::new(ConfigErrorKind::Schema, reason)
    }

    fn semantic(reason: &'static str) -> Self {
        Self::new(ConfigErrorKind::Semantic, reason)
    }

    fn key(reason: &'static str) -> Self {
        Self::new(ConfigErrorKind::Key, reason)
    }

    fn at(mut self, index: usize) -> Self {
        self.index = Some(index);
        self
    }

    /// Stable failure category.
    #[must_use]
    #[cfg(test)]
    pub fn kind(&self) -> ConfigErrorKind {
        self.kind
    }

    /// Stable, secret free reason label.
    #[must_use]
    #[cfg(test)]
    pub fn reason(&self) -> &'static str {
        self.reason
    }

    /// Zero based collection index the failure applies to, when relevant.
    #[must_use]
    #[cfg(test)]
    pub fn index(&self) -> Option<usize> {
        self.index
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.code(), self.reason)?;
        if let Some(index) = self.index {
            write!(f, " (entry {index})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

/// Stable public category of a request authentication failure.
///
/// The `Display` and `Debug` output of this type is a fixed label. It never
/// contains the presented token, its signature, or the token subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AuthError {
    /// No usable `Authorization: Bearer` credential was presented.
    MissingCredentials,
    /// The `Authorization` header was syntactically unusable.
    MalformedRequest,
    /// The presented token exceeded the configured token byte budget.
    TokenTooLarge,
    /// The token was not a well formed compact JWS.
    MalformedToken,
    /// The JOSE `typ` header was absent or not in the configured allowlist.
    UnsupportedTokenType,
    /// The JOSE `alg` header was not in the configured allowlist.
    UnsupportedAlgorithm,
    /// The JOSE header referenced no key, or no locally configured key.
    UnknownKey,
    /// The signature did not verify against the selected local key.
    InvalidSignature,
    /// The `iss` claim did not equal the configured issuer.
    InvalidIssuer,
    /// The `aud` claim did not contain the configured resource.
    InvalidAudience,
    /// The token expired, accounting for the configured clock skew.
    TokenExpired,
    /// The token is not valid yet, accounting for the configured clock skew.
    TokenNotYetValid,
    /// The token is older than the configured maximum token age.
    TokenTooOld,
    /// A required claim was missing, ill typed, or out of bounds.
    InvalidClaims,
    /// The verified subject is not mapped to a configured principal.
    UnknownPrincipal,
    /// The verified identity holds none of its configured scopes.
    InsufficientScope,
    /// The gateway could not evaluate the credential.
    Internal,
}

impl AuthError {
    /// Stable machine readable code for logs, metrics, and tests.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::MissingCredentials => "missing_credentials",
            Self::MalformedRequest => "malformed_request",
            Self::TokenTooLarge => "token_too_large",
            Self::MalformedToken => "malformed_token",
            Self::UnsupportedTokenType => "unsupported_token_type",
            Self::UnsupportedAlgorithm => "unsupported_algorithm",
            Self::UnknownKey => "unknown_key",
            Self::InvalidSignature => "invalid_signature",
            Self::InvalidIssuer => "invalid_issuer",
            Self::InvalidAudience => "invalid_audience",
            Self::TokenExpired => "token_expired",
            Self::TokenNotYetValid => "token_not_yet_valid",
            Self::TokenTooOld => "token_too_old",
            Self::InvalidClaims => "invalid_claims",
            Self::UnknownPrincipal => "unknown_principal",
            Self::InsufficientScope => "insufficient_scope",
            Self::Internal => "internal_error",
        }
    }

    /// RFC 6750 `error` parameter for a `WWW-Authenticate` challenge.
    #[must_use]
    pub fn oauth_error(self) -> &'static str {
        match self {
            Self::MissingCredentials => "",
            Self::MalformedRequest | Self::TokenTooLarge => "invalid_request",
            Self::InsufficientScope => "insufficient_scope",
            Self::Internal => "server_error",
            _ => "invalid_token",
        }
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for AuthError {}

// ---------------------------------------------------------------------------
// Authority document
// ---------------------------------------------------------------------------

/// Per-principal rate limit, mirroring `$defs/rateLimit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfig {
    /// Sustained request budget per minute.
    pub requests_per_minute: u32,
    /// Burst capacity of the token bucket.
    pub burst: u32,
    /// Maximum simultaneously executing requests.
    pub max_concurrent: u32,
}

/// Process wide limits, mirroring `$defs/serverLimits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerLimits {
    /// Maximum accepted request body size in bytes.
    pub max_request_body_bytes: u64,
    /// Maximum accepted bearer token size in bytes.
    pub max_token_bytes: u64,
    /// Maximum accepted total header size in bytes.
    pub max_header_bytes: u64,
    /// Maximum wall clock seconds for a single execution.
    pub max_execution_seconds: u64,
    /// Maximum serialized result size in bytes.
    pub max_result_bytes: u64,
    /// Maximum authenticated requests in flight.
    pub max_concurrent_requests: u32,
    /// Maximum unauthenticated requests in flight.
    pub max_pre_auth_concurrent_requests: u32,
    /// Maximum PostgreSQL connections the adapter may hold.
    pub max_database_connections: u32,
    /// SSE keepalive comment interval in seconds.
    pub sse_keepalive_seconds: u64,
    /// Maximum lifetime of a single SSE stream in seconds.
    pub max_sse_seconds: u64,
}

/// Principal entry, mirroring `$defs/principal`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrincipalConfig {
    /// Exact `sub` claim value this entry maps.
    pub subject: String,
    /// Stable operator identifier used to namespace state and limits.
    pub authority_id: String,
    /// PostgreSQL role used for read only execution.
    pub query_role: String,
    /// PostgreSQL role used for mutation, when mutation is granted.
    #[serde(default)]
    pub mutation_role: Option<String>,
    /// Scopes this principal may ever exercise.
    pub allowed_scopes: Vec<String>,
    /// Per-principal rate limit.
    pub rate_limit: RateLimitConfig,
}

/// Authority document, mirroring `v1.authority.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityDocument {
    /// Document schema version; must be `"1"`.
    pub schema_version: String,
    /// Canonical HTTPS resource identifier of this MCP server.
    pub resource: String,
    /// Expected token issuer.
    pub issuer: String,
    /// Expected token audience; must equal `resource`.
    pub audience: String,
    /// Authorization servers advertised in protected resource metadata.
    pub authorization_servers: Vec<String>,
    /// Scopes advertised in protected resource metadata.
    pub scopes_supported: Vec<String>,
    /// Path to the local JWKS document.
    pub jwks_path: String,
    /// Path to the raw principal pseudonym HMAC key.
    pub principal_hmac_key_path: String,
    /// Accepted JOSE `typ` header values.
    pub allowed_token_types: Vec<String>,
    /// Accepted JOSE `alg` header values.
    pub allowed_algorithms: Vec<String>,
    /// Claim names inspected for granted scopes.
    pub scope_claims: Vec<String>,
    /// Accepted clock skew in seconds.
    pub clock_skew_seconds: u64,
    /// Maximum accepted age of a token in seconds, measured from `iat`.
    pub max_token_age_seconds: u64,
    /// Accepted public `Host` header values.
    pub allowed_hosts: Vec<String>,
    /// Accepted browser `Origin` header values.
    pub allowed_origins: Vec<String>,
    /// Scope required for read only execution.
    pub query_scope: String,
    /// Scope required for mutation.
    pub mutation_scope: String,
    /// Whether remote mutation is advertised and executable at all.
    pub remote_mutation_enabled: bool,
    /// Process wide limits.
    pub server_limits: ServerLimits,
    /// Subject to role and scope mappings.
    pub principals: Vec<PrincipalConfig>,
}

// ---------------------------------------------------------------------------
// Verified identity
// ---------------------------------------------------------------------------

/// Identity established from a verified bearer token.
///
/// This value intentionally does not carry the raw token or the token subject.
/// `audit_pseudonym` is the only stable identity handle safe to log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedPrincipal {
    authority_id: String,
    query_role: String,
    mutation_role: Option<String>,
    granted_scopes: BTreeSet<String>,
    rate_limit: RateLimitConfig,
    audit_pseudonym: String,
}

impl AuthenticatedPrincipal {
    /// Stable operator identifier used to namespace mutation state and limits.
    #[must_use]
    pub fn authority_id(&self) -> &str {
        &self.authority_id
    }

    /// PostgreSQL role used for read only execution.
    #[must_use]
    pub fn query_role(&self) -> &str {
        &self.query_role
    }

    /// PostgreSQL role used for mutation, present only when mutation is both
    /// enabled globally and granted to this identity.
    #[must_use]
    pub fn mutation_role(&self) -> Option<&str> {
        self.mutation_role.as_deref()
    }

    /// Scopes present in the token and permitted for this principal.
    #[must_use]
    #[cfg(test)]
    pub fn granted_scopes(&self) -> &BTreeSet<String> {
        &self.granted_scopes
    }

    /// Whether a specific scope was granted.
    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.granted_scopes.contains(scope)
    }

    /// Per-principal rate limit for this identity.
    #[must_use]
    pub fn rate_limit(&self) -> RateLimitConfig {
        self.rate_limit
    }

    /// Keyed pseudonym of issuer and subject, safe for audit logs.
    #[must_use]
    pub fn audit_pseudonym(&self) -> &str {
        &self.audit_pseudonym
    }
}

// ---------------------------------------------------------------------------
// Internal records
// ---------------------------------------------------------------------------

struct VerificationKey {
    key: DecodingKey,
    algorithms: Vec<Algorithm>,
}

impl fmt::Debug for VerificationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerificationKey")
            .field("algorithms", &self.algorithms)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct PrincipalRecord {
    authority_id: String,
    query_role: String,
    mutation_role: Option<String>,
    allowed_scopes: BTreeSet<String>,
    rate_limit: RateLimitConfig,
}

struct PseudonymKey(Vec<u8>);

impl fmt::Debug for PseudonymKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PseudonymKey(redacted)")
    }
}

// ---------------------------------------------------------------------------
// Authority
// ---------------------------------------------------------------------------

/// Validated authority configuration and token verifier.
pub struct Authority {
    document: AuthorityDocument,
    algorithms: Vec<Algorithm>,
    token_types: BTreeSet<String>,
    scope_claims: Vec<String>,
    scopes_supported: BTreeSet<String>,
    allowed_hosts: BTreeSet<String>,
    allowed_origins: BTreeSet<String>,
    keys: BTreeMap<String, VerificationKey>,
    principals: BTreeMap<String, PrincipalRecord>,
    pseudonym_key: PseudonymKey,
}

impl fmt::Debug for Authority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Authority")
            .field("resource", &self.document.resource)
            .field("issuer", &self.document.issuer)
            .field("algorithms", &self.algorithms)
            .field("key_count", &self.keys.len())
            .field("principal_count", &self.principals.len())
            .finish_non_exhaustive()
    }
}

impl Authority {
    /// Load, validate, and prepare an authority configuration.
    ///
    /// `jwks_path` and `principal_hmac_key_path` are resolved relative to the
    /// directory containing `path` when they are not absolute.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when a file cannot be read, the document is not
    /// schema valid, values are mutually inconsistent, or key material is
    /// unusable.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = read_bounded(path, MAX_AUTHORITY_FILE_BYTES, "authority document")?;
        let document: AuthorityDocument = serde_json::from_slice(&raw)
            .map_err(|_| ConfigError::parse("authority document is not schema valid JSON"))?;
        validate_document(&document)?;

        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let jwks_bytes = read_bounded(
            &resolve_path(base, &document.jwks_path),
            MAX_JWKS_FILE_BYTES,
            "jwks document",
        )?;
        let hmac_bytes = read_bounded(
            &resolve_path(base, &document.principal_hmac_key_path),
            MAX_HMAC_KEY_BYTES,
            "principal hmac key",
        )?;
        Self::assemble(document, &jwks_bytes, &hmac_bytes)
    }

    fn assemble(
        document: AuthorityDocument,
        jwks_bytes: &[u8],
        hmac_bytes: &[u8],
    ) -> Result<Self, ConfigError> {
        let algorithms = document
            .allowed_algorithms
            .iter()
            .filter_map(|name| parse_algorithm(name))
            .collect::<Vec<_>>();
        if algorithms.len() != document.allowed_algorithms.len() {
            return Err(ConfigError::schema(
                "allowed_algorithms contains an unsupported value",
            ));
        }

        let keys = load_keys(jwks_bytes, &algorithms)?;
        let pseudonym_key = load_pseudonym_key(hmac_bytes)?;

        let token_types = document
            .allowed_token_types
            .iter()
            .map(|value| value.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        if token_types.len() != document.allowed_token_types.len() {
            return Err(ConfigError::schema(
                "allowed_token_types contains a case-insensitive duplicate",
            ));
        }

        let scopes_supported = document.scopes_supported.iter().cloned().collect();
        let allowed_hosts = document.allowed_hosts.iter().cloned().collect();
        let allowed_origins = document.allowed_origins.iter().cloned().collect();
        let scope_claims = document.scope_claims.clone();

        let mut principals = BTreeMap::new();
        for principal in &document.principals {
            principals.insert(
                principal.subject.clone(),
                PrincipalRecord {
                    authority_id: principal.authority_id.clone(),
                    query_role: principal.query_role.clone(),
                    mutation_role: principal.mutation_role.clone(),
                    allowed_scopes: principal.allowed_scopes.iter().cloned().collect(),
                    rate_limit: principal.rate_limit,
                },
            );
        }

        Ok(Self {
            document,
            algorithms,
            token_types,
            scope_claims,
            scopes_supported,
            allowed_hosts,
            allowed_origins,
            keys,
            principals,
            pseudonym_key,
        })
    }

    /// Canonical resource identifier of this server.
    #[must_use]
    pub fn resource(&self) -> &str {
        &self.document.resource
    }

    /// Expected token issuer.
    #[must_use]
    #[cfg(test)]
    pub fn issuer(&self) -> &str {
        &self.document.issuer
    }

    /// Authorization servers advertised in protected resource metadata.
    #[must_use]
    pub fn authorization_servers(&self) -> &[String] {
        &self.document.authorization_servers
    }

    /// Scopes advertised in protected resource metadata.
    #[must_use]
    pub fn scopes_supported(&self) -> &BTreeSet<String> {
        &self.scopes_supported
    }

    /// Scope required for read only execution.
    #[must_use]
    pub fn query_scope(&self) -> &str {
        &self.document.query_scope
    }

    /// Scope required for mutation.
    #[must_use]
    pub fn mutation_scope(&self) -> &str {
        &self.document.mutation_scope
    }

    /// Whether remote mutation is enabled at all.
    #[must_use]
    pub fn remote_mutation_enabled(&self) -> bool {
        self.document.remote_mutation_enabled
    }

    /// Process wide limits.
    #[must_use]
    pub fn server_limits(&self) -> ServerLimits {
        self.document.server_limits
    }

    /// Every configured PostgreSQL role, for startup validation.
    #[must_use]
    #[cfg(test)]
    pub fn configured_roles(&self) -> BTreeSet<String> {
        let mut roles = BTreeSet::new();
        for principal in self.principals.values() {
            roles.insert(principal.query_role.clone());
            if let Some(role) = &principal.mutation_role {
                roles.insert(role.clone());
            }
        }
        roles
    }

    /// Every configured read-only PostgreSQL role.
    #[must_use]
    pub fn query_roles(&self) -> BTreeSet<String> {
        self.principals
            .values()
            .map(|principal| principal.query_role.clone())
            .collect()
    }

    /// Every configured mutation PostgreSQL role.
    #[must_use]
    pub fn mutation_roles(&self) -> BTreeSet<String> {
        self.principals
            .values()
            .filter_map(|principal| principal.mutation_role.clone())
            .collect()
    }

    /// Whether a `Host` header value is accepted.
    #[must_use]
    pub fn is_allowed_host(&self, host: &str) -> bool {
        self.allowed_hosts.contains(&host.to_ascii_lowercase())
    }

    /// Whether an `Origin` header value is accepted.
    #[must_use]
    pub fn is_allowed_origin(&self, origin: &str) -> bool {
        self.allowed_origins.contains(origin)
    }

    /// Verify an `Authorization` header value against the current wall clock.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError`] when the credential is absent, malformed, not
    /// verifiable, temporally invalid, unmapped, or carries no usable scope.
    pub fn authenticate(
        &self,
        authorization_header: &str,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthError::Internal)?
            .as_secs();
        let now = i64::try_from(now).map_err(|_| AuthError::Internal)?;
        self.authenticate_at(authorization_header, now)
    }

    /// Verify an `Authorization` header value at an explicit Unix timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError`] as documented on [`Authority::authenticate`].
    pub fn authenticate_at(
        &self,
        authorization_header: &str,
        now_unix: i64,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        let token = self.extract_bearer(authorization_header)?;
        let header = decode_header(token).map_err(|_| AuthError::MalformedToken)?;

        if header.jwk.is_some()
            || header.jku.is_some()
            || header.x5u.is_some()
            || header.x5c.is_some()
            || header.x5t.is_some()
            || header.x5t_s256.is_some()
        {
            return Err(AuthError::UnknownKey);
        }

        let typ = header
            .typ
            .as_deref()
            .ok_or(AuthError::UnsupportedTokenType)?;
        if !self.token_types.contains(&typ.to_ascii_lowercase()) {
            return Err(AuthError::UnsupportedTokenType);
        }

        if !self.algorithms.contains(&header.alg) {
            return Err(AuthError::UnsupportedAlgorithm);
        }

        let kid = header.kid.as_deref().ok_or(AuthError::UnknownKey)?;
        if kid.is_empty() {
            return Err(AuthError::UnknownKey);
        }
        let key = self.keys.get(kid).ok_or(AuthError::UnknownKey)?;
        if !key.algorithms.contains(&header.alg) {
            return Err(AuthError::UnsupportedAlgorithm);
        }

        let mut validation = Validation::new(header.alg);
        validation.algorithms = vec![header.alg];
        validation.required_spec_claims = HashSet::new();
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.validate_aud = false;
        validation.leeway = 0;
        validation.aud = None;
        validation.iss = None;
        validation.sub = None;

        let data = decode::<Value>(token, &key.key, &validation)
            .map_err(|_| AuthError::InvalidSignature)?;
        let claims = data.claims.as_object().ok_or(AuthError::InvalidClaims)?;
        if claims.is_empty() || claims.len() > MAX_TOKEN_CLAIMS {
            return Err(AuthError::InvalidClaims);
        }

        self.check_issuer(claims)?;
        self.check_audience(claims)?;
        self.check_time(claims, now_unix)?;

        let subject = bounded_string(claims.get("sub"), MAX_SUBJECT_BYTES)?;
        let token_scopes = self.collect_scopes(claims)?;

        let record = self
            .principals
            .get(subject)
            .ok_or(AuthError::UnknownPrincipal)?;

        let granted: BTreeSet<String> = record
            .allowed_scopes
            .intersection(&token_scopes)
            .cloned()
            .collect();
        if granted.is_empty() {
            return Err(AuthError::InsufficientScope);
        }

        let mutation_role = if self.document.remote_mutation_enabled
            && granted.contains(&self.document.mutation_scope)
        {
            record.mutation_role.clone()
        } else {
            None
        };

        let audit_pseudonym = self.pseudonym(subject)?;

        Ok(AuthenticatedPrincipal {
            authority_id: record.authority_id.clone(),
            query_role: record.query_role.clone(),
            mutation_role,
            granted_scopes: granted,
            rate_limit: record.rate_limit,
            audit_pseudonym,
        })
    }

    fn extract_bearer<'a>(&self, header: &'a str) -> Result<&'a str, AuthError> {
        let trimmed = header.trim_matches([' ', '\t']);
        if trimmed.is_empty() {
            return Err(AuthError::MissingCredentials);
        }
        let (scheme, rest) = match trimmed.split_once(' ') {
            Some(parts) => parts,
            None => {
                if trimmed.eq_ignore_ascii_case("bearer") {
                    return Err(AuthError::MissingCredentials);
                }
                return Err(AuthError::MalformedRequest);
            }
        };
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(AuthError::MalformedRequest);
        }
        let token = rest.trim_start_matches(' ');
        if token.is_empty() {
            return Err(AuthError::MissingCredentials);
        }
        if token.contains(|c: char| c.is_ascii_whitespace()) {
            return Err(AuthError::MalformedRequest);
        }
        let max = usize::try_from(self.document.server_limits.max_token_bytes)
            .map_err(|_| AuthError::Internal)?;
        if token.len() > max {
            return Err(AuthError::TokenTooLarge);
        }
        if !token.is_ascii() {
            return Err(AuthError::MalformedToken);
        }
        let mut segments = token.split('.');
        let valid = match (
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
        ) {
            (Some(a), Some(b), Some(c), None) => {
                !a.is_empty()
                    && !b.is_empty()
                    && !c.is_empty()
                    && [a, b, c]
                        .iter()
                        .all(|segment| segment.bytes().all(is_base64url_byte))
            }
            _ => false,
        };
        if !valid {
            return Err(AuthError::MalformedToken);
        }
        Ok(token)
    }

    fn check_issuer(&self, claims: &serde_json::Map<String, Value>) -> Result<(), AuthError> {
        let issuer = bounded_string(claims.get("iss"), MAX_ISSUER_BYTES)
            .map_err(|_| AuthError::InvalidIssuer)?;
        if issuer == self.document.issuer {
            Ok(())
        } else {
            Err(AuthError::InvalidIssuer)
        }
    }

    fn check_audience(&self, claims: &serde_json::Map<String, Value>) -> Result<(), AuthError> {
        let resource = self.document.audience.as_str();
        match claims.get("aud") {
            Some(Value::String(value)) => {
                if value == resource {
                    Ok(())
                } else {
                    Err(AuthError::InvalidAudience)
                }
            }
            Some(Value::Array(values)) => {
                if values.is_empty() || values.len() > MAX_AUDIENCE_ENTRIES {
                    return Err(AuthError::InvalidAudience);
                }
                let mut matched = false;
                for value in values {
                    let entry = value.as_str().ok_or(AuthError::InvalidAudience)?;
                    if entry == resource {
                        matched = true;
                    }
                }
                if matched {
                    Ok(())
                } else {
                    Err(AuthError::InvalidAudience)
                }
            }
            _ => Err(AuthError::InvalidAudience),
        }
    }

    fn check_time(
        &self,
        claims: &serde_json::Map<String, Value>,
        now: i64,
    ) -> Result<(), AuthError> {
        let skew =
            i64::try_from(self.document.clock_skew_seconds).map_err(|_| AuthError::Internal)?;
        let max_age =
            i64::try_from(self.document.max_token_age_seconds).map_err(|_| AuthError::Internal)?;

        let exp = numeric_claim(claims, "exp")?.ok_or(AuthError::InvalidClaims)?;
        let iat = numeric_claim(claims, "iat")?.ok_or(AuthError::InvalidClaims)?;
        let nbf = numeric_claim(claims, "nbf")?;

        if exp < iat {
            return Err(AuthError::InvalidClaims);
        }
        if now.saturating_sub(skew) >= exp {
            return Err(AuthError::TokenExpired);
        }
        if let Some(nbf) = nbf {
            if now.saturating_add(skew) < nbf {
                return Err(AuthError::TokenNotYetValid);
            }
        }
        if now.saturating_add(skew) < iat {
            return Err(AuthError::InvalidClaims);
        }
        if now.saturating_sub(iat) > max_age.saturating_add(skew) {
            return Err(AuthError::TokenTooOld);
        }
        Ok(())
    }

    fn collect_scopes(
        &self,
        claims: &serde_json::Map<String, Value>,
    ) -> Result<BTreeSet<String>, AuthError> {
        let mut scopes = BTreeSet::new();
        for claim in &self.scope_claims {
            match claims.get(claim.as_str()) {
                None | Some(Value::Null) => {}
                Some(Value::String(value)) => {
                    for token in value.split([' ', '\t']) {
                        if token.is_empty() {
                            continue;
                        }
                        if !is_scope_token(token) {
                            return Err(AuthError::InvalidClaims);
                        }
                        scopes.insert(token.to_owned());
                    }
                }
                Some(Value::Array(values)) => {
                    if values.len() > MAX_TOKEN_SCOPES {
                        return Err(AuthError::InvalidClaims);
                    }
                    for value in values {
                        let token = value.as_str().ok_or(AuthError::InvalidClaims)?;
                        if !is_scope_token(token) {
                            return Err(AuthError::InvalidClaims);
                        }
                        scopes.insert(token.to_owned());
                    }
                }
                Some(_) => return Err(AuthError::InvalidClaims),
            }
            if scopes.len() > MAX_TOKEN_SCOPES {
                return Err(AuthError::InvalidClaims);
            }
        }
        Ok(scopes)
    }

    fn pseudonym(&self, subject: &str) -> Result<String, AuthError> {
        let mut mac =
            HmacSha256::new_from_slice(&self.pseudonym_key.0).map_err(|_| AuthError::Internal)?;
        mac.update(PSEUDONYM_DOMAIN);
        for field in [self.document.issuer.as_bytes(), subject.as_bytes()] {
            let len = u64::try_from(field.len()).map_err(|_| AuthError::Internal)?;
            mac.update(&len.to_be_bytes());
            mac.update(field);
        }
        Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
    }
}

// ---------------------------------------------------------------------------
// File and key loading
// ---------------------------------------------------------------------------

fn resolve_path(base: &Path, configured: &str) -> PathBuf {
    let path = Path::new(configured);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn read_bounded(path: &Path, max: usize, what: &'static str) -> Result<Vec<u8>, ConfigError> {
    let metadata = fs::metadata(path).map_err(|_| match what {
        "jwks document" => ConfigError::io("jwks document is not readable"),
        "principal hmac key" => ConfigError::io("principal hmac key is not readable"),
        _ => ConfigError::io("authority document is not readable"),
    })?;
    if !metadata.is_file() {
        return Err(ConfigError::io("configured path is not a regular file"));
    }
    if metadata.len() > max as u64 {
        return Err(ConfigError::io("configured file exceeds its size bound"));
    }
    let bytes = fs::read(path).map_err(|_| ConfigError::io("configured file is not readable"))?;
    if bytes.len() > max {
        return Err(ConfigError::io("configured file exceeds its size bound"));
    }
    Ok(bytes)
}

fn load_pseudonym_key(bytes: &[u8]) -> Result<PseudonymKey, ConfigError> {
    if bytes.len() < MIN_HMAC_KEY_BYTES {
        return Err(ConfigError::key(
            "principal hmac key is shorter than 32 bytes",
        ));
    }
    if bytes.len() > MAX_HMAC_KEY_BYTES {
        return Err(ConfigError::key("principal hmac key exceeds 4096 bytes"));
    }
    if bytes.iter().all(|byte| *byte == 0) {
        return Err(ConfigError::key("principal hmac key is all zero bytes"));
    }
    HmacSha256::new_from_slice(bytes)
        .map_err(|_| ConfigError::key("principal hmac key is unusable"))?;
    Ok(PseudonymKey(bytes.to_vec()))
}

fn load_keys(
    bytes: &[u8],
    allowed: &[Algorithm],
) -> Result<BTreeMap<String, VerificationKey>, ConfigError> {
    let set: JwkSet = serde_json::from_slice(bytes)
        .map_err(|_| ConfigError::parse("jwks document is not a valid JWK set"))?;
    if set.keys.is_empty() {
        return Err(ConfigError::key("jwks document contains no keys"));
    }
    if set.keys.len() > MAX_JWKS_KEYS {
        return Err(ConfigError::key("jwks document contains too many keys"));
    }

    let mut keys: BTreeMap<String, VerificationKey> = BTreeMap::new();
    for (index, jwk) in set.keys.iter().enumerate() {
        let kid = jwk
            .common
            .key_id
            .as_deref()
            .ok_or_else(|| ConfigError::key("jwks entry has no kid").at(index))?;
        if kid.is_empty() || kid.len() > 256 || !kid.is_ascii() {
            return Err(ConfigError::key("jwks entry has an unusable kid").at(index));
        }
        if let Some(use_) = &jwk.common.public_key_use {
            if *use_ != PublicKeyUse::Signature {
                return Err(ConfigError::key("jwks entry is not a signature key").at(index));
            }
        }
        if let Some(ops) = &jwk.common.key_operations {
            if !ops.contains(&KeyOperations::Verify) {
                return Err(ConfigError::key("jwks entry cannot be used to verify").at(index));
            }
        }

        let mut algorithms = match &jwk.algorithm {
            AlgorithmParameters::RSA(_) => {
                vec![Algorithm::RS256, Algorithm::RS384, Algorithm::RS512]
            }
            AlgorithmParameters::EllipticCurve(params) => match params.curve {
                EllipticCurve::P256 => vec![Algorithm::ES256],
                EllipticCurve::P384 => vec![Algorithm::ES384],
                _ => {
                    return Err(ConfigError::key("jwks entry uses an unsupported curve").at(index));
                }
            },
            AlgorithmParameters::OctetKeyPair(_) => vec![Algorithm::EdDSA],
            AlgorithmParameters::OctetKey(_) => {
                return Err(ConfigError::key("jwks entry is a symmetric key").at(index));
            }
        };
        algorithms.retain(|algorithm| allowed.contains(algorithm));
        if algorithms.is_empty() {
            return Err(ConfigError::key("jwks entry matches no allowed algorithm").at(index));
        }
        if let Some(declared) = &jwk.common.key_algorithm {
            let declared = parse_algorithm(&declared.to_string()).ok_or_else(|| {
                ConfigError::key("jwks entry declares an unsupported alg").at(index)
            })?;
            if !algorithms.contains(&declared) {
                return Err(
                    ConfigError::key("jwks entry alg conflicts with its key type").at(index),
                );
            }
            algorithms = vec![declared];
        }

        let key = DecodingKey::from_jwk(jwk)
            .map_err(|_| ConfigError::key("jwks entry is not a usable decoding key").at(index))?;
        if keys
            .insert(kid.to_owned(), VerificationKey { key, algorithms })
            .is_some()
        {
            return Err(ConfigError::key("jwks document contains a duplicate kid").at(index));
        }
    }
    Ok(keys)
}

// ---------------------------------------------------------------------------
// Document validation
// ---------------------------------------------------------------------------

fn validate_document(document: &AuthorityDocument) -> Result<(), ConfigError> {
    if document.schema_version != "1" {
        return Err(ConfigError::schema("schema_version must be \"1\""));
    }

    let resource = canonical_https_uri(&document.resource, true).map_err(ConfigError::schema)?;
    if document.audience != document.resource {
        return Err(ConfigError::semantic("audience must equal resource"));
    }
    canonical_https_uri(&document.issuer, true).map_err(ConfigError::schema)?;

    if document.authorization_servers.is_empty() || document.authorization_servers.len() > 8 {
        return Err(ConfigError::schema(
            "authorization_servers must hold 1 to 8 entries",
        ));
    }
    let mut seen_servers = BTreeSet::new();
    for (index, server) in document.authorization_servers.iter().enumerate() {
        canonical_https_uri(server, true)
            .map_err(|reason| ConfigError::schema(reason).at(index))?;
        if !seen_servers.insert(server.clone()) {
            return Err(ConfigError::schema("authorization_servers must be unique").at(index));
        }
    }

    validate_scope_list(&document.scopes_supported, 1, 32, "scopes_supported")?;
    let supported: BTreeSet<&str> = document
        .scopes_supported
        .iter()
        .map(String::as_str)
        .collect();

    if !is_scope_token(&document.query_scope) || !is_scope_token(&document.mutation_scope) {
        return Err(ConfigError::schema(
            "query_scope and mutation_scope must be valid scopes",
        ));
    }
    if document.query_scope == document.mutation_scope {
        return Err(ConfigError::semantic(
            "query_scope must differ from mutation_scope",
        ));
    }
    if !supported.contains(document.query_scope.as_str()) {
        return Err(ConfigError::semantic(
            "query_scope must appear in scopes_supported",
        ));
    }
    if !supported.contains(document.mutation_scope.as_str()) {
        return Err(ConfigError::semantic(
            "mutation_scope must appear in scopes_supported",
        ));
    }

    if document.jwks_path.is_empty() || document.jwks_path.len() > MAX_PATH_BYTES {
        return Err(ConfigError::schema("jwks_path must be 1 to 4096 bytes"));
    }
    if document.principal_hmac_key_path.is_empty()
        || document.principal_hmac_key_path.len() > MAX_PATH_BYTES
    {
        return Err(ConfigError::schema(
            "principal_hmac_key_path must be 1 to 4096 bytes",
        ));
    }

    if document.allowed_token_types.is_empty() || document.allowed_token_types.len() > 3 {
        return Err(ConfigError::schema(
            "allowed_token_types must hold 1 to 3 entries",
        ));
    }
    let mut seen_types = BTreeSet::new();
    for (index, value) in document.allowed_token_types.iter().enumerate() {
        if !matches!(value.as_str(), "at+jwt" | "application/at+jwt" | "JWT") {
            return Err(
                ConfigError::schema("allowed_token_types contains an unknown value").at(index),
            );
        }
        if !seen_types.insert(value.clone()) {
            return Err(ConfigError::schema("allowed_token_types must be unique").at(index));
        }
    }

    if document.allowed_algorithms.is_empty() || document.allowed_algorithms.len() > 6 {
        return Err(ConfigError::schema(
            "allowed_algorithms must hold 1 to 6 entries",
        ));
    }
    let mut seen_algorithms = BTreeSet::new();
    for (index, value) in document.allowed_algorithms.iter().enumerate() {
        if parse_algorithm(value).is_none() {
            return Err(
                ConfigError::schema("allowed_algorithms contains an unknown value").at(index),
            );
        }
        if !seen_algorithms.insert(value.clone()) {
            return Err(ConfigError::schema("allowed_algorithms must be unique").at(index));
        }
    }

    if document.scope_claims.is_empty() || document.scope_claims.len() > 4 {
        return Err(ConfigError::schema("scope_claims must hold 1 to 4 entries"));
    }
    let mut seen_claims = BTreeSet::new();
    for (index, value) in document.scope_claims.iter().enumerate() {
        if !is_claim_name(value) {
            return Err(
                ConfigError::schema("scope_claims contains an invalid claim name").at(index),
            );
        }
        if !seen_claims.insert(value.clone()) {
            return Err(ConfigError::schema("scope_claims must be unique").at(index));
        }
    }

    if document.clock_skew_seconds > 300 {
        return Err(ConfigError::schema("clock_skew_seconds must be 0 to 300"));
    }
    if document.max_token_age_seconds == 0 || document.max_token_age_seconds > 86_400 {
        return Err(ConfigError::schema(
            "max_token_age_seconds must be 1 to 86400",
        ));
    }

    validate_hosts(document, &resource)?;
    validate_origins(document)?;
    validate_limits(&document.server_limits)?;
    validate_principals(document, &supported)?;
    Ok(())
}

fn validate_hosts(document: &AuthorityDocument, resource: &Url) -> Result<(), ConfigError> {
    if document.allowed_hosts.is_empty() || document.allowed_hosts.len() > 32 {
        return Err(ConfigError::schema(
            "allowed_hosts must hold 1 to 32 entries",
        ));
    }
    let mut seen = BTreeSet::new();
    for (index, host) in document.allowed_hosts.iter().enumerate() {
        if !is_host_pattern(host) {
            return Err(ConfigError::schema("allowed_hosts contains an invalid host").at(index));
        }
        if *host != host.to_ascii_lowercase() {
            return Err(ConfigError::schema("allowed_hosts must be lowercase").at(index));
        }
        if !seen.insert(host.clone()) {
            return Err(ConfigError::schema("allowed_hosts must be unique").at(index));
        }
    }
    let host = resource
        .host_str()
        .ok_or_else(|| ConfigError::schema("resource has no host"))?;
    let authority = match resource.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    if !seen.contains(&authority) {
        return Err(ConfigError::semantic(
            "allowed_hosts must contain the resource host",
        ));
    }
    Ok(())
}

fn validate_origins(document: &AuthorityDocument) -> Result<(), ConfigError> {
    if document.allowed_origins.len() > 32 {
        return Err(ConfigError::schema(
            "allowed_origins must hold at most 32 entries",
        ));
    }
    let mut seen = BTreeSet::new();
    for (index, origin) in document.allowed_origins.iter().enumerate() {
        if origin.len() > MAX_URI_BYTES {
            return Err(ConfigError::schema("allowed_origins entry is too long").at(index));
        }
        let parsed = Url::parse(origin)
            .map_err(|_| ConfigError::schema("allowed_origins entry is not a URI").at(index))?;
        if parsed.scheme() != "https" {
            return Err(ConfigError::schema("allowed_origins entry must be https").at(index));
        }
        if parsed.origin().ascii_serialization() != *origin {
            return Err(
                ConfigError::schema("allowed_origins entry must be a bare origin").at(index),
            );
        }
        if !seen.insert(origin.clone()) {
            return Err(ConfigError::schema("allowed_origins must be unique").at(index));
        }
    }
    Ok(())
}

fn validate_limits(limits: &ServerLimits) -> Result<(), ConfigError> {
    if !(1024..=1_048_576).contains(&limits.max_request_body_bytes) {
        return Err(ConfigError::schema(
            "max_request_body_bytes is out of range",
        ));
    }
    if !(256..=65_536).contains(&limits.max_token_bytes) {
        return Err(ConfigError::schema("max_token_bytes is out of range"));
    }
    if !(1024..=1_048_576).contains(&limits.max_header_bytes) {
        return Err(ConfigError::schema("max_header_bytes is out of range"));
    }
    if !(1..=3600).contains(&limits.max_execution_seconds) {
        return Err(ConfigError::schema("max_execution_seconds is out of range"));
    }
    if !(1024..=1_073_741_824).contains(&limits.max_result_bytes) {
        return Err(ConfigError::schema("max_result_bytes is out of range"));
    }
    if !(1..=10_000).contains(&limits.max_concurrent_requests) {
        return Err(ConfigError::schema(
            "max_concurrent_requests is out of range",
        ));
    }
    if !(1..=10_000).contains(&limits.max_pre_auth_concurrent_requests) {
        return Err(ConfigError::schema(
            "max_pre_auth_concurrent_requests is out of range",
        ));
    }
    if !(2..=10_000).contains(&limits.max_database_connections) {
        return Err(ConfigError::schema(
            "max_database_connections is out of range",
        ));
    }
    if !(1..=60).contains(&limits.sse_keepalive_seconds) {
        return Err(ConfigError::schema("sse_keepalive_seconds is out of range"));
    }
    if !(1..=3600).contains(&limits.max_sse_seconds) {
        return Err(ConfigError::schema("max_sse_seconds is out of range"));
    }
    if limits.max_pre_auth_concurrent_requests > limits.max_concurrent_requests {
        return Err(ConfigError::semantic(
            "max_pre_auth_concurrent_requests must not exceed max_concurrent_requests",
        ));
    }
    if limits.max_token_bytes > limits.max_header_bytes {
        return Err(ConfigError::semantic(
            "max_token_bytes must not exceed max_header_bytes",
        ));
    }
    if limits.sse_keepalive_seconds > limits.max_sse_seconds {
        return Err(ConfigError::semantic(
            "sse_keepalive_seconds must not exceed max_sse_seconds",
        ));
    }
    Ok(())
}

fn validate_principals(
    document: &AuthorityDocument,
    supported: &BTreeSet<&str>,
) -> Result<(), ConfigError> {
    if document.principals.is_empty() || document.principals.len() > 10_000 {
        return Err(ConfigError::schema(
            "principals must hold 1 to 10000 entries",
        ));
    }

    let mut subjects = BTreeSet::new();
    let mut authority_ids = BTreeSet::new();
    let mut mutation_capable = false;

    for (index, principal) in document.principals.iter().enumerate() {
        if principal.subject.is_empty() || principal.subject.len() > MAX_SUBJECT_BYTES {
            return Err(ConfigError::schema("subject must be 1 to 512 bytes").at(index));
        }
        if principal.subject.contains(char::is_control) {
            return Err(
                ConfigError::schema("subject must not contain control characters").at(index),
            );
        }
        if !subjects.insert(principal.subject.clone()) {
            return Err(ConfigError::semantic("principal subjects must be unique").at(index));
        }
        if !is_authority_id(&principal.authority_id) {
            return Err(ConfigError::schema("authority_id has an invalid format").at(index));
        }
        if !authority_ids.insert(principal.authority_id.clone()) {
            return Err(ConfigError::semantic("authority_id values must be unique").at(index));
        }
        validate_role(&principal.query_role)
            .map_err(|reason| ConfigError::schema(reason).at(index))?;
        if let Some(role) = &principal.mutation_role {
            validate_role(role).map_err(|reason| ConfigError::schema(reason).at(index))?;
        }

        validate_scope_list(&principal.allowed_scopes, 1, 32, "allowed_scopes")
            .map_err(|error| error.at(index))?;
        for scope in &principal.allowed_scopes {
            if !supported.contains(scope.as_str()) {
                return Err(ConfigError::semantic(
                    "allowed_scopes must be a subset of scopes_supported",
                )
                .at(index));
            }
        }

        let grants_mutation = principal.allowed_scopes.contains(&document.mutation_scope);
        let grants_query = principal.allowed_scopes.contains(&document.query_scope);
        if !grants_query {
            return Err(
                ConfigError::semantic("allowed_scopes must contain the query scope").at(index),
            );
        }
        if grants_mutation != principal.mutation_role.is_some() {
            return Err(ConfigError::semantic(
                "mutation_role must be present exactly when the mutation scope is allowed",
            )
            .at(index));
        }
        if grants_mutation
            && principal.mutation_role.as_deref() == Some(principal.query_role.as_str())
        {
            return Err(
                ConfigError::semantic("mutation_role must differ from query_role").at(index),
            );
        }
        mutation_capable = mutation_capable || grants_mutation;

        validate_rate_limit(&principal.rate_limit).map_err(|error| error.at(index))?;
    }

    if document.remote_mutation_enabled && !mutation_capable {
        return Err(ConfigError::semantic(
            "remote_mutation_enabled requires at least one mutation capable principal",
        ));
    }
    Ok(())
}

fn validate_rate_limit(limit: &RateLimitConfig) -> Result<(), ConfigError> {
    if !(1..=1_000_000).contains(&limit.requests_per_minute) {
        return Err(ConfigError::schema("requests_per_minute is out of range"));
    }
    if !(1..=100_000).contains(&limit.burst) {
        return Err(ConfigError::schema("burst is out of range"));
    }
    if !(1..=10_000).contains(&limit.max_concurrent) {
        return Err(ConfigError::schema("max_concurrent is out of range"));
    }
    if limit.burst > limit.requests_per_minute {
        return Err(ConfigError::semantic(
            "burst must not exceed requests_per_minute",
        ));
    }
    if limit.max_concurrent > limit.burst {
        return Err(ConfigError::semantic(
            "max_concurrent must not exceed burst",
        ));
    }
    Ok(())
}

fn validate_scope_list(
    scopes: &[String],
    min: usize,
    max: usize,
    what: &'static str,
) -> Result<(), ConfigError> {
    if scopes.len() < min || scopes.len() > max {
        return Err(ConfigError::schema(match what {
            "scopes_supported" => "scopes_supported must hold 1 to 32 entries",
            _ => "allowed_scopes must hold 1 to 32 entries",
        }));
    }
    let mut seen = BTreeSet::new();
    for scope in scopes {
        if !is_scope_token(scope) {
            return Err(ConfigError::schema(match what {
                "scopes_supported" => "scopes_supported contains an invalid scope",
                _ => "allowed_scopes contains an invalid scope",
            }));
        }
        if !seen.insert(scope.clone()) {
            return Err(ConfigError::schema(match what {
                "scopes_supported" => "scopes_supported must be unique",
                _ => "allowed_scopes must be unique",
            }));
        }
    }
    Ok(())
}

fn validate_role(role: &str) -> Result<(), &'static str> {
    if role.is_empty() || role.len() > 63 {
        return Err("database role must be 1 to 63 bytes");
    }
    let mut chars = role.chars();
    let first = chars.next().ok_or("database role must not be empty")?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err("database role must start with a letter or underscore");
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("database role contains an invalid character");
    }
    if role.len() >= 3 && role[..3].eq_ignore_ascii_case("pg_") {
        return Err("database role must not use the reserved pg_ prefix");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Small predicates
// ---------------------------------------------------------------------------

fn parse_algorithm(name: &str) -> Option<Algorithm> {
    match name {
        "RS256" => Some(Algorithm::RS256),
        "RS384" => Some(Algorithm::RS384),
        "RS512" => Some(Algorithm::RS512),
        "ES256" => Some(Algorithm::ES256),
        "ES384" => Some(Algorithm::ES384),
        "EdDSA" => Some(Algorithm::EdDSA),
        _ => None,
    }
}

fn is_scope_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| matches!(c, '\u{21}' | '\u{23}'..='\u{5B}' | '\u{5D}'..='\u{7E}'))
}

fn is_claim_name(value: &str) -> bool {
    if value.is_empty() || value.len() > 64 {
        return false;
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn is_authority_id(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 {
        return false;
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

fn is_host_pattern(value: &str) -> bool {
    if value.is_empty() || value.len() > 255 {
        return false;
    }
    let (host, port) = match value.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (value, None),
    };
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
    {
        return false;
    }
    match port {
        None => true,
        Some(port) => {
            !port.is_empty()
                && port.len() <= 5
                && port.chars().all(|c| c.is_ascii_digit())
                && port.parse::<u32>().map(|p| p <= 65_535).unwrap_or(false)
        }
    }
}

fn is_base64url_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

/// Parse an HTTPS URI and require it to already be in canonical form.
fn canonical_https_uri(raw: &str, allow_path: bool) -> Result<Url, &'static str> {
    if raw.is_empty() || raw.len() > MAX_URI_BYTES {
        return Err("uri must be 1 to 2048 bytes");
    }
    if !raw.is_ascii() {
        return Err("uri must be ASCII");
    }
    if raw.contains('%') {
        return Err("uri must not use percent-encoding");
    }
    let url = Url::parse(raw).map_err(|_| "uri is not a valid absolute URI")?;
    if url.scheme() != "https" {
        return Err("uri must use https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("uri must not contain userinfo");
    }
    if url.query().is_some() {
        return Err("uri must not contain a query");
    }
    if url.fragment().is_some() {
        return Err("uri must not contain a fragment");
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("uri must contain a host");
    }
    let path = url.path();
    if path != "/" {
        if !allow_path {
            return Err("uri must not contain a path");
        }
        if path.ends_with('/') {
            return Err("uri path must not end with a slash");
        }
        for segment in path.split('/').skip(1) {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err("uri path is not normalized");
            }
        }
    }
    let canonical = url.as_str();
    let matches_raw = canonical == raw
        || (path == "/" && canonical.len() == raw.len() + 1 && canonical.starts_with(raw));
    if !matches_raw {
        return Err("uri is not in canonical form");
    }
    Ok(url)
}

fn bounded_string(value: Option<&Value>, max: usize) -> Result<&str, AuthError> {
    let text = value
        .and_then(Value::as_str)
        .ok_or(AuthError::InvalidClaims)?;
    if text.is_empty() || text.len() > max || text.contains(char::is_control) {
        return Err(AuthError::InvalidClaims);
    }
    Ok(text)
}

fn numeric_claim(
    claims: &serde_json::Map<String, Value>,
    name: &str,
) -> Result<Option<i64>, AuthError> {
    match claims.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_i64().map(Some).ok_or(AuthError::InvalidClaims),
        Some(_) => Err(AuthError::InvalidClaims),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};

    use base64::engine::general_purpose::STANDARD;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde_json::json;

    /// TEST ONLY key material. This P-256 private key was generated for this
    /// test module alone, is committed deliberately, and must never be used by
    /// any runtime code path. Runtime keys are always read from operator files.
    const TEST_ONLY_EC_PKCS8_DER_BASE64: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgIsdDmETcgjs0NZDZinKPU1c7aDNCMWbO7p+gurfTFYahRANCAARwO2HxAX5NabVbfjTA6+ky8tnXrkgHVaMRykKxrshpNGrx06rCkL8ZVVtmu56agRc6KE1WAoKCD0HtiW6HUsPT";
    const TEST_ONLY_EC_X: &str = "cDth8QF-TWm1W340wOvpMvLZ165IB1WjEcpCsa7IaTQ";
    const TEST_ONLY_EC_Y: &str = "avHTqsKQvxlVW2a7npqBFzooTVYCgoIPQe2JbodSw9M";
    const TEST_KID: &str = "test-key";
    const NOW: i64 = 1_700_000_000;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default();
            let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("target");
            path.push("test-tmp");
            path.push(format!(
                "mcp-http-auth-{label}-{}-{nanos}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self { path }
        }

        fn write(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.path.join(name);
            fs::write(&path, contents).expect("write test file");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn base_document() -> Value {
        json!({
            "schema_version": "1",
            "resource": "https://mcp.example.test/mcp",
            "issuer": "https://identity.example.test",
            "audience": "https://mcp.example.test/mcp",
            "authorization_servers": ["https://identity.example.test"],
            "scopes_supported": ["postgresem.query", "postgresem.mutate"],
            "jwks_path": "jwks.json",
            "principal_hmac_key_path": "principal.key",
            "allowed_token_types": ["at+jwt", "application/at+jwt"],
            "allowed_algorithms": ["RS256", "ES256"],
            "scope_claims": ["scope", "scp"],
            "clock_skew_seconds": 30,
            "max_token_age_seconds": 3600,
            "allowed_hosts": ["mcp.example.test"],
            "allowed_origins": ["https://agent.example.test"],
            "query_scope": "postgresem.query",
            "mutation_scope": "postgresem.mutate",
            "remote_mutation_enabled": true,
            "server_limits": {
                "max_request_body_bytes": 1_048_576,
                "max_token_bytes": 16_384,
                "max_header_bytes": 32_768,
                "max_execution_seconds": 30,
                "max_result_bytes": 1_048_576,
                "max_concurrent_requests": 64,
                "max_pre_auth_concurrent_requests": 8,
                "max_database_connections": 32,
                "sse_keepalive_seconds": 10,
                "max_sse_seconds": 60
            },
            "principals": [
                {
                    "subject": "tenant-a-agent",
                    "authority_id": "tenant-a",
                    "query_role": "postgresem_tenant_a",
                    "allowed_scopes": ["postgresem.query"],
                    "rate_limit": {
                        "requests_per_minute": 60,
                        "burst": 10,
                        "max_concurrent": 4
                    }
                },
                {
                    "subject": "tenant-b-agent",
                    "authority_id": "tenant-b",
                    "query_role": "postgresem_tenant_b",
                    "mutation_role": "postgresem_order_writer",
                    "allowed_scopes": ["postgresem.query", "postgresem.mutate"],
                    "rate_limit": {
                        "requests_per_minute": 30,
                        "burst": 5,
                        "max_concurrent": 2
                    }
                }
            ]
        })
    }

    fn jwks_document() -> Value {
        json!({
            "keys": [
                {
                    "kty": "EC",
                    "crv": "P-256",
                    "alg": "ES256",
                    "use": "sig",
                    "kid": TEST_KID,
                    "x": TEST_ONLY_EC_X,
                    "y": TEST_ONLY_EC_Y
                }
            ]
        })
    }

    fn build(document: &Value, jwks: &Value) -> (TempDir, Result<Authority, ConfigError>) {
        let dir = TempDir::new("case");
        dir.write("jwks.json", jwks.to_string().as_bytes());
        dir.write("principal.key", &[7u8; 32]);
        let path = dir.write("authority.json", document.to_string().as_bytes());
        let authority = Authority::load(&path);
        (dir, authority)
    }

    fn authority_from(document: &Value) -> (TempDir, Authority) {
        let (dir, authority) = build(document, &jwks_document());
        let authority = authority.expect("authority should load");
        (dir, authority)
    }

    fn rejected(result: Result<Authority, ConfigError>) -> ConfigError {
        result.expect_err("configuration should be rejected")
    }

    fn config_error(document: &Value) -> ConfigError {
        let (_dir, authority) = build(document, &jwks_document());
        rejected(authority)
    }

    fn encoding_key() -> EncodingKey {
        let der = STANDARD
            .decode(TEST_ONLY_EC_PKCS8_DER_BASE64)
            .expect("decode test key");
        EncodingKey::from_ec_der(&der)
    }

    fn claims(subject: &str, scope: &str) -> Value {
        json!({
            "iss": "https://identity.example.test",
            "aud": "https://mcp.example.test/mcp",
            "sub": subject,
            "iat": NOW - 10,
            "nbf": NOW - 10,
            "exp": NOW + 300,
            "scope": scope
        })
    }

    fn sign(claims: &Value) -> String {
        let mut header = Header::new(Algorithm::ES256);
        header.typ = Some("at+jwt".to_owned());
        header.kid = Some(TEST_KID.to_owned());
        encode(&header, claims, &encoding_key()).expect("sign test token")
    }

    fn bearer(token: &str) -> String {
        format!("Bearer {token}")
    }

    fn craft(header: &Value, payload: &Value) -> String {
        format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(payload.to_string()),
            URL_SAFE_NO_PAD.encode("not-a-signature")
        )
    }

    fn crafted_header(typ: &str, alg: &str, kid: &str) -> Value {
        json!({ "typ": typ, "alg": alg, "kid": kid })
    }

    #[test]
    fn shipped_fixture_document_is_valid() {
        const FIXTURE: &str = include_str!("../../../fixtures/mcp-http/authority.example.json");
        let document: AuthorityDocument =
            serde_json::from_str(FIXTURE).expect("fixture should deserialize");
        validate_document(&document).expect("fixture should validate");
        assert_eq!(document.audience, document.resource);
        assert!(!document.remote_mutation_enabled);
    }

    #[test]
    fn shipped_fixture_shape_loads() {
        let mut document = base_document();
        document["remote_mutation_enabled"] = json!(false);
        document["jwks_path"] = json!("jwks.json");
        let (_dir, authority) = authority_from(&document);
        assert_eq!(authority.resource(), "https://mcp.example.test/mcp");
        assert_eq!(authority.issuer(), "https://identity.example.test");
        assert!(!authority.remote_mutation_enabled());
        assert_eq!(authority.configured_roles().len(), 3);
        assert!(authority.is_allowed_host("MCP.example.test"));
        assert!(authority.is_allowed_origin("https://agent.example.test"));
        assert!(!authority.is_allowed_origin("https://evil.example.test"));
    }

    #[test]
    fn valid_token_authenticates() {
        let (_dir, authority) = authority_from(&base_document());
        let token = sign(&claims(
            "tenant-b-agent",
            "postgresem.query postgresem.mutate",
        ));
        let principal = authority
            .authenticate_at(&bearer(&token), NOW)
            .expect("token should authenticate");
        assert_eq!(principal.authority_id(), "tenant-b");
        assert_eq!(principal.query_role(), "postgresem_tenant_b");
        assert_eq!(principal.mutation_role(), Some("postgresem_order_writer"));
        assert!(principal.has_scope("postgresem.query"));
        assert!(principal.has_scope("postgresem.mutate"));
        assert_eq!(principal.rate_limit().requests_per_minute, 30);
        assert!(!principal.audit_pseudonym().is_empty());
        assert!(!principal.audit_pseudonym().contains("tenant-b-agent"));
    }

    #[test]
    fn pseudonym_is_stable_keyed_and_subject_specific() {
        let (_dir, authority) = authority_from(&base_document());
        let a = authority.pseudonym("tenant-a-agent").expect("pseudonym");
        let b = authority.pseudonym("tenant-b-agent").expect("pseudonym");
        let again = authority.pseudonym("tenant-a-agent").expect("pseudonym");
        assert_eq!(a, again);
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);

        let dir = TempDir::new("other-key");
        dir.write("jwks.json", jwks_document().to_string().as_bytes());
        dir.write("principal.key", &[9u8; 32]);
        let path = dir.write("authority.json", base_document().to_string().as_bytes());
        let other = Authority::load(&path).expect("authority should load");
        assert_ne!(a, other.pseudonym("tenant-a-agent").expect("pseudonym"));
    }

    #[test]
    fn scope_intersection_is_limited_to_allowed_scopes() {
        let (_dir, authority) = authority_from(&base_document());
        let token = sign(&claims(
            "tenant-a-agent",
            "postgresem.query postgresem.mutate other.scope",
        ));
        let principal = authority
            .authenticate_at(&bearer(&token), NOW)
            .expect("token should authenticate");
        assert_eq!(principal.granted_scopes().len(), 1);
        assert!(principal.has_scope("postgresem.query"));
        assert!(!principal.has_scope("postgresem.mutate"));
        assert_eq!(principal.mutation_role(), None);
    }

    #[test]
    fn scope_array_claim_is_accepted() {
        let (_dir, authority) = authority_from(&base_document());
        let mut token_claims = claims("tenant-a-agent", "");
        token_claims["scope"] = json!(null);
        token_claims["scp"] = json!(["postgresem.query"]);
        let token = sign(&token_claims);
        let principal = authority
            .authenticate_at(&bearer(&token), NOW)
            .expect("token should authenticate");
        assert!(principal.has_scope("postgresem.query"));
    }

    #[test]
    fn audience_array_containing_resource_is_accepted() {
        let (_dir, authority) = authority_from(&base_document());
        let mut token_claims = claims("tenant-a-agent", "postgresem.query");
        token_claims["aud"] = json!(["https://other.example.test", "https://mcp.example.test/mcp"]);
        let token = sign(&token_claims);
        assert!(authority.authenticate_at(&bearer(&token), NOW).is_ok());
    }

    #[test]
    fn no_overlapping_scope_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let token = sign(&claims("tenant-a-agent", "postgresem.mutate"));
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::InsufficientScope)
        );
    }

    #[test]
    fn unmapped_subject_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let token = sign(&claims("tenant-z-agent", "postgresem.query"));
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::UnknownPrincipal)
        );
    }

    #[test]
    fn wrong_audience_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let mut token_claims = claims("tenant-a-agent", "postgresem.query");
        token_claims["aud"] = json!("https://other.example.test/mcp");
        let token = sign(&token_claims);
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::InvalidAudience)
        );
    }

    #[test]
    fn wrong_issuer_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let mut token_claims = claims("tenant-a-agent", "postgresem.query");
        token_claims["iss"] = json!("https://evil.example.test");
        let token = sign(&token_claims);
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::InvalidIssuer)
        );
    }

    #[test]
    fn expired_token_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let mut token_claims = claims("tenant-a-agent", "postgresem.query");
        token_claims["exp"] = json!(NOW - 60);
        token_claims["iat"] = json!(NOW - 120);
        token_claims["nbf"] = json!(NOW - 120);
        let token = sign(&token_claims);
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::TokenExpired)
        );
    }

    #[test]
    fn token_within_clock_skew_is_accepted() {
        let (_dir, authority) = authority_from(&base_document());
        let mut token_claims = claims("tenant-a-agent", "postgresem.query");
        token_claims["exp"] = json!(NOW - 10);
        let token = sign(&token_claims);
        assert!(authority.authenticate_at(&bearer(&token), NOW).is_ok());
    }

    #[test]
    fn future_token_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let mut token_claims = claims("tenant-a-agent", "postgresem.query");
        token_claims["nbf"] = json!(NOW + 600);
        let token = sign(&token_claims);
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::TokenNotYetValid)
        );
    }

    #[test]
    fn overaged_token_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let mut token_claims = claims("tenant-a-agent", "postgresem.query");
        token_claims["iat"] = json!(NOW - 7200);
        token_claims["nbf"] = json!(NOW - 7200);
        token_claims["exp"] = json!(NOW + 300);
        let token = sign(&token_claims);
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::TokenTooOld)
        );
    }

    #[test]
    fn missing_required_claims_are_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let mut token_claims = claims("tenant-a-agent", "postgresem.query");
        token_claims
            .as_object_mut()
            .expect("object claims")
            .remove("iat");
        let token = sign(&token_claims);
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::InvalidClaims)
        );
    }

    #[test]
    fn unsupported_token_type_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let token = craft(
            &crafted_header("JOSE", "ES256", TEST_KID),
            &claims("tenant-a-agent", "postgresem.query"),
        );
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::UnsupportedTokenType)
        );
    }

    #[test]
    fn token_type_match_is_case_insensitive() {
        let (_dir, authority) = authority_from(&base_document());
        let mut token_claims = claims("tenant-a-agent", "postgresem.query");
        token_claims["sub"] = json!("tenant-a-agent");
        let mut header = Header::new(Algorithm::ES256);
        header.typ = Some("AT+JWT".to_owned());
        header.kid = Some(TEST_KID.to_owned());
        let token = encode(&header, &token_claims, &encoding_key()).expect("sign test token");
        assert!(authority.authenticate_at(&bearer(&token), NOW).is_ok());
    }

    #[test]
    fn disallowed_algorithm_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let token = craft(
            &crafted_header("at+jwt", "HS256", TEST_KID),
            &claims("tenant-a-agent", "postgresem.query"),
        );
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::UnsupportedAlgorithm)
        );
    }

    #[test]
    fn unknown_kid_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let token = craft(
            &crafted_header("at+jwt", "ES256", "rotated-away"),
            &claims("tenant-a-agent", "postgresem.query"),
        );
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::UnknownKey)
        );
    }

    #[test]
    fn missing_kid_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let token = craft(
            &json!({ "typ": "at+jwt", "alg": "ES256" }),
            &claims("tenant-a-agent", "postgresem.query"),
        );
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::UnknownKey)
        );
    }

    #[test]
    fn embedded_jwk_header_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let header = json!({
            "typ": "at+jwt",
            "alg": "ES256",
            "kid": TEST_KID,
            "jwk": {
                "kty": "EC",
                "crv": "P-256",
                "x": TEST_ONLY_EC_X,
                "y": TEST_ONLY_EC_Y
            }
        });
        let token = craft(&header, &claims("tenant-a-agent", "postgresem.query"));
        assert_eq!(
            authority.authenticate_at(&bearer(&token), NOW),
            Err(AuthError::UnknownKey)
        );
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let token = sign(&claims("tenant-a-agent", "postgresem.query"));
        let mut parts = token.split('.');
        let header = parts.next().unwrap_or_default();
        let payload = parts.next().unwrap_or_default();
        let forged = format!("{header}.{payload}.{}", URL_SAFE_NO_PAD.encode([0u8; 64]));
        assert_eq!(
            authority.authenticate_at(&bearer(&forged), NOW),
            Err(AuthError::InvalidSignature)
        );
    }

    #[test]
    fn malformed_authorization_headers_are_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        assert_eq!(
            authority.authenticate_at("", NOW),
            Err(AuthError::MissingCredentials)
        );
        assert_eq!(
            authority.authenticate_at("Bearer ", NOW),
            Err(AuthError::MissingCredentials)
        );
        assert_eq!(
            authority.authenticate_at("Basic abcdef", NOW),
            Err(AuthError::MalformedRequest)
        );
        assert_eq!(
            authority.authenticate_at("Bearer a.b.c.d", NOW),
            Err(AuthError::MalformedToken)
        );
        assert_eq!(
            authority.authenticate_at("Bearer a.b.$$", NOW),
            Err(AuthError::MalformedToken)
        );
    }

    #[test]
    fn oversized_token_is_rejected() {
        let (_dir, authority) = authority_from(&base_document());
        let oversized = format!("Bearer {}", "a".repeat(20_000));
        assert_eq!(
            authority.authenticate_at(&oversized, NOW),
            Err(AuthError::TokenTooLarge)
        );
    }

    #[test]
    fn errors_never_disclose_token_or_subject() {
        let rendered = AuthError::UnknownPrincipal.to_string();
        assert_eq!(rendered, "unknown_principal");
        assert_eq!(
            AuthError::InsufficientScope.oauth_error(),
            "insufficient_scope"
        );
        assert_eq!(AuthError::InvalidSignature.oauth_error(), "invalid_token");
        let config = ConfigError::semantic("audience must equal resource").at(2);
        assert_eq!(
            config.to_string(),
            "authority_semantic: audience must equal resource (entry 2)"
        );
    }

    #[test]
    fn duplicate_subject_is_rejected() {
        let mut document = base_document();
        document["principals"][1]["subject"] = json!("tenant-a-agent");
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Semantic);
        assert_eq!(error.reason(), "principal subjects must be unique");
        assert_eq!(error.index(), Some(1));
    }

    #[test]
    fn duplicate_authority_id_is_rejected() {
        let mut document = base_document();
        document["principals"][1]["authority_id"] = json!("tenant-a");
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Semantic);
        assert_eq!(error.reason(), "authority_id values must be unique");
    }

    #[test]
    fn audience_must_equal_resource() {
        let mut document = base_document();
        document["audience"] = json!("https://mcp.example.test/other");
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Semantic);
        assert_eq!(error.reason(), "audience must equal resource");
    }

    #[test]
    fn unknown_field_is_rejected() {
        let mut document = base_document();
        document["surprise"] = json!(true);
        assert_eq!(config_error(&document).kind(), ConfigErrorKind::Parse);
    }

    #[test]
    fn wrong_schema_version_is_rejected() {
        let mut document = base_document();
        document["schema_version"] = json!("2");
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Schema);
        assert_eq!(error.reason(), "schema_version must be \"1\"");
    }

    #[test]
    fn non_canonical_resource_is_rejected() {
        for value in [
            "http://mcp.example.test/mcp",
            "https://user@mcp.example.test/mcp",
            "https://mcp.example.test/mcp?x=1",
            "https://mcp.example.test/mcp#a",
            "https://mcp.example.test/mcp/",
            "https://mcp.example.test/a/../mcp",
            "https://mcp.example.test/%6dcp",
        ] {
            let mut document = base_document();
            document["resource"] = json!(value);
            document["audience"] = json!(value);
            let error = config_error(&document);
            assert_eq!(error.kind(), ConfigErrorKind::Schema, "accepted {value}");
        }
    }

    #[test]
    fn resource_host_must_be_allowed() {
        let mut document = base_document();
        document["allowed_hosts"] = json!(["other.example.test"]);
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Semantic);
        assert_eq!(
            error.reason(),
            "allowed_hosts must contain the resource host"
        );
    }

    #[test]
    fn non_https_authorization_server_is_rejected() {
        let mut document = base_document();
        document["authorization_servers"] = json!(["http://identity.example.test"]);
        assert_eq!(config_error(&document).kind(), ConfigErrorKind::Schema);
    }

    #[test]
    fn scope_outside_supported_set_is_rejected() {
        let mut document = base_document();
        document["principals"][0]["allowed_scopes"] = json!(["postgresem.other"]);
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Semantic);
        assert_eq!(
            error.reason(),
            "allowed_scopes must be a subset of scopes_supported"
        );
    }

    #[test]
    fn mutation_role_without_mutation_scope_is_rejected() {
        let mut document = base_document();
        document["principals"][0]["mutation_role"] = json!("postgresem_writer");
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Semantic);
        assert_eq!(
            error.reason(),
            "mutation_role must be present exactly when the mutation scope is allowed"
        );
    }

    #[test]
    fn mutation_scope_cannot_replace_the_required_query_scope() {
        let mut document = base_document();
        document["principals"][1]["allowed_scopes"] = json!(["postgresem.mutate"]);
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Semantic);
        assert_eq!(
            error.reason(),
            "allowed_scopes must contain the query scope"
        );
    }

    #[test]
    fn remote_mutation_requires_a_capable_principal() {
        let mut document = base_document();
        document["principals"] = json!([document["principals"][0].clone()]);
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Semantic);
        assert_eq!(
            error.reason(),
            "remote_mutation_enabled requires at least one mutation capable principal"
        );
    }

    #[test]
    fn disabled_remote_mutation_withholds_the_mutation_role() {
        let mut document = base_document();
        document["remote_mutation_enabled"] = json!(false);
        let (_dir, authority) = authority_from(&document);
        let token = sign(&claims(
            "tenant-b-agent",
            "postgresem.query postgresem.mutate",
        ));
        let principal = authority
            .authenticate_at(&bearer(&token), NOW)
            .expect("token should authenticate");
        assert_eq!(principal.mutation_role(), None);
    }

    #[test]
    fn preauth_concurrency_must_not_exceed_global() {
        let mut document = base_document();
        document["server_limits"]["max_pre_auth_concurrent_requests"] = json!(128);
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Semantic);
        assert_eq!(
            error.reason(),
            "max_pre_auth_concurrent_requests must not exceed max_concurrent_requests"
        );
    }

    #[test]
    fn too_few_database_connections_are_rejected() {
        let mut document = base_document();
        document["server_limits"]["max_database_connections"] = json!(1);
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Schema);
        assert_eq!(error.reason(), "max_database_connections is out of range");
    }

    #[test]
    fn reserved_role_prefix_is_rejected() {
        let mut document = base_document();
        document["principals"][0]["query_role"] = json!("pg_read_all_data");
        assert_eq!(config_error(&document).kind(), ConfigErrorKind::Schema);
    }

    #[test]
    fn invalid_authority_id_is_rejected() {
        let mut document = base_document();
        document["principals"][0]["authority_id"] = json!("-tenant");
        let error = config_error(&document);
        assert_eq!(error.kind(), ConfigErrorKind::Schema);
        assert_eq!(error.reason(), "authority_id has an invalid format");
    }

    #[test]
    fn empty_scope_claim_list_is_rejected() {
        let mut document = base_document();
        document["scope_claims"] = json!([]);
        assert_eq!(config_error(&document).kind(), ConfigErrorKind::Schema);
    }

    #[test]
    fn empty_algorithm_list_is_rejected() {
        let mut document = base_document();
        document["allowed_algorithms"] = json!([]);
        assert_eq!(config_error(&document).kind(), ConfigErrorKind::Schema);
    }

    #[test]
    fn empty_token_type_list_is_rejected() {
        let mut document = base_document();
        document["allowed_token_types"] = json!([]);
        assert_eq!(config_error(&document).kind(), ConfigErrorKind::Schema);
    }

    #[test]
    fn symmetric_jwk_is_rejected() {
        let jwks = json!({
            "keys": [{
                "kty": "oct",
                "kid": TEST_KID,
                "alg": "HS256",
                "k": "c2VjcmV0LXNlY3JldC1zZWNyZXQtc2VjcmV0LXNlY3JldA"
            }]
        });
        let (_dir, result) = build(&base_document(), &jwks);
        let error = rejected(result);
        assert_eq!(error.kind(), ConfigErrorKind::Key);
        assert_eq!(error.reason(), "jwks entry is a symmetric key");
    }

    #[test]
    fn duplicate_kid_is_rejected() {
        let mut jwks = jwks_document();
        let entry = jwks["keys"][0].clone();
        jwks["keys"] = json!([entry.clone(), entry]);
        let (_dir, result) = build(&base_document(), &jwks);
        let error = rejected(result);
        assert_eq!(error.kind(), ConfigErrorKind::Key);
        assert_eq!(error.reason(), "jwks document contains a duplicate kid");
    }

    #[test]
    fn jwk_without_kid_is_rejected() {
        let mut jwks = jwks_document();
        jwks["keys"][0]
            .as_object_mut()
            .expect("jwk object")
            .remove("kid");
        let (_dir, result) = build(&base_document(), &jwks);
        assert_eq!(rejected(result).kind(), ConfigErrorKind::Key);
    }

    #[test]
    fn short_pseudonym_key_is_rejected() {
        let dir = TempDir::new("short-key");
        dir.write("jwks.json", jwks_document().to_string().as_bytes());
        dir.write("principal.key", b"too-short");
        let path = dir.write("authority.json", base_document().to_string().as_bytes());
        let error = rejected(Authority::load(&path));
        assert_eq!(error.kind(), ConfigErrorKind::Key);
        assert_eq!(
            error.reason(),
            "principal hmac key is shorter than 32 bytes"
        );
    }

    #[test]
    fn missing_files_report_io_errors() {
        let dir = TempDir::new("missing");
        let path = dir.write("authority.json", base_document().to_string().as_bytes());
        assert_eq!(rejected(Authority::load(&path)).kind(), ConfigErrorKind::Io);
    }
}
