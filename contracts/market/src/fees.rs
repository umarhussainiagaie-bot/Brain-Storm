//! Protocol fee configuration and distribution helpers (#660).

use soroban_sdk::{symbol_short, Address, Env, Symbol};

use crate::DataKey;

pub const MAX_FEE_BPS: u32 = 1_000; // 10 %
const EVT_FEE_SET: Symbol = symbol_short!("fee_set");
const EVT_FEE_DIST: Symbol = symbol_short!("fee_dist");

/// Configure the protocol fee in basis points (admin-only).
pub fn set_fee_bps(env: &Env, admin: &Address, fee_bps: u32) {
    admin.require_auth();
    let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
    assert!(*admin == stored, "Only admin");
    assert!(fee_bps <= MAX_FEE_BPS, "Fee exceeds max (1000 bps)");
    env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
    env.events().publish((EVT_FEE_SET,), fee_bps);
}

pub fn get_fee_bps(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::FeeBps)
        .unwrap_or(0)
}

/// Configure the treasury address (admin-only).
pub fn set_treasury(env: &Env, admin: &Address, treasury: Address) {
    admin.require_auth();
    let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
    assert!(*admin == stored, "Only admin");
    env.storage().instance().set(&DataKey::Treasury, &treasury);
}

pub fn get_treasury(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Treasury)
}

/// Compute fee and net amount. Returns `(fee, net)`.
/// Rounds fee down (truncation), net = amount - fee.
pub fn compute_fee(amount: i128, fee_bps: u32) -> (i128, i128) {
    let fee = amount * fee_bps as i128 / 10_000;
    (fee, amount - fee)
}

/// Accrue fee to the treasury balance in persistent storage.
pub fn accrue_fee(env: &Env, fee: i128) {
    if fee == 0 {
        return;
    }
    let treasury: Address = env
        .storage()
        .instance()
        .get(&DataKey::Treasury)
        .expect("Treasury not configured");
    let balance: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TreasuryBalance)
        .unwrap_or(0);
    env.storage()
        .persistent()
        .set(&DataKey::TreasuryBalance, &(balance + fee));
    env.events().publish((EVT_FEE_DIST,), (treasury, fee));
}
