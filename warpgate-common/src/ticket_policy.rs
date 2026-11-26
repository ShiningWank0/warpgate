//! Ticket decisions over UTC timestamps expressed in nanoseconds.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redemption {
    Denied,
    Unlimited,
    Spend { remaining: i16 },
}

pub fn is_current(expiry: Option<i128>, now: i128) -> bool {
    expiry.is_none_or(|expiry| now < expiry)
}

/// A configured TTL requires a finite, one-redemption ticket. Evaluate this
/// against the locked database row before committing a redemption.
pub fn decide_redemption(
    issued: i128,
    expiry: Option<i128>,
    uses_left: Option<i16>,
    now: i128,
    max_ttl: Option<u128>,
) -> Redemption {
    if !is_current(expiry, now) {
        return Redemption::Denied;
    }

    if let Some(max_ttl) = max_ttl {
        let Some(expiry) = expiry else {
            return Redemption::Denied;
        };
        if uses_left != Some(1) || issued > now || expiry.abs_diff(issued) > max_ttl {
            return Redemption::Denied;
        }
    }

    match uses_left {
        None => Redemption::Unlimited,
        Some(remaining) if remaining > 0 => Redemption::Spend {
            remaining: remaining - 1,
        },
        Some(_) => Redemption::Denied,
    }
}
