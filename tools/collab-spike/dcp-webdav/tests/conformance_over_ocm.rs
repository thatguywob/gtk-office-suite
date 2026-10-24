//! The shared conformance journeys over a FEDERATION: the owner is alice on
//! `nc-a`; the editor and the viewer are bob and carol on a different Nextcloud,
//! `nc-b`, and reach the document only through Open Cloud Mesh shares that alice
//! made to `bob@nc-b` and `carol@nc-b` and that each of them accepted on `nc-b`.
//! Every request of the editor and the viewer goes to `nc-b`, which proxies it to
//! `nc-a` with the share's secret.
//!
//! No journey is hand-edited for this: it is the same `run_all` and the same
//! store, pointed at a different server.

mod common;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use common::*;
use dcp_conformance::{
    new_file, run_all, standard_meta, Capabilities, Failure, Faults, Fixture, Forger, RealTime,
    StoreFactory, BASE_FILE,
};
use dcp_core::{DocId, DocumentStore, Role, Version};
use dcp_webdav::ocm::accept_pending_remote_shares;
use dcp_webdav::{ServerClock, WebdavStore};

struct NcFaults(Arc<AtomicBool>);

impl Faults for NcFaults {
    fn truncate_next_upload(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

struct NcForger {
    viewer: Arc<WebdavStore>,
}

#[async_trait]
impl Forger for NcForger {
    async fn forge_version(&self, doc: &DocId, version: Version) -> Result<Option<String>, Failure> {
        Ok(self.viewer.put_raw_version(doc, &unique("forged"), &version).await?)
    }
}

fn capabilities() -> Capabilities {
    Capabilities {
        grant_revoke: true,
        revocation: true,
        arbitrary_lease_writes: true,
        racy_lease: false,
        history: true,
        arbitrary_parents: true,
        updates: false,
        upload_faults: true,
        forgery: true,
    }
}

struct OcmFactory {
    clock_a: Arc<ServerClock>,
    clock_b: Arc<ServerClock>,
}

#[async_trait]
impl StoreFactory for OcmFactory {
    fn name(&self) -> String {
        "nextcloud over OCM (alice@nc-a owner; bob@nc-b editor, carol@nc-b viewer)".into()
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let fault = Arc::new(AtomicBool::new(false));
        let owner = Arc::new(store(&url_a(), "alice", &self.clock_a, &fault));
        let editor = Arc::new(store(&url_b(), "bob", &self.clock_b, &fault));
        let viewer = Arc::new(store(&url_b(), "carol", &self.clock_b, &fault));
        let doc = owner
            .create_document(&standard_meta(), Some(new_file(BASE_FILE)))
            .await?;
        owner.grant(&doc, &editor.principal(), Role::Editor).await?;
        owner.grant(&doc, &viewer.principal(), Role::Viewer).await?;
        // A remote share waits on the receiving server until its user accepts it.
        for (who, dav) in [("bob", editor.dav()), ("carol", viewer.dav())] {
            let n = accept_pending_remote_shares(dav, &doc).await?;
            if n != 2 {
                return Err(Failure::new(format!("{who} had {n} pending shares of {doc}, expected 2")));
            }
        }
        let mut fx = Fixture::from_existing(
            owner,
            editor,
            viewer.clone(),
            doc,
            Arc::new(RealTime),
            capabilities(),
            Some(Arc::new(NcFaults(fault))),
            // Two servers and a notification between them: allow some time.
            Duration::from_secs(5),
        )
        .await?;
        fx.forger = Some(Arc::new(NcForger { viewer }));
        Ok(fx)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn journeys_over_a_federation() {
    if require("journeys_over_a_federation", &[url_a(), url_b()]).await.is_none() {
        return;
    }
    let f = OcmFactory {
        clock_a: clock(&url_a()).await,
        clock_b: clock(&url_b()).await,
    };
    let report = run_all(&f).await;
    println!("{report}");
    // The expectation is written down only after the first run: whatever differs
    // from the single-server result is a finding, listed in the results document.
    report.assert_no_failures();
    report.assert_skipped_exactly(&[
        "unknown_algorithm_falls_back_to_versions",
        "update_stream_gap_recovery",
    ]);
}
