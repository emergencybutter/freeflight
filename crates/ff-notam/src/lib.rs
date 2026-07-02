//! Client for the FAA NOTAM Search API (DESIGN.md §3, §9.2).

pub mod client;

pub use client::{NotamClient, NotamError, DEFAULT_BASE_URL};
