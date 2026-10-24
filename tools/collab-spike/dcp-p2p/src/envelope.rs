//! What a DCP entry carries. Every entry of a document is a p2panda operation whose payload is a
//! space-encrypted JSON `Envelope`; the DCP objects inside are exactly the draft-1 schemas.

use dcp_core::{Lease, Meta, Update, Version};
use p2panda_core::VerifyingKey;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Envelope {
    Meta {
        meta: Meta,
    },
    /// `blob` is the BLAKE3 hash (hex) of the sealed file in iroh-blobs and `key` the 32-byte
    /// file key (hex): the binding's own fields, outside the DCP `Version` object.
    Version {
        version: Version,
        blob: String,
        key: String,
    },
    /// `lamport` orders concurrent lease writes without a clock (see the results document).
    Lease {
        lease: Lease,
        lamport: u64,
    },
    Update {
        update: Update,
        device: String,
    },
}

/// A principal of this binding is the Ed25519 public key as 64 lower-case hex characters. It is
/// the iroh endpoint id, the p2panda author and the p2panda-auth actor at once.
pub fn principal_of(key: &VerifyingKey) -> String {
    key.to_hex()
}

pub fn parse_principal(s: &str) -> Option<VerifyingKey> {
    s.parse().ok()
}

/// The locator a version's file gets. The draft-1 locator table has no content-addressed entry,
/// so the BLAKE3 hash is carried as an `https` URL that is never fetched (see the results
/// document, "Spec corrections").
pub fn blob_locator(blake3_hex: &str) -> dcp_core::Locator {
    dcp_core::Locator::Url {
        href: format!("https://iroh-blobs.invalid/{blake3_hex}"),
    }
}

pub fn blob_of_locator(loc: &dcp_core::Locator) -> Option<String> {
    match loc {
        dcp_core::Locator::Url { href } => href
            .strip_prefix("https://iroh-blobs.invalid/")
            .map(|s| s.to_string()),
        _ => None,
    }
}
