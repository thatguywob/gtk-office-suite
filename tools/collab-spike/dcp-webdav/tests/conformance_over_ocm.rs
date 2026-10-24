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
    // `lease_expiry_releases` is the one journey whose clock a federation strains: it
    // needs the renewal and its read-backs to fit in 0.5 s after the record's own
    // timestamp, and every operation is two server hops. It failed in most
    // federated runs and passed in some; `tests/lease_timing.rs` runs it with the
    // duration as a parameter and requires 20 s to pass. Everything else MUST pass.
    let failed = report.failed();
    assert!(
        failed.iter().all(|n| *n == "lease_expiry_releases"),
        "only lease_expiry_releases may fail over a federation: {failed:?}\n{report}"
    );
}
