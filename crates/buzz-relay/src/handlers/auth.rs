//! NIP-42 AUTH handler — verify challenge response, transition auth state.
//!
//! Relay membership enforcement uses the shared
//! [`crate::api::relay_members::enforce_relay_membership`] helper, which supports
//! NIP-OA owner-delegation fallback on closed relays. The auth handler also
//! verifies a presented NIP-OA credential for an already-admitted direct member,
//! so direct membership cannot suppress agent→owner materialization required by
//! observer-frame authorization. On open relays it performs the same extraction
//! without using NIP-OA as an admission gate.
//!
//! For WebSocket auth, the NIP-OA `auth` tag is extracted from the signed AUTH
//! event itself (the tag is integrity-protected by the event signature).

use std::sync::Arc;

use axum::extract::ws::Message as WsMessage;
use tracing::{debug, info, warn};

use crate::connection::{AuthState, ConnectionState};
use crate::metrics::{AuthOutcome, AuthPostTerminalState};
use crate::protocol::RelayMessage;
use crate::state::AppState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BanOutcome {
    Clear,
    Banned,
    DbError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PolicyCheck<T> {
    Allowed(T),
    Denied,
    DependencyError,
}

fn classify_allowlist<E>(result: Result<bool, E>) -> PolicyCheck<()> {
    match result {
        Ok(true) => PolicyCheck::Allowed(()),
        Ok(false) => PolicyCheck::Denied,
        Err(_) => PolicyCheck::DependencyError,
    }
}

fn classify_relay_membership(
    result: Result<crate::api::relay_members::MembershipDecision, String>,
) -> PolicyCheck<Option<nostr::PublicKey>> {
    use crate::api::relay_members::MembershipDecision;

    match result {
        Ok(MembershipDecision::OpenRelay | MembershipDecision::Member) => {
            PolicyCheck::Allowed(None)
        }
        Ok(MembershipDecision::ViaOwner(owner)) => PolicyCheck::Allowed(Some(owner)),
        Ok(MembershipDecision::Denied) => PolicyCheck::Denied,
        Err(_) => PolicyCheck::DependencyError,
    }
}

/// Resolve the authenticated NIP-OA owner after relay admission succeeds.
///
/// `membership_owner` is populated when a closed relay admitted the agent via
/// its owner. A direct member has already passed admission, but a valid owner
/// credential still carries authorization metadata that must be materialized
/// for observer frames and other owner-scoped operations. Closed relays honor
/// that metadata only when NIP-OA auth is enabled; open relays preserve their
/// existing opportunistic owner-discovery behavior.
fn resolve_authenticated_owner(
    membership_owner: Option<nostr::PublicKey>,
    require_relay_membership: bool,
    allow_nip_oa_auth: bool,
    auth_tag_json: Option<&str>,
    agent_pubkey: &nostr::PublicKey,
    signed_auth_created_at: u64,
) -> Option<nostr::PublicKey> {
    membership_owner.or_else(|| {
        let should_extract =
            auth_tag_json.is_some() && (!require_relay_membership || allow_nip_oa_auth);
        if !should_extract {
            return None;
        }
        crate::api::relay_members::extract_nip_oa_owner(
            agent_pubkey.as_bytes(),
            auth_tag_json,
            Some(signed_auth_created_at),
        )
    })
}

fn ban_denial(outcome: BanOutcome) -> Option<(&'static str, &'static str, AuthOutcome)> {
    match outcome {
        BanOutcome::Clear => None,
        BanOutcome::Banned => Some((
            "banned",
            "blocked: you are banned from this community",
            AuthOutcome::Banned,
        )),
        BanOutcome::DbError => Some((
            "ban_check_error",
            "error: internal error checking restriction state",
            AuthOutcome::BanCheckError,
        )),
    }
}

/// Extract a NIP-OA `auth` tag from a verified AUTH event and serialize it as
/// the JSON-array string that [`buzz_sdk::nip_oa::verify_auth_tag`] expects.
///
/// Returns `None` if no `auth` tag is present (direct-member auth path) or if
/// more than one `auth` tag exists (per NIP-OA spec: >1 auth tag ⇒ no valid tag).
pub fn extract_auth_tag_json(event: &nostr::Event) -> Option<String> {
    let mut iter = event
        .tags
        .iter()
        .filter(|t| t.as_slice().first().map(|s| s.as_str()) == Some("auth"));
    let first = iter.next()?;
    if iter.next().is_some() {
        return None; // NIP-OA spec: treat >1 auth tag as no valid auth tag
    }
    serde_json::to_string(first.as_slice()).ok()
}

/// Handle a NIP-42 AUTH message: verify the challenge response and transition
/// the connection to authenticated state.
///
/// Pure crypto verification — no API tokens, no JWT, no DB token lookups.
#[tracing::instrument(skip_all, fields(event_id, conn_id))]
pub async fn handle_auth(event: nostr::Event, conn: Arc<ConnectionState>, state: Arc<AppState>) {
    let event_id_hex = event.id.to_hex();
    let (challenge, conn_id) = {
        match conn.auth_state_snapshot() {
            AuthState::Pending { challenge, .. } => (challenge, conn.conn_id),
            AuthState::Authenticated(_) => {
                debug!(conn_id = %conn.conn_id, "AUTH received but already authenticated");
                crate::metrics::record_post_terminal_auth_frame(
                    AuthPostTerminalState::Authenticated,
                );
                conn.send(RelayMessage::ok(
                    &event_id_hex,
                    false,
                    "auth-required: already authenticated",
                ));
                return;
            }
            AuthState::Failed => {
                debug!(conn_id = %conn.conn_id, "AUTH received after failed auth");
                crate::metrics::record_post_terminal_auth_frame(AuthPostTerminalState::Failed);
                conn.send(RelayMessage::ok(
                    &event_id_hex,
                    false,
                    "auth-required: authentication already failed",
                ));
                return;
            }
        }
    };

    // Record the declared span fields now that we have the values.
    tracing::Span::current()
        .record("event_id", event_id_hex.as_str())
        .record("conn_id", conn_id.to_string().as_str());

    // Extract the NIP-OA auth tag before verification consumes the event.
    // The tag is integrity-protected by the event's Schnorr signature — if
    // tampered, NIP-42 verification will fail before we ever inspect it.
    let auth_tag_json = extract_auth_tag_json(&event);
    let signed_auth_created_at = event.created_at.as_secs();

    let relay_url =
        crate::api::bridge::nip42_expected_relay_url(&state.config.relay_url, &conn.tenant);
    let auth_svc = Arc::clone(&state.auth);

    // Pure NIP-42 verification — crypto only, no DB lookups.
    match auth_svc
        .verify_auth_event(event, &challenge, &relay_url)
        .await
    {
        Ok(mut auth_ctx) => {
            let pubkey = auth_ctx.pubkey;

            // Community ban gate (NIP-42 seam). Runs immediately after auth
            // verification succeeds and before the allowlist and relay-membership
            // gates, per COMMUNITY_MODERATION_PLAN.md §0 decision 4 and the
            // MOD-7/M20 invariant (a ban must block connection auth even for open
            // channels — enforcement is structural, not filtered later). A banned
            // principal gets the standard protocol denial and the connection is
            // dropped with zero further processing.
            //
            // NIP-OA cascade: a ban on the authenticated pubkey blocks it directly;
            // a ban on its cryptographically-proven owner cascades to the agent
            // (owner ban ⇒ agents banned; agent ban is agent-only). The owner is
            // extracted from the self-proving auth tag with no DB round-trip.
            {
                // Fail closed on a DB error, but distinguish it from a real ban:
                // a transient blip must deny (never let a banned principal
                // through) without telling an innocent user they are banned and
                // pinning `Failed` for the connection's life on a false premise.
                // `Banned` claims the ban; `DbError` denies with `error: internal`
                // (mirrors the ingest write-path gate).
                let mut outcome = match state
                    .db
                    .moderation_restriction_state(conn.tenant.community(), pubkey.as_bytes())
                    .await
                {
                    Ok(state) if state.banned => BanOutcome::Banned,
                    Ok(_) => BanOutcome::Clear,
                    Err(e) => {
                        warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), error = %e,
                              "ban-state DB lookup failed, denying (fail-closed)");
                        BanOutcome::DbError
                    }
                };

                // Cascade: check the proven NIP-OA owner only if the agent itself
                // is clear (a DB error already denies; a direct ban already blocks
                // — both skip the needless second DB read).
                if matches!(outcome, BanOutcome::Clear) {
                    if let Some(owner) = crate::api::relay_members::extract_nip_oa_owner(
                        pubkey.as_bytes(),
                        auth_tag_json.as_deref(),
                        Some(signed_auth_created_at),
                    ) {
                        outcome = match state
                            .db
                            .moderation_restriction_state(conn.tenant.community(), owner.as_bytes())
                            .await
                        {
                            Ok(state) if state.banned => BanOutcome::Banned,
                            Ok(_) => BanOutcome::Clear,
                            Err(e) => {
                                warn!(conn_id = %conn_id, owner = %owner.to_hex(), error = %e,
                                      "owner ban-state DB lookup failed, denying (fail-closed)");
                                BanOutcome::DbError
                            }
                        };
                    }
                }

                if let Some((metric_reason, deny_reason, auth_outcome)) = ban_denial(outcome) {
                    warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), reason = deny_reason, "principal denied at ban seam");
                    metrics::counter!("buzz_auth_failures_total", "reason" => metric_reason)
                        .increment(1);
                    if !conn.reject_auth(auth_outcome) {
                        return;
                    }
                    // Decision 4: banned ⇒ OK false + immediate WebSocket close.
                    // Route the reason frame on the control channel (not `send`,
                    // which uses the data channel and would race the cancel), so
                    // the send loop drains it ahead of the Close it emits on
                    // cancel. Then cancel to close the socket immediately.
                    let _ = conn.ctrl_tx.try_send(WsMessage::Text(
                        RelayMessage::ok(&event_id_hex, false, deny_reason).into(),
                    ));
                    conn.cancel.cancel();
                    return;
                }
            }

            // Pubkey allowlist gate — only for pubkey-only auth.
            if state.config.pubkey_allowlist_enabled
                && auth_ctx.auth_method == buzz_auth::AuthMethod::Nip42
            {
                let allowlist = state
                    .db
                    .is_pubkey_allowed(conn.tenant.community(), pubkey.as_bytes())
                    .await;
                if let Err(e) = &allowlist {
                    warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), error = %e,
                              "allowlist DB lookup failed, denying (fail-closed)");
                }
                match classify_allowlist(allowlist) {
                    PolicyCheck::Allowed(()) => {}
                    PolicyCheck::Denied => {
                        warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), "pubkey not in allowlist");
                        metrics::counter!("buzz_auth_failures_total", "reason" => "allowlist_denied")
                            .increment(1);
                        if !conn.reject_auth(AuthOutcome::AllowlistDenied) {
                            return;
                        }
                        conn.send(RelayMessage::ok(
                            &event_id_hex,
                            false,
                            "auth-required: verification failed",
                        ));
                        return;
                    }
                    PolicyCheck::DependencyError => {
                        metrics::counter!("buzz_auth_failures_total", "reason" => "allowlist_check_error")
                            .increment(1);
                        if !conn.reject_auth(AuthOutcome::AllowlistCheckError) {
                            return;
                        }
                        conn.send(RelayMessage::ok(
                            &event_id_hex,
                            false,
                            "error: internal error checking allowlist",
                        ));
                        return;
                    }
                }
            }

            // Relay membership gate — uses the shared helper with NIP-OA fallback.
            let membership = crate::api::relay_members::check_relay_membership(
                &state,
                conn.tenant.community(),
                pubkey.as_bytes(),
                auth_tag_json.as_deref(),
                Some(signed_auth_created_at),
            )
            .await;
            if let Err(e) = &membership {
                warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), error = %e,
                    "relay membership DB lookup failed, denying (fail-closed)");
            }
            let nip_oa_owner = match classify_relay_membership(membership) {
                PolicyCheck::Allowed(owner) => owner,
                PolicyCheck::Denied => {
                    warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), "not a relay member");
                    metrics::counter!("buzz_auth_failures_total", "reason" => "not_relay_member")
                        .increment(1);
                    if !conn.reject_auth(AuthOutcome::NotRelayMember) {
                        return;
                    }
                    conn.send(RelayMessage::ok(
                        &event_id_hex,
                        false,
                        "restricted: not a relay member",
                    ));
                    return;
                }
                PolicyCheck::DependencyError => {
                    metrics::counter!("buzz_auth_failures_total", "reason" => "relay_membership_check_error")
                        .increment(1);
                    if !conn.reject_auth(AuthOutcome::RelayMembershipCheckError) {
                        return;
                    }
                    conn.send(RelayMessage::ok(
                        &event_id_hex,
                        false,
                        "error: internal error checking relay membership",
                    ));
                    return;
                }
            };

            // Resolve owner metadata independently from the admission route.
            // A direct member may still be an agent presenting a valid NIP-OA
            // credential; skipping it here leaves `agent_owner_pubkey` unset and
            // makes every kind:24200 frame fail authorization.
            let nip_oa_owner = resolve_authenticated_owner(
                nip_oa_owner,
                state.config.require_relay_membership,
                state.config.allow_nip_oa_auth,
                auth_tag_json.as_deref(),
                &pubkey,
                signed_auth_created_at,
            );

            // Stash NIP-OA owner on the auth context only after the shared
            // backfill confirms the first-write-wins relationship.
            if let Some(owner) = nip_oa_owner {
                if crate::api::relay_members::materialize_nip_oa_owner(
                    &state,
                    &conn.tenant,
                    &pubkey,
                    &owner,
                )
                .await
                {
                    auth_ctx.agent_owner_pubkey = Some(owner);
                } else {
                    warn!(
                        conn_id = %conn_id,
                        agent = %pubkey.to_hex(),
                        nip_oa_owner = %owner.to_hex(),
                        "NIP-OA owner could not be materialized"
                    );
                }
            }

            info!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), "NIP-42 auth successful");
            if !conn.authenticate(auth_ctx) {
                return;
            }
            state
                .conn_manager
                .set_authenticated_pubkey(conn_id, pubkey.to_bytes().to_vec());
            conn.send(RelayMessage::ok(&event_id_hex, true, ""));
        }
        Err(e) => {
            warn!(conn_id = %conn_id, error = %e, "NIP-42 auth failed");
            metrics::counter!("buzz_auth_failures_total", "reason" => "nip42_invalid").increment(1);
            if !conn.reject_auth(AuthOutcome::Invalid) {
                return;
            }
            conn.send(RelayMessage::ok(
                &event_id_hex,
                false,
                "auth-required: verification failed",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ban_denial, classify_allowlist, classify_relay_membership, extract_auth_tag_json,
        handle_auth, resolve_authenticated_owner, BanOutcome, PolicyCheck,
    };
    use crate::api::relay_members::MembershipDecision;
    use crate::connection::{tests::test_conn_with_auth, AuthState};
    use crate::metrics::{AuthOutcome, AuthPostTerminalState};
    use metrics_util::debugging::DebugValue;
    use nostr::{EventBuilder, Keys, Kind, RelayUrl, Tag};
    use std::time::Instant;

    type MetricSnapshot = Vec<(
        metrics_util::CompositeKey,
        Option<metrics::Unit>,
        Option<metrics::SharedString>,
        DebugValue,
    )>;

    fn metric_counter(snapshot: &MetricSnapshot, name: &str, outcome: Option<&str>) -> u64 {
        snapshot
            .iter()
            .find_map(|(key, _, _, value)| {
                if key.key().name() != name {
                    return None;
                }
                let labels = key.key().labels().collect::<Vec<_>>();
                if outcome.is_some_and(|expected| {
                    !labels
                        .iter()
                        .any(|label| label.key() == "outcome" && label.value() == expected)
                }) {
                    return None;
                }
                let DebugValue::Counter(value) = value else {
                    panic!("{name} must be a counter");
                };
                Some(*value)
            })
            .unwrap_or_default()
    }

    fn pending(challenge: &str) -> AuthState {
        AuthState::Pending {
            challenge: challenge.to_owned(),
            started_at: Instant::now(),
        }
    }

    #[test]
    fn direct_member_nip_oa_credential_resolves_owner_on_closed_relay() {
        let owner = Keys::generate();
        let agent = Keys::generate();
        let auth_tag = buzz_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "")
            .expect("compute auth tag");

        let resolved = resolve_authenticated_owner(
            None,
            true,
            true,
            Some(&auth_tag),
            &agent.public_key(),
            nostr::Timestamp::now().as_secs(),
        );

        assert_eq!(resolved, Some(owner.public_key()));
    }

    #[test]
    fn closed_relay_with_nip_oa_disabled_does_not_resolve_direct_member_owner() {
        let owner = Keys::generate();
        let agent = Keys::generate();
        let auth_tag = buzz_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "")
            .expect("compute auth tag");

        assert_eq!(
            resolve_authenticated_owner(
                None,
                true,
                false,
                Some(&auth_tag),
                &agent.public_key(),
                nostr::Timestamp::now().as_secs(),
            ),
            None,
        );
    }

    #[test]
    fn open_relay_preserves_opportunistic_nip_oa_owner_discovery() {
        let owner = Keys::generate();
        let agent = Keys::generate();
        let auth_tag = buzz_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "")
            .expect("compute auth tag");

        assert_eq!(
            resolve_authenticated_owner(
                None,
                false,
                false,
                Some(&auth_tag),
                &agent.public_key(),
                nostr::Timestamp::now().as_secs(),
            ),
            Some(owner.public_key()),
        );
    }

    /// Build a signed NIP-98 (kind 27235) event carrying the given tags. The
    /// `auth` tag lives inside the signed event exactly as the git and
    /// WebSocket auth paths receive it.
    fn signed_event_with_tags(tags: Vec<Tag>) -> nostr::Event {
        EventBuilder::new(Kind::HttpAuth, "")
            .tags(tags)
            .sign_with_keys(&Keys::generate())
            .expect("sign auth event")
    }

    /// A single `auth` tag is extracted verbatim as its JSON-array string —
    /// this is the exact value fed to `verify_auth_tag` on the git path.
    #[test]
    fn single_auth_tag_extracted_verbatim() {
        let owner = Keys::generate().public_key().to_hex();
        let sig = "00".repeat(64);
        let event = signed_event_with_tags(vec![
            Tag::parse(["u", "https://relay/git/x/y"]).unwrap(),
            Tag::parse(["auth", owner.as_str(), "", sig.as_str()]).unwrap(),
        ]);

        let extracted = extract_auth_tag_json(&event).expect("auth tag present");
        let expected = serde_json::to_string(&["auth", owner.as_str(), "", sig.as_str()]).unwrap();
        assert_eq!(extracted, expected);
    }

    /// No `auth` tag → `None` (the direct-member path, tag absent).
    #[test]
    fn no_auth_tag_returns_none() {
        let event =
            signed_event_with_tags(vec![Tag::parse(["u", "https://relay/git/x/y"]).unwrap()]);
        assert_eq!(extract_auth_tag_json(&event), None);
    }

    /// More than one `auth` tag → `None`. Per NIP-OA, an ambiguous set of
    /// attestations is treated as no valid attestation (fail-closed), so a
    /// second forged tag cannot smuggle an alternate delegation past the gate.
    #[test]
    fn duplicate_auth_tags_return_none() {
        let a = Keys::generate().public_key().to_hex();
        let b = Keys::generate().public_key().to_hex();
        let sig = "00".repeat(64);
        let event = signed_event_with_tags(vec![
            Tag::parse(["auth", a.as_str(), "", sig.as_str()]).unwrap(),
            Tag::parse(["auth", b.as_str(), "", sig.as_str()]).unwrap(),
        ]);
        assert_eq!(extract_auth_tag_json(&event), None);
    }

    #[test]
    fn ban_decisions_map_to_bounded_public_outcomes() {
        assert_eq!(ban_denial(BanOutcome::Clear), None);
        assert_eq!(
            ban_denial(BanOutcome::Banned),
            Some((
                "banned",
                "blocked: you are banned from this community",
                AuthOutcome::Banned,
            ))
        );
        assert_eq!(
            ban_denial(BanOutcome::DbError),
            Some((
                "ban_check_error",
                "error: internal error checking restriction state",
                AuthOutcome::BanCheckError,
            ))
        );
    }

    #[test]
    fn dependency_failures_are_distinct_from_policy_denials() {
        assert_eq!(
            classify_allowlist(Ok::<_, &str>(true)),
            PolicyCheck::Allowed(())
        );
        assert_eq!(
            classify_allowlist(Ok::<_, &str>(false)),
            PolicyCheck::Denied
        );
        assert_eq!(
            classify_allowlist(Err::<bool, _>("database unavailable")),
            PolicyCheck::DependencyError
        );

        let owner = Keys::generate().public_key();
        assert_eq!(
            classify_relay_membership(Ok(MembershipDecision::OpenRelay)),
            PolicyCheck::Allowed(None)
        );
        assert_eq!(
            classify_relay_membership(Ok(MembershipDecision::Member)),
            PolicyCheck::Allowed(None)
        );
        assert_eq!(
            classify_relay_membership(Ok(MembershipDecision::ViaOwner(owner))),
            PolicyCheck::Allowed(Some(owner))
        );
        assert_eq!(
            classify_relay_membership(Ok(MembershipDecision::Denied)),
            PolicyCheck::Denied
        );
        assert_eq!(
            classify_relay_membership(Err("database unavailable".to_owned())),
            PolicyCheck::DependencyError
        );
    }

    /// The handler owns retry classification, so drive its real terminal-state
    /// branches rather than calling the metric helper directly. A malformed
    /// signature also traverses the real NIP-42 verifier before terminalizing.
    #[tokio::test(flavor = "current_thread")]
    async fn handler_separates_post_terminal_frames_from_invalid_attempts() {
        let recorder = metrics_util::debugging::DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _recorder_guard = metrics::set_default_local_recorder(&recorder);
        let state = crate::state::tests::test_state().await;

        let (authenticated, mut authenticated_rx) =
            test_conn_with_auth(crate::connection::tests::authenticated_state());
        let duplicate = signed_event_with_tags(Vec::new());
        handle_auth(duplicate, authenticated, state.clone()).await;
        let duplicate_frame = crate::connection::tests::read_frame(&mut authenticated_rx);
        assert_eq!(duplicate_frame[2], false);
        assert_eq!(duplicate_frame[3], "auth-required: already authenticated");

        let (failed, mut failed_rx) = test_conn_with_auth(AuthState::Failed);
        let after_failure = signed_event_with_tags(Vec::new());
        handle_auth(after_failure, failed, state.clone()).await;
        let failed_frame = crate::connection::tests::read_frame(&mut failed_rx);
        assert_eq!(failed_frame[2], false);
        assert_eq!(
            failed_frame[3],
            "auth-required: authentication already failed"
        );

        let challenge = "invalid-signature-challenge";
        let (invalid_conn, mut invalid_rx) = test_conn_with_auth(pending(challenge));
        crate::metrics::record_auth_attempt_started();
        let relay_url: RelayUrl = crate::api::bridge::nip42_expected_relay_url(
            &state.config.relay_url,
            &invalid_conn.tenant,
        )
        .parse()
        .expect("test relay URL");
        let mut invalid = EventBuilder::auth(challenge, relay_url)
            .sign_with_keys(&Keys::generate())
            .expect("sign auth event");
        invalid.content.push('x');
        handle_auth(invalid, invalid_conn.clone(), state).await;
        let invalid_frame = crate::connection::tests::read_frame(&mut invalid_rx);
        assert_eq!(invalid_frame[2], false);
        assert_eq!(invalid_frame[3], "auth-required: verification failed");
        assert!(matches!(
            invalid_conn.auth_state_snapshot(),
            AuthState::Failed
        ));

        let snapshot = snapshotter.snapshot().into_vec();
        let attempts = metric_counter(&snapshot, "buzz_auth_attempts_total", None);
        assert_eq!(attempts, 1);
        assert_eq!(
            metric_counter(
                &snapshot,
                "buzz_auth_outcomes_total",
                Some(AuthOutcome::Invalid.as_str()),
            ),
            1
        );
        for state in AuthPostTerminalState::ALL {
            let count = snapshot
                .iter()
                .find_map(|(key, _, _, value)| {
                    (key.key().name() == "buzz_auth_post_terminal_frames_total"
                        && key
                            .key()
                            .labels()
                            .any(|label| label.key() == "state" && label.value() == state.as_str()))
                    .then(|| match value {
                        DebugValue::Counter(value) => *value,
                        _ => panic!("post-terminal frame metric must be a counter"),
                    })
                })
                .unwrap_or_default();
            assert_eq!(count, 1, "{} post-terminal frame", state.as_str());
        }
    }

    /// A verified signature followed by an unavailable restriction database
    /// must deny fail-closed and expose a dependency error, not mislabel the
    /// principal as banned or let the attempt disappear.
    #[tokio::test(flavor = "current_thread")]
    async fn handler_accounts_ban_check_database_error() {
        let recorder = metrics_util::debugging::DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _recorder_guard = metrics::set_default_local_recorder(&recorder);
        let state = crate::state::tests::test_state_with_database_url(
            "postgres://buzz:buzz_dev@127.0.0.1:1/buzz",
        )
        .await;

        let challenge = "ban-check-error-challenge";
        let (conn, _rx) = test_conn_with_auth(pending(challenge));
        crate::metrics::record_auth_attempt_started();
        let relay_url: RelayUrl =
            crate::api::bridge::nip42_expected_relay_url(&state.config.relay_url, &conn.tenant)
                .parse()
                .expect("test relay URL");
        let event = EventBuilder::auth(challenge, relay_url)
            .sign_with_keys(&Keys::generate())
            .expect("sign auth event");

        handle_auth(event, conn.clone(), state).await;

        assert!(matches!(conn.auth_state_snapshot(), AuthState::Failed));
        assert!(conn.cancel.is_cancelled());
        let snapshot = snapshotter.snapshot().into_vec();
        assert_eq!(
            metric_counter(
                &snapshot,
                "buzz_auth_outcomes_total",
                Some(AuthOutcome::BanCheckError.as_str()),
            ),
            1
        );
        assert_eq!(
            metric_counter(
                &snapshot,
                "buzz_auth_outcomes_total",
                Some(AuthOutcome::Banned.as_str()),
            ),
            0
        );
        assert_eq!(
            metric_counter(&snapshot, "buzz_auth_attempts_total", None),
            1
        );
    }
}

