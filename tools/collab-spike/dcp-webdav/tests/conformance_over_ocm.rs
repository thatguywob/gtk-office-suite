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

use common::*;
use dcp_conformance::run_all;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn journeys_over_a_federation() {
    if require("journeys_over_a_federation", &[url_a(), url_b()]).await.is_none() {
        return;
    }
    let report = run_all(&NcFactory::federated().await).await;
    println!("{report}");
    report.assert_skipped_exactly(&[
        "unknown_algorithm_falls_back_to_versions",
        "update_stream_gap_recovery",
    ]);
    // Expected failures are pinned by name (and explained in the results
    // document) rather than tolerated: a failure that appears or disappears
    // fails this test.
    let mut failed = report.failed();
    failed.sort_unstable();
    assert_eq!(failed, EXPECTED_FAILURES, "{report}");
}

/// Journeys that fail over a federation, and why (results document, "Over OCM").
/// Filled in from the measured run: `lease_expiry_releases` needs the renewal
/// and its read-backs to fit in 0.5 s after the lease's own timestamp, and a
/// request through two Nextcloud servers takes longer than that.
const EXPECTED_FAILURES: [&str; 1] = ["lease_expiry_releases"];
