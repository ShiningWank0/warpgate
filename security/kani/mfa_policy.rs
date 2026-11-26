#[path = "../../warpgate-common/src/auth/mfa_policy.rs"]
mod mfa_policy;

use mfa_policy::missing_mfa_factors;

#[kani::proof]
fn enforced_totp_requires_a_separately_verified_primary() {
    let totp_verified: bool = kani::any();
    let primary_verified: bool = kani::any();
    let missing = missing_mfa_factors(totp_verified, true, primary_verified);

    assert_eq!(
        !missing.required && !missing.primary,
        totp_verified && primary_verified
    );
    assert_eq!(missing.required, !totp_verified);
    assert_eq!(missing.primary, !primary_verified);
}

#[kani::proof]
fn verified_browser_approval_can_satisfy_the_mfa_requirement() {
    let browser_verified: bool = kani::any();
    let primary_verified: bool = kani::any();
    let missing = missing_mfa_factors(browser_verified, false, primary_verified);

    assert_eq!(missing.required, !browser_verified);
    assert!(!missing.primary);
}
