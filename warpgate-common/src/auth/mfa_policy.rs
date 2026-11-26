#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MissingMfaFactors {
    pub(super) required: bool,
    pub(super) primary: bool,
}

pub(super) fn missing_mfa_factors(
    required_verified: bool,
    requires_totp: bool,
    primary_verified: bool,
) -> MissingMfaFactors {
    MissingMfaFactors {
        required: !required_verified,
        primary: requires_totp && !primary_verified,
    }
}
