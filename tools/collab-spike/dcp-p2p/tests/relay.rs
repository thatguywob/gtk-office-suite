//! Two nodes that can only reach each other through a relay (iroh's in-process relay server, TLS
//! with a self-signed certificate that the test endpoints accept). Needs `--features relay-test`.
#![cfg(feature = "relay-test")]

mod common;

use std::time::{Duration, Instant};

use common::*;
use dcp_conformance::{new_file, standard_meta};
use dcp_core::{DocumentStore, Role};
use iroh::EndpointAddr;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nodes_reach_each_other_through_a_relay_and_sync_a_document() {
    init_logging();
    let (map, url, _server) = iroh::test_utils::run_relay_server().await.expect("a relay server");
    let mut c = cfg();
    c.relay = Some(map);
    let a = node(c.clone()).await;
    let b = node(c).await;
    // Addresses that carry the relay URL and no direct address: the first packets can only go
    // through the relay.
    let via_relay = |n: &std::sync::Arc<dcp_p2p::P2pStore>| EndpointAddr::new(n.addr().id).with_relay_url(url.clone());
    a.add_peer(via_relay(&b)).await.unwrap();
    b.add_peer(via_relay(&a)).await.unwrap();
    let doc = a.create_document(&standard_meta(), Some(new_file(b"through a relay"))).await.unwrap();
    invite(&a, &b, &doc).await;
    a.grant(&doc, &b.principal(), Role::Viewer).await.unwrap();
    let t = Instant::now();
    wait_for("b reads the document through the relay", Duration::from_secs(60), || async {
        let v = b.versions(&doc).await.ok()?;
        (v.len() == 1).then_some(())
    })
    .await;
    eprintln!("MEASURE relay-only addresses: from the grant until the viewer lists the version: {} ms", t.elapsed().as_millis());
    let v = b.versions(&doc).await.unwrap().remove(0);
    assert_eq!(b.fetch_file(&doc, &v).await.unwrap(), b"through a relay");
    // A cold session over a fresh connection, timed.
    let t = Instant::now();
    let s = b.sync_now(&doc, &a.principal()).await.unwrap();
    eprintln!("MEASURE sync session through the relay: {} us, moved {} / {} operations", t.elapsed().as_micros(), s.received_operations, s.sent_operations);
}
