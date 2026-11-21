//! The Peppol dispatch status rules (docs/efaktura.md §3.3).
//!
//! Workers may process events in any order, so an attempt's status comes
//! from the rank of its events and never from which arrived last.

use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    SendAttempt,
    NotSent,
    Unresolved,
    Accepted,
    /// The access point reports failure, without proof nothing arrived.
    DeliveryFailed,
    Rejected,
    Delivered,
    MessageResponse,
    InvoiceResponse,
}

impl EventKind {
    /// The value stored in `dispatch_event.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SendAttempt => "sendeforsok",
            Self::NotSent => "ikke_sendt",
            Self::Unresolved => "uavklart",
            Self::Accepted => "akseptert",
            Self::DeliveryFailed => "leveringsfeil",
            Self::Rejected => "avvist",
            Self::Delivered => "levert",
            Self::MessageResponse => "mlr",
            Self::InvoiceResponse => "svar",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "sendeforsok" => Self::SendAttempt,
            "ikke_sendt" => Self::NotSent,
            "uavklart" => Self::Unresolved,
            "akseptert" => Self::Accepted,
            "leveringsfeil" => Self::DeliveryFailed,
            "avvist" => Self::Rejected,
            "levert" => Self::Delivered,
            "mlr" => Self::MessageResponse,
            "svar" => Self::InvoiceResponse,
            _ => return None,
        })
    }

    /// Buyer responses say nothing about whether transport succeeded.
    fn rank(self) -> Option<u8> {
        match self {
            Self::SendAttempt => Some(1),
            Self::NotSent => Some(2),
            Self::Unresolved => Some(3),
            Self::Accepted => Some(4),
            Self::DeliveryFailed => Some(5),
            Self::Rejected => Some(6),
            // Proof of delivery wins: such a document must never be resent.
            Self::Delivered => Some(7),
            Self::MessageResponse | Self::InvoiceResponse => None,
        }
    }

    /// Whether the next attempt can start without a duplicate check.
    pub fn is_safe_to_retry(self) -> bool {
        matches!(self, Self::NotSent | Self::Rejected)
    }
}

/// Maps the access point's delivery report. Only a failure that proves the
/// buyer did not take the document is a rejection, which may be resent.
pub fn delivery_event(status: Option<&str>, failure_category: Option<&str>) -> Option<EventKind> {
    match status {
        Some("delivered") => Some(EventKind::Delivered),
        Some("failed") => Some(match failure_category {
            Some("recipient_not_found")
            | Some("document_not_supported")
            | Some("validation")
            | Some("recipient_rejected") => EventKind::Rejected,
            _ => EventKind::DeliveryFailed,
        }),
        _ => None,
    }
}

/// One attempt's computed transport status.
pub fn attempt_status(events: impl IntoIterator<Item = EventKind>) -> Option<EventKind> {
    events
        .into_iter()
        .filter(|kind| kind.rank().is_some())
        .max_by_key(|kind| kind.rank())
}

/// What the public Peppol directory says about a receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryAnswer {
    Supported,
    /// The SML holds no entry for the participant.
    Absent,
    /// The participant exists but is not published for this document.
    MissingDocumentType,
    /// No usable answer, which says nothing about registration.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Peppol,
    Email,
    /// No send: e-mail could break the e-invoice duty.
    Hold,
}

/// The channel table in docs/efaktura.md §3.2. `provider` is the access
/// point's answer, `None` when it did not answer. `directory` is only
/// consulted when the provider did not confirm support.
pub fn choose_channel(provider: Option<bool>, directory: Option<DirectoryAnswer>) -> Channel {
    match (provider, directory) {
        (Some(true), _) => Channel::Peppol,
        (Some(false), Some(DirectoryAnswer::Absent | DirectoryAnswer::MissingDocumentType)) => {
            Channel::Email
        }
        _ => Channel::Hold,
    }
}

pub fn own_event_key(dispatch_id: impl Display, attempt: i32, kind: EventKind) -> String {
    format!("regnmed:{dispatch_id}:{attempt}:{}", kind.as_str())
}

pub fn provider_event_key(document_id: &str, kind: EventKind) -> String {
    format!("recommand:{document_id}:{}", kind.as_str())
}

#[cfg(test)]
mod tests {
    use super::EventKind::*;
    use super::*;

    #[test]
    fn status_follows_rank_not_arrival() {
        assert_eq!(
            attempt_status([Delivered, Accepted, SendAttempt]),
            Some(Delivered)
        );
        assert_eq!(
            attempt_status([SendAttempt, Accepted, Delivered]),
            Some(Delivered)
        );
        assert_eq!(attempt_status([Rejected, Accepted]), Some(Rejected));
        assert_eq!(attempt_status([NotSent, SendAttempt]), Some(NotSent));
        assert_eq!(attempt_status([NotSent, Unresolved]), Some(Unresolved));
    }

    #[test]
    fn buyer_responses_do_not_change_transport_status() {
        assert_eq!(attempt_status([Accepted, MessageResponse]), Some(Accepted));
        assert_eq!(attempt_status([InvoiceResponse]), None);
    }

    #[test]
    fn only_proven_non_delivery_can_retry_without_a_check() {
        for kind in [SendAttempt, Unresolved, Accepted, DeliveryFailed, Delivered] {
            assert!(!kind.is_safe_to_retry(), "{}", kind.as_str());
        }
        assert!(NotSent.is_safe_to_retry());
        assert!(Rejected.is_safe_to_retry());
    }

    #[test]
    fn an_ambiguous_delivery_failure_is_not_safe_to_resend() {
        assert_eq!(delivery_event(Some("pending"), None), None);
        assert_eq!(delivery_event(Some("delivered"), None), Some(Delivered));
        assert_eq!(
            delivery_event(Some("failed"), Some("validation")),
            Some(Rejected)
        );
        assert_eq!(
            delivery_event(Some("failed"), Some("transport")),
            Some(DeliveryFailed)
        );
        assert_eq!(delivery_event(Some("failed"), None), Some(DeliveryFailed));
        assert_eq!(
            attempt_status([Accepted, DeliveryFailed]),
            Some(DeliveryFailed)
        );
        assert_eq!(attempt_status([Unresolved, Accepted]), Some(Accepted));
    }

    #[test]
    fn stored_kinds_round_trip() {
        for kind in [
            SendAttempt,
            NotSent,
            Unresolved,
            Accepted,
            DeliveryFailed,
            Rejected,
            Delivered,
            MessageResponse,
            InvoiceResponse,
        ] {
            assert_eq!(EventKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(EventKind::parse("sent"), None);
    }

    #[test]
    fn email_needs_proof_that_the_buyer_cannot_receive() {
        use DirectoryAnswer::*;
        assert_eq!(choose_channel(Some(true), None), Channel::Peppol);
        assert_eq!(choose_channel(Some(false), Some(Absent)), Channel::Email);
        assert_eq!(
            choose_channel(Some(false), Some(MissingDocumentType)),
            Channel::Email
        );
        assert_eq!(choose_channel(Some(false), Some(Supported)), Channel::Hold);
        assert_eq!(choose_channel(Some(false), Some(Unknown)), Channel::Hold);
        assert_eq!(choose_channel(Some(false), None), Channel::Hold);
        assert_eq!(choose_channel(None, Some(Absent)), Channel::Hold);
    }

    #[test]
    fn event_keys_are_deterministic() {
        assert_eq!(own_event_key("d1", 2, NotSent), "regnmed:d1:2:ikke_sendt");
        assert_eq!(
            provider_event_key("doc_1", Delivered),
            "recommand:doc_1:levert"
        );
    }
}
