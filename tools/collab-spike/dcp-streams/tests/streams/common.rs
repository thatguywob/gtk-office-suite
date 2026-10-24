use std::sync::Arc;
use std::time::Duration;

use dcp_core::DocumentStore;
use dcp_streams::{Http, Identity, StreamsConfig, StreamsStore};

/// `Some(base url)` when a server is configured. Panics when it is not,
/// unless `DCP_SKIP_STREAMS=1`, in which case it says so and returns `None`.
pub fn server_url(test: &str) -> Option<String> {
    match std::env::var("DCP_STREAMS_URL") {
        Ok(u) if !u.is_empty() => Some(u),
        _ => {
            if std::env::var("DCP_SKIP_STREAMS").as_deref() == Ok("1") {
                eprintln!("SKIPPED {test} (DCP_SKIP_STREAMS=1): no Durable Streams server");
                None
            } else {
                panic!(
                    "DCP_STREAMS_URL is not set: start the reference server (scripts/streams-up.sh) or set DCP_SKIP_STREAMS=1 to skip these tests explicitly"
                );
            }
        }
    }
}

#[macro_export]
macro_rules! require_streams {
    () => {
        match $crate::common::server_url(module_path!()) {
            Some(u) => u,
            None => return,
        }
    };
}

pub fn store_at(base: &str, device: &str) -> Arc<StreamsStore> {
    Arc::new(StreamsStore::new(
        Http::new(base),
        Identity::generate(),
        StreamsConfig::new(device),
    ))
}

pub fn store_with(base: &str, device: &str, id: Identity) -> Arc<StreamsStore> {
    Arc::new(StreamsStore::new(
        Http::new(base),
        id,
        StreamsConfig::new(device),
    ))
}

pub fn dynamic(s: &Arc<StreamsStore>) -> Arc<dyn DocumentStore> {
    s.clone()
}

pub const SETTLE: Duration = Duration::from_secs(10);
