//! Client for the FAA NOTAM Management Service (NMS) API (DESIGN.md §3,
//! §9.2, §12).

pub mod client;

pub use client::{NotamClient, NotamError, DEFAULT_API_BASE_URL, DEFAULT_AUTH_URL};
