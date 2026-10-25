use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;

const RESOLVE_TIMEOUT: Duration = Duration::from_secs(2);

/// Resolve a hostname to its IP addresses. IP literals pass through as-is
/// (no DNS lookup). Used by the quick-connect search to detect saved hosts
/// behind a domain.
#[tauri::command]
pub async fn resolve_host(address: String) -> Result<Vec<String>, String> {
    let address = address.trim().to_lowercase();
    if address.is_empty() {
        return Ok(Vec::new());
    }

    // IP literal: nothing to look up
    if let Ok(ip) = address.parse::<IpAddr>() {
        return Ok(vec![ip.to_string()]);
    }

    let resolved = tokio::time::timeout(RESOLVE_TIMEOUT, tokio::net::lookup_host((address.as_str(), 0)))
        .await
        .map_err(|_| format!("Timed out resolving {address}"))?
        .map_err(|e| format!("Could not resolve {address}: {e}"))?;

    let mut seen = HashSet::new();
    let mut ips = Vec::new();
    for addr in resolved {
        let ip = addr.ip().to_string();
        if seen.insert(ip.clone()) {
            ips.push(ip);
        }
    }
    Ok(ips)
}
