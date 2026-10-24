//! RFC-0004 Spike B: the Durable Streams binding against a real Durable
//! Streams server (the Node reference server, pinned in
//! `scripts/streams-server/package.json`).
//!
//! Needs `DCP_STREAMS_URL` (for example `http://127.0.0.1:4437/v1/stream/`);
//! every test FAILS without it unless `DCP_SKIP_STREAMS=1`, which prints that
//! it skipped. Tests that start their own server (restart, crash) need
//! `node` and the installed `scripts/streams-server`; they fail without them
//! unless `DCP_SKIP_STREAMS=1`.

mod auth;
mod common;
mod conformance;
mod measure;
mod properties;
mod restart;
mod server;