#[cfg(test)]
mod postgres_tests {
    use std::{
        collections::HashMap,
        sync::{atomic::AtomicU8, Arc, Mutex as StdMutex},
        time::Instant,
    };

    use axum::extract::ws::Message as WsMessage;
    use buzz_core::{observer::encrypt_observer_payload, TenantContext};
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use tokio::sync::{mpsc, Mutex};
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;

    use crate::{
        connection::{AuthState, ConnectionState},
        handlers::{auth::handle_auth, event::handle_event},
    };

    fn test_connection(
        tenant: TenantContext,
        challenge: &str,
    ) -> (Arc<ConnectionState>, mpsc::Receiver<WsMessage>) {
        let (send_tx, send_rx) = mpsc::channel(8);
        let (ctrl_tx, _ctrl_rx) = mpsc::channel(4);
        let conn = ConnectionState {
            conn_id: Uuid::new_v4(),
            tenant,
            remote_addr: "127.0.0.1:1234".parse().expect("socket address"),
            auth_state: StdMutex::new(AuthState::Pending {
                challenge: challenge.to_owned(),
                started_at: Instant::now(),
            }),
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            send_tx,
            ctrl_tx,
            cancel: CancellationToken::new(),
            backpressure_count: Arc::new(AtomicU8::new(0)),
            grace_limit: 3,
        };
        (Arc::new(conn), send_rx)
    }

