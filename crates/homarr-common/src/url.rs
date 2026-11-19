use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::Url;

use crate::error::{HomarrError, HomarrResult};

pub fn remove_trailing_slash(path: &str) -> &str {
    path.trim_end_matches('/')
}

pub fn is_http_url(raw: &str) -> bool {
    Url::parse(raw)
        .map(|u| u.scheme() == "http" || u.scheme() == "https")
        .unwrap_or(false)
}

/// Parses an HTTP(S) URL without embedded credentials (`user:pass@`), returning `None` if invalid.
pub fn get_safe_application_url(raw: &str, base: Option<&str>) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed = match base {
        Some(base_raw) => {
            let mut base_url = parse_http_url_no_creds(base_raw)?;
            base_url.set_query(None);
            base_url.set_fragment(None);
            let joined = base_url.join(trimmed).ok()?;
            if (joined.scheme() != "http" && joined.scheme() != "https")
                || !joined.username().is_empty()
                || joined.password().is_some()
            {
                return None;
            }
            joined
        }
        None => parse_http_url_no_creds(trimmed)?,
    };

    Some(parsed.to_string())
}

fn parse_http_url_no_creds(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    if (url.scheme() != "http" && url.scheme() != "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url)
}

pub fn redact_url_query(raw: &str) -> String {
    let Ok(url) = Url::parse(raw) else {
        return "<invalid-url>".to_string();
    };
    let origin = url.origin().ascii_serialization();
    let path = url.path();
    let redacted_keys: Vec<String> = url
        .query_pairs()
        .map(|(k, _)| format!("{k}=REDACTED"))
        .collect();
    if redacted_keys.is_empty() {
        format!("{origin}{path}")
    } else {
        format!("{origin}{path}?{}", redacted_keys.join("&"))
    }
}

/// Checks if an IP address is blocked for untrusted outbound fetches (loopback, link-local,
/// unspecified, multicast, reserved, or cloud metadata endpoints).
/// Private RFC1918 networks (10/8, 172.16/12, 192.168/16) remain allowed for self-hosted apps.
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_ipv4(v4),
        IpAddr::V6(v6) => is_blocked_ipv6(v6),
    }
}

fn is_blocked_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    // 0.0.0.0/8 (unspecified)
    if octets[0] == 0 {
        return true;
    }
    // 127.0.0.0/8 (loopback)
    if octets[0] == 127 {
        return true;
    }
    // 169.254.0.0/16 (link-local & AWS/GCP/Azure metadata)
    if octets[0] == 169 && octets[1] == 254 {
        return true;
    }
    // 100.100.100.200/32 (Alibaba Cloud metadata)
    if octets == [100, 100, 100, 200] {
        return true;
    }
    // 168.63.129.16/32 (Azure WireServer)
    if octets == [168, 63, 129, 16] {
        return true;
    }
    // 224.0.0.0/4 (multicast) and 240.0.0.0/4 (reserved)
    if octets[0] >= 224 {
        return true;
    }
    false
}

fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return is_blocked_ipv4(mapped);
    }
    let segs = ip.segments();
    // ::/96 (includes :: and ::1)
    if segs[0..6] == [0, 0, 0, 0, 0, 0] {
        return true;
    }
    // fe80::/10 (link-local)
    if (segs[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    // fd00:ec2::254/128 (AWS IPv6 metadata)
    if segs == [0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254] {
        return true;
    }
    // ff00::/8 (multicast)
    if (segs[0] & 0xff00) == 0xff00 {
        return true;
    }
    // NAT64 64:ff9b::/96
    if segs[0] == 0x0064 && segs[1] == 0xff9b && segs[2..6] == [0, 0, 0, 0] {
        let embedded = Ipv4Addr::new(
            (segs[6] >> 8) as u8,
            (segs[6] & 0xff) as u8,
            (segs[7] >> 8) as u8,
            (segs[7] & 0xff) as u8,
        );
        return is_blocked_ipv4(embedded);
    }
    // 6to4 2002::/16
    if segs[0] == 0x2002 {
        let embedded = Ipv4Addr::new(
            (segs[1] >> 8) as u8,
            (segs[1] & 0xff) as u8,
            (segs[2] >> 8) as u8,
            (segs[2] & 0xff) as u8,
        );
        return is_blocked_ipv4(embedded);
    }
    false
}

/// Validates an outbound URL before requesting it; blocks non-HTTP(S) schemes and
/// literal blocked IPs (unless `allow_trusted_origin` matches its origin).
pub fn validate_outbound_url(raw: &str, trusted_origin: Option<&str>) -> HomarrResult<Url> {
    let url =
        Url::parse(raw).map_err(|err| HomarrError::BadRequest(format!("Invalid URL: {err}")))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(HomarrError::BadRequest(format!(
            "Refusing to request a {} url",
            url.scheme()
        )));
    }
    let origin = url.origin().ascii_serialization();
    if trusted_origin == Some(origin.as_str()) {
        return Ok(url);
    }
    if let Some(host_str) = url.host_str() {
        let clean = host_str.trim_start_matches('[').trim_end_matches(']');
        if clean.eq_ignore_ascii_case("localhost") || clean.ends_with(".localhost") {
            return Err(HomarrError::BadRequest(format!(
                "Refusing to request {origin} because host is localhost"
            )));
        }
        if let Ok(ip) = clean.parse::<IpAddr>() {
            if is_blocked_ip(ip) {
                return Err(HomarrError::BadRequest(format!(
                    "Refusing to request {origin} because it resolves to blocked address {ip}"
                )));
            }
        }
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_metadata_and_loopback_allows_lan() {
        assert!(is_blocked_ip("127.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("169.254.169.254".parse().unwrap()));
        assert!(is_blocked_ip("::ffff:169.254.169.254".parse().unwrap()));
        assert!(is_blocked_ip("fd00:ec2::254".parse().unwrap()));
        assert!(is_blocked_ip("::1".parse().unwrap()));
        assert!(is_blocked_ip("fe80::1".parse().unwrap()));

        // Homelab private networks stay allowed
        assert!(!is_blocked_ip("192.168.1.50".parse().unwrap()));
        assert!(!is_blocked_ip("10.0.0.15".parse().unwrap()));
        assert!(!is_blocked_ip("172.17.0.2".parse().unwrap()));
        assert!(!is_blocked_ip("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn redacts_query_values() {
        assert_eq!(
            redact_url_query("http://sonarr:8989/api/v3/system/status?apikey=secret123&foo=bar"),
            "http://sonarr:8989/api/v3/system/status?apikey=REDACTED&foo=REDACTED"
        );
    }

    #[test]
    fn validates_trusted_vs_untrusted() {
        assert!(validate_outbound_url("http://169.254.169.254/latest/meta-data", None).is_err());
        assert!(validate_outbound_url("ftp://example.com/file", None).is_err());
        assert!(
            validate_outbound_url("http://127.0.0.1:8080/api", Some("http://127.0.0.1:8080"))
                .is_ok()
        );
        assert!(validate_outbound_url("http://192.168.1.10:8989/api", None).is_ok());
    }
}
