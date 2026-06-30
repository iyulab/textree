//! Telemetry connection config — pure parsing of the App Insights connection string.
//! No I/O, no env reads here; the single on/off gate lives in `super::resolved_connection`.

/// The only switch for outbound telemetry. Absent/blank ⇒ disabled (dev, test, MIT forks,
/// and official builds whose CI has not injected the string yet).
pub const ENV_VAR: &str = "TEXTREE_TELEMETRY_CONNECTION";

#[derive(Clone, Debug, PartialEq)]
pub struct TelemetryConfig {
    pub instrumentation_key: String,
    /// Full ingestion URL ending in `/v2/track`.
    pub track_url: String,
}

/// Parses an App Insights connection string (`Key=Value;Key=Value;...`). Returns `None` unless
/// both `InstrumentationKey` and `IngestionEndpoint` are present and non-empty — so a malformed
/// or partial string disables telemetry rather than sending to the wrong place.
pub fn parse_connection(raw: &str) -> Option<TelemetryConfig> {
    let mut key: Option<&str> = None;
    let mut endpoint: Option<&str> = None;
    for part in raw.split(';') {
        let mut kv = part.splitn(2, '=');
        match (kv.next(), kv.next()) {
            (Some(k), Some(v)) => {
                let v = v.trim();
                if v.is_empty() {
                    continue;
                }
                match k.trim() {
                    "InstrumentationKey" => key = Some(v),
                    "IngestionEndpoint" => endpoint = Some(v),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let key = key?;
    let endpoint = endpoint?.trim_end_matches('/');
    Some(TelemetryConfig {
        instrumentation_key: key.to_string(),
        track_url: format!("{endpoint}/v2/track"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "InstrumentationKey=88f1e21d-26f9-48bd-8639-991e55b49378;IngestionEndpoint=https://koreacentral-0.in.applicationinsights.azure.com/;LiveEndpoint=https://x/;ApplicationId=c34cf9f2";

    #[test]
    fn parses_key_and_builds_track_url() {
        let c = parse_connection(FULL).expect("valid connection string parses");
        assert_eq!(c.instrumentation_key, "88f1e21d-26f9-48bd-8639-991e55b49378");
        assert_eq!(c.track_url, "https://koreacentral-0.in.applicationinsights.azure.com/v2/track");
    }

    #[test]
    fn endpoint_without_trailing_slash_still_builds_track_url() {
        let c = parse_connection("InstrumentationKey=abc;IngestionEndpoint=https://x.example.com").unwrap();
        assert_eq!(c.track_url, "https://x.example.com/v2/track");
    }

    #[test]
    fn none_when_key_missing() {
        assert!(parse_connection("IngestionEndpoint=https://x/").is_none());
    }

    #[test]
    fn none_when_endpoint_missing() {
        assert!(parse_connection("InstrumentationKey=abc").is_none());
    }

    #[test]
    fn none_when_blank_or_garbage() {
        assert!(parse_connection("").is_none());
        assert!(parse_connection("   ").is_none());
        assert!(parse_connection("nonsense").is_none());
    }
}
