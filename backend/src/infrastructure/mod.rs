//! Infrastructure: connections to the concrete external systems the fund runs
//! on. Scaffold — each module opens a client/pool and hands it back; domain
//! mapping (repositories, gateways) is layered on top as features land.

pub mod db;
pub mod tigerbeetle;
