#[path = "../../warpgate-common/src/ticket_policy.rs"]
mod ticket_policy;

use ticket_policy::{Redemption, decide_redemption, is_current};

#[kani::proof]
fn the_expiry_instant_is_excluded() {
    let expiry: i128 = kani::any();
    let now: i128 = kani::any();
    assert_eq!(is_current(Some(expiry), now), now < expiry);
    assert!(is_current(None, now));
}

#[kani::proof]
fn ordinary_redemption_obeys_expiry_and_use_count() {
    let issued: i128 = kani::any();
    let expiry: Option<i128> = kani::any();
    let uses: Option<i16> = kani::any();
    let now: i128 = kani::any();
    let expired = matches!(expiry, Some(end) if end <= now);

    match decide_redemption(issued, expiry, uses, now, None) {
        Redemption::Denied => {
            assert!(expired || matches!(uses, Some(n) if n <= 0));
        }
        Redemption::Unlimited => {
            assert!(!expired && uses.is_none());
        }
        Redemption::Spend { remaining } => {
            assert!(!expired);
            assert!(remaining >= 0);
            assert_eq!(uses.map(i32::from), Some(i32::from(remaining) + 1));
        }
    }
}

#[kani::proof]
fn short_lived_redemption_requires_exactly_one_use_and_a_bounded_lifetime() {
    let issued: i128 = kani::any();
    let expiry: Option<i128> = kani::any();
    let uses: Option<i16> = kani::any();
    let now: i128 = kani::any();
    let limit: u128 = kani::any();

    // Express the lifetime limit as an upper bound on the expiry rather than
    // the distance between two timestamps. If the bound exceeds i128::MAX,
    // every representable expiry already satisfies it.
    let may_redeem = uses == Some(1)
        && issued <= now
        && expiry.is_some_and(|end| {
            now < end
                && issued
                    .checked_add_unsigned(limit)
                    .is_none_or(|bound| end <= bound)
        });

    let decision = decide_redemption(issued, expiry, uses, now, Some(limit));
    assert_eq!(
        decision,
        if may_redeem {
            Redemption::Spend { remaining: 0 }
        } else {
            Redemption::Denied
        },
    );
}

#[kani::proof]
fn a_committed_one_use_redemption_leaves_no_second_use() {
    let issued: i128 = kani::any();
    let expiry: Option<i128> = kani::any();
    let first_now: i128 = kani::any();
    let second_now: i128 = kani::any();
    let first_policy: Option<u128> = kani::any();
    let second_policy: Option<u128> = kani::any();

    if let Redemption::Spend { remaining } =
        decide_redemption(issued, expiry, Some(1), first_now, first_policy)
    {
        assert_eq!(
            decide_redemption(issued, expiry, Some(remaining), second_now, second_policy),
            Redemption::Denied,
        );
    }
}