    fn parse_ok(frame: WsMessage) -> serde_json::Value {
        let WsMessage::Text(text) = frame else {
            panic!("expected text relay frame");
        };
        serde_json::from_str(&text).expect("relay frame JSON")
    }

    /// Production-seam regression for the deployed failure: the agent is
    /// already a direct relay member, but also presents a valid owner
    /// attestation. AUTH must materialize that owner and the resulting
    /// authenticated connection must be able to publish kind:24200 telemetry.
    #[tokio::test]
    #[ignore = "requires PostgreSQL and Redis"]
    async fn direct_member_auth_materializes_owner_and_authorizes_observer_frame() {
        let database_url = crate::test_support::database_url();
        let host = format!("observer-direct-member-{}.example", Uuid::new_v4());
        let mut config = crate::config::Config::from_env().expect("config from env");
        config.database_url = database_url.clone();
        config.read_database_url = None;
        config.redis_url = std::env::var("BUZZ_TEST_REDIS_URL")
            .or_else(|_| std::env::var("REDIS_URL"))
            .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_owned());
        config.relay_url = format!("wss://{host}");
        config.require_relay_membership = true;
        config.allow_nip_oa_auth = true;
        config.pubkey_allowlist_enabled = false;

        let pool = sqlx::PgPool::connect(&database_url)
            .await
            .expect("requires reachable PostgreSQL");
        let db = buzz_db::Db::from_pool(pool.clone());
        let community = db
            .ensure_configured_community(&host)
            .await
            .expect("ensure test community");
        let tenant = TenantContext::resolved(community.id, &host);

