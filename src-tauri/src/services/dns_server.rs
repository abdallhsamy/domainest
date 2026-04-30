use std::sync::{OnceLock, RwLock};

use hickory_proto::{
  op::{Message, MessageType, OpCode, Query, ResponseCode},
  rr::{Name, RData, Record, RecordType},
};
use tokio::net::UdpSocket;

use crate::error::AppResult;

static DNS_RUNNING: OnceLock<()> = OnceLock::new();
static DNS_SUFFIX: OnceLock<RwLock<String>> = OnceLock::new();

pub const DNS_ADDR: &str = "127.0.0.1:53535";

pub fn ensure_running(domain_suffix: &str) -> AppResult<()> {
  let suffix_lock = DNS_SUFFIX.get_or_init(|| RwLock::new("test".to_string()));
  {
    let mut w = suffix_lock.write().unwrap();
    *w = domain_suffix.to_string();
  }

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
      // Another instance is already bound (e.g. dev hot-reload). Treat as already running.
      log::info!("dns server already listening on {DNS_ADDR}");
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
  // We only respond to A queries for *.test.
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
  let suffix_dot = format!(".{}.", suffix.to_lowercase().trim_start_matches('.'));
  let root = format!("{}.", suffix.to_lowercase().trim_start_matches('.'));

  if !name_str.ends_with(&suffix_dot) && name_str != root {
    return Ok(None);
  }

  let mut record = Record::new();
  record.set_name(name);
  record.set_record_type(RecordType::A);
  record.set_ttl(60);
  record.set_data(Some(RData::A("127.0.0.1".parse()?)));
  Ok(Some(record))
}

