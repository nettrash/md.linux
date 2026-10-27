//! The EPUB cover's https fetch (shell-design §7.5) over libsoup 3 (`webkit::soup`, no new crate):
//! no automatic redirects, at most `CoverReader::MAX_REDIRECTS` followed by hand and only to
//! `https:`, raced against `CoverReader::FETCH_TIMEOUT`; any failure is no cover. Implements
//! md-logic's cover fetch seam (md-logic pending: `export::CoverFetcher`, being ported).
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4