        let owner = Keys::generate();
        let agent = Keys::generate();
        let owner_hex = owner.public_key().to_hex();
        let agent_hex = agent.public_key().to_hex();
        db.add_relay_member(community.id, &owner_hex, "owner", None)
            .await
            .expect("add owner relay member");
        db.add_relay_member(community.id, &agent_hex, "member", Some(&owner_hex))
            .await
            .expect("add direct agent relay member");

        let state =
            crate::state::tests::test_state_with_config_and_database_pool(config, pool.clone())
                .await;

        let challenge = "direct-member-owner-materialization";
        let (conn, mut send_rx) = test_connection(tenant, challenge);
        let auth_tag_json = buzz_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "")
            .expect("compute owner attestation");
        let auth_tag_parts: Vec<String> =
            serde_json::from_str(&auth_tag_json).expect("auth tag JSON");
        let auth_tag = Tag::parse(auth_tag_parts).expect("parse auth tag");
        let relay_tag = Tag::parse(["relay", state.config.relay_url.as_str()]).expect("relay tag");
        let challenge_tag = Tag::parse(["challenge", challenge]).expect("challenge tag");
        let auth_event = EventBuilder::new(Kind::Authentication, "")
            .tags([relay_tag, challenge_tag, auth_tag])
            .sign_with_keys(&agent)
            .expect("sign AUTH event");

        handle_auth(auth_event, Arc::clone(&conn), Arc::clone(&state)).await;
        let auth_ok = parse_ok(send_rx.recv().await.expect("AUTH response"));
        assert_eq!(auth_ok[2], true, "direct-member AUTH must succeed");
        let AuthState::Authenticated(auth_ctx) = conn.auth_state_snapshot() else {
            panic!("connection must be authenticated");
        };
        assert_eq!(auth_ctx.agent_owner_pubkey, Some(owner.public_key()));
        assert!(
            state
                .db
                .is_agent_owner(
                    community.id,
                    agent.public_key().as_bytes(),
                    owner.public_key().as_bytes(),
                )
                .await
                .expect("query materialized owner"),
            "AUTH must persist the owner relationship used by observer authorization"
        );

        let encrypted = encrypt_observer_payload(
            &agent,
            &owner.public_key(),
            &serde_json::json!({"kind":"turn_started","payload":{}}),
        )
        .expect("encrypt observer payload");
        let observer = buzz_sdk::build_agent_observer_frame(
            &owner_hex,
            &agent_hex,
            buzz_core::observer::OBSERVER_FRAME_TELEMETRY,
            &encrypted,
        )
        .expect("build observer frame")
        .sign_with_keys(&agent)
        .expect("sign observer frame");
        handle_event(observer, conn, Arc::clone(&state)).await;
        let observer_ok = parse_ok(send_rx.recv().await.expect("observer response"));
        assert_eq!(
            observer_ok[2], true,
            "materialized direct-member owner must authorize kind:24200"
        );

        sqlx::query("DELETE FROM events WHERE community_id = $1")
            .bind(community.id.as_uuid())
            .execute(&pool)
            .await
            .expect("clean events");
        sqlx::query("DELETE FROM audit_log WHERE community_id = $1")
            .bind(community.id.as_uuid())
            .execute(&pool)
            .await
            .expect("clean audit log");
        sqlx::query("DELETE FROM relay_members WHERE community_id = $1")
            .bind(community.id.as_uuid())
            .execute(&pool)
            .await
            .expect("clean relay members");
        sqlx::query("DELETE FROM users WHERE community_id = $1")
            .bind(community.id.as_uuid())
            .execute(&pool)
            .await
            .expect("clean users");
        sqlx::query("DELETE FROM communities WHERE id = $1")
            .bind(community.id.as_uuid())
            .execute(&pool)
            .await
            .expect("clean community");
    }
}
