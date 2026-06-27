#![no_std]
//! Market contract — escrow, tips, protocol fees (#660) and multi-sig escrow (#658).

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec,
};

pub mod fees;
pub mod multisig_escrow;

// ── Storage keys ──────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    FeeBps,
    Treasury,
    TreasuryBalance,
    Escrow(u64),
    NextEscrowId,
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum EscrowStatus {
    Funded,
    Settled,
    Refunded,
}

#[contracttype]
#[derive(Clone)]
pub struct Escrow {
    pub id: u64,
    pub payer: Address,
    pub payee: Address,
    pub amount: i128,
    pub status: EscrowStatus,
}

// ── Events ────────────────────────────────────────────────────────────────────
const EVT_ESCROW_FUNDED: Symbol = symbol_short!("es_fund");
const EVT_ESCROW_SETTLED: Symbol = symbol_short!("es_settl");
const EVT_ESCROW_REFUNDED: Symbol = symbol_short!("es_refnd");
const EVT_TIP: Symbol = symbol_short!("tip");

#[contract]
pub struct MarketContract;

#[contractimpl]
impl MarketContract {
    // ── Init ──────────────────────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address) {
        assert!(!env.storage().instance().has(&DataKey::Admin), "Already initialized");
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    // ── Fee configuration (#660) ──────────────────────────────────────────────

    pub fn set_fee_bps(env: Env, admin: Address, fee_bps: u32) {
        fees::set_fee_bps(&env, &admin, fee_bps);
    }

    pub fn get_fee_bps(env: Env) -> u32 {
        fees::get_fee_bps(&env)
    }

    pub fn set_treasury(env: Env, admin: Address, treasury: Address) {
        fees::set_treasury(&env, &admin, treasury);
    }

