//! APEX core: platform-independent optimization engine.
//!
//! * [`recommend`] — deterministic, hardware-aware recommendations
//! * [`executor`] — transactional apply / verify / rollback / undo / crash recovery
//! * [`ledger`] — SQLite audit log and benchmark history
//! * [`bench`] — benchmark workloads and statistical comparison
//! * [`storage`] — safe temp cleanup, large-file and duplicate analysis
//! * [`net`] — DNS and TCP latency measurement
//! * [`safety`] — protected processes and startup entries

pub mod actions;
pub mod bench;
pub mod executor;
pub mod ledger;
pub mod model;
pub mod net;
pub mod recommend;
pub mod safety;
pub mod storage;
