//! Build metadata. Single source of truth for the report header.

pub const NAME: &str = "kibYtMusicLinkUtil";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Phase 1 == offline only. Nothing in this crate opens a socket.
pub const PHASE: u32 = 1;
pub const NETWORK: bool = false;

/// Version of the *segment spec* (not the binary). Bumped when segment
/// semantics change, so archived reports stay interpretable.
pub const SPEC_VERSION: &str = "1.0";