    pub fn get_treasury_balance(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::TreasuryBalance)
            .unwrap_or(0)
    }

    // ── Escrow (#660) ─────────────────────────────────────────────────────────

    /// Fund an escrow (caller is the payer).
    pub fn fund_escrow(env: Env, payer: Address, payee: Address, amount: i128) -> u64 {
        payer.require_auth();
        assert!(amount > 0, "Amount must be positive");

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextEscrowId)
            .unwrap_or(1);

        let escrow = Escrow { id, payer: payer.clone(), payee, amount, status: EscrowStatus::Funded };
        env.storage().persistent().set(&DataKey::Escrow(id), &escrow);
        env.storage().instance().set(&DataKey::NextEscrowId, &(id + 1));
        env.events().publish((EVT_ESCROW_FUNDED,), (id, payer, amount));
        id
    }

    /// Settle escrow: apply fee → treasury, net → payee.
    /// Only the payer or admin may settle.
    pub fn settle_escrow(env: Env, caller: Address, escrow_id: u64) -> (i128, i128) {
        caller.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        assert!(escrow.status == EscrowStatus::Funded, "Escrow not funded");

        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(caller == escrow.payer || caller == admin, "Unauthorized");

        let fee_bps = fees::get_fee_bps(&env);
        let (fee, net) = fees::compute_fee(escrow.amount, fee_bps);

        fees::accrue_fee(&env, fee);

        escrow.status = EscrowStatus::Settled;
        env.storage().persistent().set(&DataKey::Escrow(escrow_id), &escrow);
        env.events()
            .publish((EVT_ESCROW_SETTLED,), (escrow_id, escrow.payee, net, fee));
        (net, fee)
    }

    /// Refund escrow back to payer (admin-only).
    pub fn refund_escrow(env: Env, admin: Address, escrow_id: u64) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(admin == stored_admin, "Only admin");

        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        assert!(escrow.status == EscrowStatus::Funded, "Escrow not funded");
        escrow.status = EscrowStatus::Refunded;
        env.storage().persistent().set(&DataKey::Escrow(escrow_id), &escrow);
        env.events()
            .publish((EVT_ESCROW_REFUNDED,), (escrow_id, escrow.payer, escrow.amount));
    }

    pub fn get_escrow(env: Env, escrow_id: u64) -> Option<Escrow> {
        env.storage().persistent().get(&DataKey::Escrow(escrow_id))
    }

    // ── Tip (#660) ────────────────────────────────────────────────────────────

    /// Send a tip: fee → treasury, net returned to caller for actual token transfer.
    pub fn tip(env: Env, tipper: Address, amount: i128) -> (i128, i128) {
        tipper.require_auth();
        assert!(amount > 0, "Amount must be positive");
        let fee_bps = fees::get_fee_bps(&env);
        let (fee, net) = fees::compute_fee(amount, fee_bps);
        fees::accrue_fee(&env, fee);
        env.events().publish((EVT_TIP,), (tipper, amount, fee, net));
        (net, fee)
    }

    // ── Multi-sig escrow (#658) ───────────────────────────────────────────────

    pub fn ms_fund_escrow(
        env: Env,
        payer: Address,
        payee: Address,
        amount: i128,
        signers: Vec<Address>,
        threshold: u32,
        timeout_ledgers: u32,
    ) -> u64 {
        multisig_escrow::create_ms_escrow(&env, payer, payee, amount, signers, threshold, timeout_ledgers)
    }

    pub fn ms_approve_escrow(env: Env, escrow_id: u64, signer: Address) {
        multisig_escrow::approve_ms_escrow(&env, escrow_id, signer);
    }

    pub fn ms_timeout_escrow(env: Env, escrow_id: u64) -> bool {
        multisig_escrow::timeout_ms_escrow(&env, escrow_id)
    }

    pub fn ms_get_escrow(env: Env, escrow_id: u64) -> Option<multisig_escrow::MsEscrow> {
        multisig_escrow::get_ms_escrow(&env, escrow_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, vec, Env};

    fn setup() -> (Env, MarketContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register_contract(None, MarketContract);
        let client = MarketContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin)
    }

    // ── Fee config ────────────────────────────────────────────────────────────

    #[test]
    fn test_fee_default_zero() {
        let (_, client, _) = setup();
        assert_eq!(client.get_fee_bps(), 0);
    }

    #[test]
    fn test_set_fee_bps() {
        let (_, client, admin) = setup();
        client.set_fee_bps(&admin, &200); // 2 %
        assert_eq!(client.get_fee_bps(), 200);
    }

    #[test]
    #[should_panic(expected = "Fee exceeds max")]
    fn test_fee_bps_too_high() {
        let (_, client, admin) = setup();
        client.set_fee_bps(&admin, &1001);
    }

    #[test]
    #[should_panic(expected = "Only admin")]
    fn test_non_admin_cannot_set_fee() {
        let (env, client, _) = setup();
        let rando = Address::generate(&env);
        client.set_fee_bps(&rando, &100);
    }

    // ── Escrow + fee ──────────────────────────────────────────────────────────

    #[test]
    fn test_settle_applies_fee() {
        let (env, client, admin) = setup();
        let treasury = Address::generate(&env);
        client.set_fee_bps(&admin, &200); // 2 %
        client.set_treasury(&admin, &treasury);
        let payer = Address::generate(&env);
        let payee = Address::generate(&env);
        let id = client.fund_escrow(&payer, &payee, &1_000_000);
        let (net, fee) = client.settle_escrow(&payer, &id);
        assert_eq!(fee, 20_000);   // 2 % of 1_000_000
        assert_eq!(net, 980_000);
        assert_eq!(client.get_treasury_balance(), 20_000);
    }

    #[test]
    fn test_settle_zero_fee() {
        let (env, client, _) = setup();
        let payer = Address::generate(&env);
        let payee = Address::generate(&env);
        let id = client.fund_escrow(&payer, &payee, &500);
        let (net, fee) = client.settle_escrow(&payer, &id);
        assert_eq!(fee, 0);
        assert_eq!(net, 500);
    }

    #[test]
    fn test_settle_rounding_down() {
        // 1 bps on 1 token = 0.0001 → rounds to 0
        let (env, client, admin) = setup();
        let treasury = Address::generate(&env);
        client.set_fee_bps(&admin, &1);
        client.set_treasury(&admin, &treasury);
        let payer = Address::generate(&env);
        let payee = Address::generate(&env);
        let id = client.fund_escrow(&payer, &payee, &1);
        let (net, fee) = client.settle_escrow(&payer, &id);
        assert_eq!(fee, 0);
        assert_eq!(net, 1);
    }

    #[test]
    fn test_refund_escrow() {
        let (env, client, admin) = setup();
        let payer = Address::generate(&env);
        let payee = Address::generate(&env);
        let id = client.fund_escrow(&payer, &payee, &100);
        client.refund_escrow(&admin, &id);
        let escrow = client.get_escrow(&id).unwrap();
        assert_eq!(escrow.status, EscrowStatus::Refunded);
    }

    // ── Tip ───────────────────────────────────────────────────────────────────

    #[test]
    fn test_tip_fee_distribution() {
        let (env, client, admin) = setup();
        let treasury = Address::generate(&env);
        client.set_fee_bps(&admin, &500); // 5 %
        client.set_treasury(&admin, &treasury);
        let tipper = Address::generate(&env);
        let (net, fee) = client.tip(&tipper, &10_000);
        assert_eq!(fee, 500);
        assert_eq!(net, 9_500);
        assert_eq!(client.get_treasury_balance(), 500);
    }

    // ── Multi-sig escrow ──────────────────────────────────────────────────────

    #[test]
    fn test_ms_escrow_threshold_release() {
        let (env, client, _) = setup();
        let payer = Address::generate(&env);
        let payee = Address::generate(&env);
        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);
        let s3 = Address::generate(&env);
        let signers = vec![&env, s1.clone(), s2.clone(), s3.clone()];
        let id = client.ms_fund_escrow(&payer, &payee, &5_000, &signers, &2, &100);
        client.ms_approve_escrow(&id, &s1);
        // After 1 approval, still pending
        let escrow = client.ms_get_escrow(&id).unwrap();
        assert_eq!(escrow.status, multisig_escrow::MsEscrowStatus::Pending);
        client.ms_approve_escrow(&id, &s2);
        // After 2 approvals (threshold=2), released
        let escrow = client.ms_get_escrow(&id).unwrap();
        assert_eq!(escrow.status, multisig_escrow::MsEscrowStatus::Released);
    }

    #[test]
    fn test_ms_escrow_timeout() {
        let (env, client, _) = setup();
        let payer = Address::generate(&env);
        let payee = Address::generate(&env);
        let s1 = Address::generate(&env);
        let signers = vec![&env, s1.clone()];
        let id = client.ms_fund_escrow(&payer, &payee, &1_000, &signers, &1, &10);
        // Advance ledger past expiry
        env.ledger().set_sequence_number(200);
        let refund = client.ms_timeout_escrow(&id);
        assert!(refund);
        let escrow = client.ms_get_escrow(&id).unwrap();
        assert_eq!(escrow.status, multisig_escrow::MsEscrowStatus::TimedOut);
    }

    #[test]
    #[should_panic(expected = "Not an authorized signer")]
    fn test_ms_escrow_unauthorized_signer() {
        let (env, client, _) = setup();
        let payer = Address::generate(&env);
        let payee = Address::generate(&env);
        let s1 = Address::generate(&env);
        let rando = Address::generate(&env);
        let signers = vec![&env, s1];
        let id = client.ms_fund_escrow(&payer, &payee, &1_000, &signers, &1, &100);
        client.ms_approve_escrow(&id, &rando);
    }

    #[test]
    #[should_panic(expected = "Invalid threshold")]
    fn test_ms_escrow_threshold_exceeds_signers() {
        let (env, client, _) = setup();
        let payer = Address::generate(&env);
        let payee = Address::generate(&env);
        let s1 = Address::generate(&env);
        let signers = vec![&env, s1];
        client.ms_fund_escrow(&payer, &payee, &1_000, &signers, &5, &100);
    }
}
