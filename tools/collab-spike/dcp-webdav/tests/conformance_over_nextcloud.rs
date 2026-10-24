//! The shared conformance journeys (spec section 12, `dcp-conformance`) run
//! against a real Nextcloud: `WebdavStore` over WebDAV and OCS sharing, three
//! accounts (alice owner, bob editor, carol viewer) on one instance.
//!
//! Capabilities are what the binding really does; the journeys that need a
//! capability it lacks are SKIPPED and pinned by `assert_skipped_exactly`, and
//! `docs/rfc/0004-spike-e-results.md` explains each. A test that needs Nextcloud
//! fails when it is missing unless `DCP_SKIP_NEXTCLOUD=1`.

mod common;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use common::*;
use dcp_conformance::wrappers::{Defect, Defective};
use dcp_conformance::{
    run_all, run_one, Capabilities, Failure, Faults, Fixture, Forger, Outcome, RealTime, StoreFactory,
};
use dcp_core::{DocId, DocumentStore, Version};
use dcp_webdav::{ServerClock, WebdavStore};

struct NcFaults(Arc<AtomicBool>);

impl Faults for NcFaults {
    fn truncate_next_upload(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The viewer's client writing straight into the document folder.
struct NcForger {
    viewer: Arc<WebdavStore>,
}

#[async_trait]
impl Forger for NcForger {
    async fn forge_version(&self, doc: &DocId, version: Version) -> Result<Option<String>, Failure> {
        let name = unique("forged");
        Ok(self.viewer.put_raw_version(doc, &name, &version).await?)
    }
}

pub fn capabilities() -> Capabilities {
    Capabilities {
        grant_revoke: true,
        // Revocation is access control on a server that holds the plaintext:
        // content published after it is unreadable to the revoked user, nothing
        // is encrypted.
        revocation: true,
        // The lease log takes any record.
        arbitrary_lease_writes: true,
        // Creating a free name is atomic on Nextcloud, so the log is a real
        // compare-and-set.
        racy_lease: false,
        history: true,
        arbitrary_parents: true,
        // No ordered update stream in WebDAV (L2).
        updates: false,
        upload_faults: true,
        forgery: true,
    }
}

struct NcFactory {
    base: String,
    clock: Arc<ServerClock>,
    hide_newest_from_viewer: bool,
}

impl NcFactory {
    async fn new(hide: bool) -> NcFactory {
        let base = url_a();
        let clock = clock(&base).await;
        NcFactory {
            base,
            clock,
            hide_newest_from_viewer: hide,
        }
    }
}

#[async_trait]
impl StoreFactory for NcFactory {
    fn name(&self) -> String {
        if self.hide_newest_from_viewer {
            "nextcloud with a defective viewer".into()
        } else {
            format!("nextcloud ({})", self.base)
        }
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let fault = Arc::new(AtomicBool::new(false));
        let owner = Arc::new(store(&self.base, "alice", &self.clock, &fault));
        let editor = Arc::new(store(&self.base, "bob", &self.clock, &fault));
        let viewer = Arc::new(store(&self.base, "carol", &self.clock, &fault));
        let viewer_dyn: Arc<dyn DocumentStore> = if self.hide_newest_from_viewer {
            Arc::new(Defective::new(viewer.clone(), Defect::HideNewest))
        } else {
            viewer.clone()
        };
        let mut fx = Fixture::standard(
            owner,
            editor,
            viewer_dyn,
            Arc::new(RealTime),
            capabilities(),
            Some(Arc::new(NcFaults(fault))),
            // Nextcloud is one database: what one handle wrote the next reads.
            Duration::ZERO,
        )
        .await?;
        fx.forger = Some(Arc::new(NcForger { viewer }));
        Ok(fx)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn journeys_over_nextcloud() {
    if require("journeys_over_nextcloud", &[url_a()]).await.is_none() {
        return;
    }
    let report = run_all(&NcFactory::new(false).await).await;
    println!("{report}");
    report.assert_no_failures();
    report.assert_skipped_exactly(&[
        // WebDAV has no ordered stream of small messages: L2 is not offered.
        "unknown_algorithm_falls_back_to_versions",
        "update_stream_gap_recovery",
    ]);
    let mut passed = report.passed();
    passed.sort_unstable();
    assert_eq!(
        passed,
        [
            "convergence_on_one_head",
            "corrupt_file_rejected",
            "forged_version_from_a_viewer_is_not_accepted",
            "lease_exclusion_and_readback",
            "lease_expiry_releases",
            "revocation_hides_new_content",
            "roles_enforced_at_store",
            "second_head_shown_not_hidden",
            "truncated_upload_never_published",
        ]
    );
}

/// Negative control: a viewer's handle that never shows the newest version
/// must fail convergence over Nextcloud as it does everywhere. If it passed,
/// the journeys would not be reading what they claim to read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_journeys_can_fail_over_nextcloud() {
    if require("the_journeys_can_fail_over_nextcloud", &[url_a()]).await.is_none() {
        return;
    }
    let r = run_one(&NcFactory::new(true).await, "convergence_on_one_head").await;
    assert!(matches!(r.outcome, Outcome::Failed(_)), "{:?}", r.outcome);
}

/// The skipped journeys say why they are skipped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skipped_journeys_say_why() {
    if require("skipped_journeys_say_why", &[url_a()]).await.is_none() {
        return;
    }
    let f = NcFactory::new(false).await;
    for name in ["unknown_algorithm_falls_back_to_versions", "update_stream_gap_recovery"] {
        let r = run_one(&f, name).await;
        match r.outcome {
            Outcome::Skipped { missing } => assert_eq!(missing, ["updates"], "{name}"),
            other => panic!("{name}: {other:?}"),
        }
    }
}
