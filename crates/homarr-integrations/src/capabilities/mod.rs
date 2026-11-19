//! Rust mirrors of the TypeScript capability interfaces (`packages/integrations/src/interfaces/**`).
//!
//! All types serialize with `camelCase` field names to match the TS shapes. They are the contract
//! between the integration groups and the consumers of `IntegrationRegistry`. Several of them are
//! not consumed yet (Phase 2 rewires the API/widgets), hence the module level `dead_code` allowance.
//!
//! The older, simplified types in `crate::types` stay for the existing consumers.
#![allow(dead_code)] // Capability types are consumed in later phases.

pub mod calendar;
pub mod dns_hole;
pub mod downloads;
pub mod firewall;
pub mod health_monitoring;
pub mod indexer_manager;
pub mod media_organizer;
pub mod media_releases;
pub mod media_requests;
pub mod media_server;
pub mod media_transcoding;
pub mod network_controller;
pub mod notifications;
pub mod search;
pub mod service_version;
pub mod smart_home;
pub mod stats;
pub mod traefik;
pub mod ups;
pub mod vpn;
