//! Service layer: domain + infra orchestration.

pub mod converter;
pub mod daemon;

#[cfg(test)]
pub(crate) mod test_helpers;
