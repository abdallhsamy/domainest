use std::{
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};

use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::{Name, RData, Record, RecordType},
};
use tokio::net::UdpSocket;

use crate::{domain_suffix, error::AppResult};

static DNS_RUNNING: OnceLock<()> = OnceLock::new();
static DNS_SUFFIX: OnceLock<RwLock<String>> = OnceLock::new();
static PROJECT_DOMAINS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
static DISK_DOMAINS_CACHE: OnceLock<RwLock<(Instant, Vec<String>)>> = OnceLock::new();
const DISK_DOMAINS_TTL: Duration = Duration::from_secs(2);

pub const DNS_ADDR: &str = "127.0.0.1:53535";

pub fn set_project_domains(domains: &[String]) {
    let lock = PROJECT_DOMAINS.get_or_init(|| RwLock::new(Vec::new()));
    *lock.write().unwrap() = domains.to_vec();
}

pub fn ensure_running(domain_suffix: &str) -> AppResult<()> {
    let suffix_lock = DNS_SUFFIX.get_or_init(|| RwLock::new("test".to_string()));
    {
        let mut w = suffix_lock.write().unwrap();
        *w = domain_suffix.to_string();
    }
    PROJECT_DOMAINS.get_or_init(|| RwLock::new(Vec::new()));

    // Zone / project list may change while the UDP task is already bound.
    if DNS_RUNNING.get().is_some() {
        return Ok(());
    }

    // If two calls race, only one initializes; the other is fine.
    let _ = DNS_RUNNING.set(());

    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_dns().await {
            log::error!("dns server failed: {e}");
        }
    });

    Ok(())
}

async fn run_dns() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let socket = match UdpSocket::bind(DNS_ADDR).await {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Another Domainest instance owns DNS; quit duplicate apps so this build can serve queries.
            log::warn!(
                "dns port {DNS_ADDR} already in use by another process — quit other Domainest instances and restart"
            );
            return Ok(());
        }
        Err(e) => return Err(Box::new(e)),
    };
    log::info!("dns server listening on {DNS_ADDR}");

    let mut buf = vec![0u8; 2048];
    loop {
        let (len, peer) = socket.recv_from(&mut buf).await?;
        let req = match Message::from_vec(&buf[..len]) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let resp = build_response(req)?;
        let resp_bytes = resp.to_vec()?;
        let _ = socket.send_to(&resp_bytes, peer).await;
    }
}

fn build_response(req: Message) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
    let mut resp = Message::new();
    resp.set_id(req.id());
    resp.set_message_type(MessageType::Response);
    resp.set_op_code(OpCode::Query);
    resp.set_authoritative(true);
    resp.set_response_code(ResponseCode::NoError);

    // Echo queries back.
    for q in req.queries() {
        resp.add_query(q.clone());
        if let Some(answer) = answer_for_query(q)? {
            resp.add_answer(answer);
        }
    }

    Ok(resp)
}

fn answer_for_query(q: &Query) -> Result<Option<Record>, Box<dyn std::error::Error + Send + Sync>> {
    // We only respond to A queries for names under the configured DNS zone.
    if q.query_type() != RecordType::A {
        return Ok(None);
    }

    let name: Name = q.name().clone();
    let name_str = name.to_ascii().to_lowercase();
    let suffix = DNS_SUFFIX
        .get()
        .and_then(|l| l.read().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "test".to_string());
    if !query_matches_dns_config(&name_str, &suffix) {
        return Ok(None);
    }

    let mut record = Record::new();
    record.set_name(name);
    record.set_record_type(RecordType::A);
    record.set_ttl(60);
    record.set_data(Some(RData::A("127.0.0.1".parse()?)));
    Ok(Some(record))
}

fn query_matches_dns_config(name: &str, zone: &str) -> bool {
    if domain_suffix::domain_under_zone(name, zone) {
        return true;
    }
    registered_project_domains()
        .iter()
        .any(|d| domain_suffix::domain_under_zone(name, d))
}

fn registered_project_domains() -> Vec<String> {
    let mut domains = PROJECT_DOMAINS
        .get()
        .map(|l| l.read().unwrap().clone())
        .unwrap_or_default();
    for d in domains_from_disk() {
        if !domains.iter().any(|x| x == &d) {
            domains.push(d);
        }
    }
    domains
}

fn domains_from_disk() -> Vec<String> {
    let cache = DISK_DOMAINS_CACHE.get_or_init(|| {
        RwLock::new((
            Instant::now()
                .checked_sub(DISK_DOMAINS_TTL)
                .unwrap_or_else(Instant::now),
            Vec::new(),
        ))
    });
    {
        let guard = cache.read().unwrap();
        if guard.0.elapsed() < DISK_DOMAINS_TTL {
            return guard.1.clone();
        }
    }

    let loaded = load_domains_from_projects_file().unwrap_or_default();
    *cache.write().unwrap() = (Instant::now(), loaded.clone());
    loaded
}

fn load_domains_from_projects_file() -> Option<Vec<String>> {
    use crate::{models::Project, paths};

    let path = paths::projects_json_path().ok()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let projects: Vec<Project> = serde_json::from_str(&raw).ok()?;
    Some(
        projects
            .into_iter()
            .map(|p| p.domain.trim().to_lowercase())
            .filter(|d| !d.is_empty())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_matching() {
        assert!(query_matches_dns_config("api.myapp.com.", "myapp.com"));
        assert!(!query_matches_dns_config("github.com.", "myapp.com"));
        assert!(query_matches_dns_config("app.test.", "test"));
        set_project_domains(&["be-brand.dev".to_string()]);
        assert!(query_matches_dns_config("be-brand.dev.", "test"));
    }
}
