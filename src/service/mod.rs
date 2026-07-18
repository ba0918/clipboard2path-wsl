//! Service layer: domain + infra orchestration.

pub mod converter;
pub mod daemon;
pub mod setup;

#[cfg(test)]
pub(crate) mod test_helpers;
