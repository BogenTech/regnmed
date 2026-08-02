//! The outbound-mail rail moved to its own crate (#75) so the CLI's
//! abonnement automation can publish too — the wire contract with
//! regnid has exactly one copy. This module remains as the API's name
//! for it.

pub use regnmed_mail::*;
