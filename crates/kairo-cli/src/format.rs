//! Formatting helpers for `VerificationReport` and its sub-status enums.
//! Used by the verify-signature / verify-object / verify-actor-genesis
//! commands; consolidated here so the text and `--json` shapes are
//! defined in one place.

use kairo_statement::verify::{
    ActorResolution, SignatureStatus, TrustEvaluation, VerificationReport,
};
use kairo_statement::ObjectRevisionBody;

pub(crate) fn format_verification_report(
    revision: &ObjectRevisionBody,
    report: &VerificationReport,
) -> String {
    format!(
        "valid revision actor genesis\n\
         object = {}\n\
         revision = {}\n\
         actor = {}\n\
         statement_id = {}\n\
         signature = {}\n\
         actor_resolution = {}\n\
         trust = {}\n",
        revision.object(),
        revision.revision().as_str(),
        report.envelope_actor,
        report.statement_id,
        format_signature_status(&report.signature),
        format_actor_resolution(&report.actor),
        format_trust(&report.trust),
    )
}

pub(crate) fn format_verification_report_json(
    revision: &ObjectRevisionBody,
    report: &VerificationReport,
) -> String {
    let mut signature = serde_json::Map::new();
    signature.insert(
        "status".to_owned(),
        serde_json::Value::String(format_signature_status(&report.signature).to_owned()),
    );
    match &report.signature {
        SignatureStatus::UnsupportedAlgorithm(algorithm) => {
            signature.insert(
                "algorithm".to_owned(),
                serde_json::Value::String(algorithm.clone()),
            );
        }
        SignatureStatus::Malformed {
            expected_len,
            actual_len,
        } => {
            signature.insert(
                "expected_len".to_owned(),
                serde_json::Value::Number((*expected_len).into()),
            );
            signature.insert(
                "actual_len".to_owned(),
                serde_json::Value::Number((*actual_len).into()),
            );
        }
        _ => {}
    }

    let mut actor = serde_json::Map::new();
    actor.insert(
        "status".to_owned(),
        serde_json::Value::String(format_actor_resolution(&report.actor).to_owned()),
    );
    if let ActorResolution::ResolverUnavailable(reason) = &report.actor {
        actor.insert(
            "reason".to_owned(),
            serde_json::Value::String(reason.clone()),
        );
    }

    let value = serde_json::json!({
        "statement_id": report.statement_id.to_string(),
        "envelope_actor": report.envelope_actor.to_string(),
        "signature_actor": report.signature_actor.to_string(),
        "object": revision.object().to_string(),
        "revision": revision.revision().as_str(),
        "signature": serde_json::Value::Object(signature),
        "actor": serde_json::Value::Object(actor),
        "trust": format_trust(&report.trust),
        "cryptographically_valid": report.is_cryptographically_valid(),
    });
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    output.push('\n');
    output
}

pub(crate) fn format_signature_status(status: &SignatureStatus) -> &'static str {
    match status {
        SignatureStatus::Valid => "valid",
        SignatureStatus::Invalid => "invalid",
        SignatureStatus::UnsupportedAlgorithm(_) => "unsupported-algorithm",
        SignatureStatus::Malformed { .. } => "malformed",
        SignatureStatus::AlgorithmMismatch => "algorithm-mismatch",
        SignatureStatus::KeyMismatch { .. } => "key-mismatch",
        SignatureStatus::KeyRevoked => "key-revoked",
        SignatureStatus::NoActiveKey => "no-active-key",
        SignatureStatus::NotInAttestationSet { .. } => "not-in-attestation-set",
        SignatureStatus::BelowThreshold { .. } => "below-threshold",
        SignatureStatus::NotEvaluated => "not-evaluated",
    }
}

pub(crate) fn format_actor_resolution(resolution: &ActorResolution) -> &'static str {
    match resolution {
        ActorResolution::Resolved => "resolved",
        ActorResolution::NotFound => "not-found",
        ActorResolution::ResolverUnavailable(_) => "resolver-unavailable",
        ActorResolution::SignatureActorMismatch => "signature-actor-mismatch",
    }
}

pub(crate) fn format_trust(trust: &TrustEvaluation) -> &'static str {
    match trust {
        TrustEvaluation::Trusted => "trusted",
        TrustEvaluation::Untrusted => "untrusted",
        TrustEvaluation::Unknown => "unknown",
        TrustEvaluation::Unevaluated => "unevaluated",
    }
}
