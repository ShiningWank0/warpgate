#[path = "../../warpgate-protocol-ssh/src/server/session_deadline.rs"]
mod session_deadline;

use session_deadline::SessionDeadline;

#[kani::proof]
fn target_authentication_cannot_replace_an_expired_opening_deadline() {
    let opening: Option<u64> = kani::any();
    let observed_at: u64 = kani::any();
    let active: Option<u64> = kani::any();

    let next = SessionDeadline::Opening(opening)
        .on_connected(observed_at, |start| {
            assert_eq!(start, observed_at);
            Ok::<_, ()>(active)
        })
        .unwrap();

    match opening {
        Some(cutoff) if cutoff <= observed_at => assert_eq!(next, None),
        _ => assert_eq!(next, Some(SessionDeadline::Active(active))),
    }
}

#[kani::proof]
fn repeated_target_authentication_cannot_renew_an_active_deadline() {
    let active: Option<u64> = kani::any();
    let observed_at: u64 = kani::any();
    let current = SessionDeadline::Active(active);

    let next = current
        .on_connected(observed_at, |_| -> Result<_, ()> {
            panic!("an active session must never calculate a fresh cutoff")
        })
        .unwrap();

    match active {
        Some(cutoff) if cutoff <= observed_at => assert_eq!(next, None),
        _ => assert_eq!(next, Some(current)),
    }
}

#[kani::proof]
fn an_unrepresentable_active_cutoff_is_an_error() {
    let observed_at: u64 = kani::any();
    let next = SessionDeadline::Opening(None).on_connected(observed_at, |_| Err(()));
    assert_eq!(next, Err(()));
}
