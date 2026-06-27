#![no_std]
//! Dispute resolution contract (#659).
//! Lifecycle: Open → Evidence → Decision → Settled.

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};

// ── Storage keys ──────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    Arbiter,
    Dispute(u64),
    NextId,
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum DisputeStatus {
    Open,
    Evidence,
    Decision,
    Settled,
}

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum Outcome {
    None,
    FavourClaimant,
    FavourRespondent,
    Split, // 50/50
}

#[contracttype]
#[derive(Clone)]
pub struct Dispute {
    pub id: u64,
    pub claimant: Address,
    pub respondent: Address,
    pub amount: i128,
    pub evidence_hash: BytesN<32>, // off-chain content hash; zero = not submitted
    pub status: DisputeStatus,
    pub outcome: Outcome,
    pub created_at: u64,
}

// ── Events ────────────────────────────────────────────────────────────────────
const EVT_OPENED: Symbol = symbol_short!("d_open");
const EVT_EVIDENCE: Symbol = symbol_short!("d_evid");
const EVT_DECISION: Symbol = symbol_short!("d_dec");
const EVT_SETTLED: Symbol = symbol_short!("d_settl");

#[contract]
pub struct DisputeContract;

#[contractimpl]
impl DisputeContract {
    // ── Init ──────────────────────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address, arbiter: Address) {
        assert!(!env.storage().instance().has(&DataKey::Admin), "Already initialized");
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Arbiter, &arbiter);
    }

    pub fn get_arbiter(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Arbiter).unwrap()
    }

    /// Admin can update the arbiter.
    pub fn set_arbiter(env: Env, admin: Address, arbiter: Address) {
        admin.require_auth();
        let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(admin == stored, "Only admin");
        env.storage().instance().set(&DataKey::Arbiter, &arbiter);
    }

    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Claimant opens a dispute. Returns the dispute id.
    pub fn open_dispute(
        env: Env,
        claimant: Address,
        respondent: Address,
        amount: i128,
    ) -> u64 {
        claimant.require_auth();
        assert!(amount > 0, "Amount must be positive");

        let id: u64 = env.storage().instance().get(&DataKey::NextId).unwrap_or(1);

        let dispute = Dispute {
            id,
            claimant: claimant.clone(),
            respondent,
            amount,
            evidence_hash: BytesN::from_array(&env, &[0u8; 32]),
            status: DisputeStatus::Open,
            outcome: Outcome::None,
            created_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&DataKey::Dispute(id), &dispute);
        env.storage().instance().set(&DataKey::NextId, &(id + 1));
        env.events().publish((EVT_OPENED,), (id, claimant, amount));
        id
    }

    /// Either party submits evidence hash (moves to Evidence phase).
    pub fn submit_evidence(env: Env, caller: Address, dispute_id: u64, hash: BytesN<32>) {
        caller.require_auth();

        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .expect("Dispute not found");

        assert!(dispute.status == DisputeStatus::Open, "Must be in Open status");
        assert!(
            caller == dispute.claimant || caller == dispute.respondent,
            "Only parties may submit evidence"
        );

        dispute.evidence_hash = hash;
        dispute.status = DisputeStatus::Evidence;
        env.storage().persistent().set(&DataKey::Dispute(dispute_id), &dispute);
        env.events().publish((EVT_EVIDENCE,), (dispute_id, caller));
    }

    /// Arbiter records their decision (moves to Decision phase).
    pub fn record_decision(env: Env, arbiter: Address, dispute_id: u64, outcome: Outcome) {
        arbiter.require_auth();
        let stored_arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        assert!(arbiter == stored_arbiter, "Only arbiter");

        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .expect("Dispute not found");

        assert!(
            dispute.status == DisputeStatus::Evidence
                || dispute.status == DisputeStatus::Open,
            "Must be in Open or Evidence status"
        );
        assert!(outcome != Outcome::None, "Invalid outcome");

        dispute.outcome = outcome.clone();
        dispute.status = DisputeStatus::Decision;
        env.storage().persistent().set(&DataKey::Dispute(dispute_id), &dispute);
        env.events().publish((EVT_DECISION,), (dispute_id, arbiter));
    }

    /// Enforce settlement: compute payouts based on outcome, mark as Settled.
    /// Returns (claimant_amount, respondent_amount).
    pub fn settle(env: Env, arbiter: Address, dispute_id: u64) -> (i128, i128) {
        arbiter.require_auth();
        let stored_arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        assert!(arbiter == stored_arbiter, "Only arbiter");

        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .expect("Dispute not found");

        assert!(dispute.status == DisputeStatus::Decision, "Must be in Decision status");

        let (claimant_amt, respondent_amt) = match dispute.outcome {
            Outcome::FavourClaimant => (dispute.amount, 0i128),
            Outcome::FavourRespondent => (0i128, dispute.amount),
            Outcome::Split => {
                let half = dispute.amount / 2;
                (half, dispute.amount - half)
            }
            Outcome::None => (0i128, 0i128),
        };

        dispute.status = DisputeStatus::Settled;
        env.storage().persistent().set(&DataKey::Dispute(dispute_id), &dispute);
        env.events()
            .publish((EVT_SETTLED,), (dispute_id, claimant_amt, respondent_amt));
        (claimant_amt, respondent_amt)
    }

    pub fn get_dispute(env: Env, dispute_id: u64) -> Option<Dispute> {
        env.storage().persistent().get(&DataKey::Dispute(dispute_id))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    fn setup() -> (Env, DisputeContractClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register_contract(None, DisputeContract);
        let client = DisputeContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        let arbiter = Address::generate(&env);
        client.initialize(&admin, &arbiter);
        (env, client, admin, arbiter)
    }

    #[test]
    fn test_full_lifecycle_favour_claimant() {
        let (env, client, _, arbiter) = setup();
        let claimant = Address::generate(&env);
        let respondent = Address::generate(&env);

        let id = client.open_dispute(&claimant, &respondent, &1_000);
        let d = client.get_dispute(&id).unwrap();
        assert_eq!(d.status, DisputeStatus::Open);

        let hash = BytesN::from_array(&env, &[1u8; 32]);
        client.submit_evidence(&claimant, &id, &hash);
        let d = client.get_dispute(&id).unwrap();
        assert_eq!(d.status, DisputeStatus::Evidence);

        client.record_decision(&arbiter, &id, &Outcome::FavourClaimant);
        let d = client.get_dispute(&id).unwrap();
        assert_eq!(d.status, DisputeStatus::Decision);

        let (c, r) = client.settle(&arbiter, &id);
        assert_eq!(c, 1_000);
        assert_eq!(r, 0);
        let d = client.get_dispute(&id).unwrap();
        assert_eq!(d.status, DisputeStatus::Settled);
    }

    #[test]
    fn test_full_lifecycle_split() {
        let (env, client, _, arbiter) = setup();
        let claimant = Address::generate(&env);
        let respondent = Address::generate(&env);

        let id = client.open_dispute(&claimant, &respondent, &1_001);
        let hash = BytesN::from_array(&env, &[2u8; 32]);
        client.submit_evidence(&respondent, &id, &hash);
        client.record_decision(&arbiter, &id, &Outcome::Split);
        let (c, r) = client.settle(&arbiter, &id);
        assert_eq!(c, 500);
        assert_eq!(r, 501); // odd amount: respondent gets remainder
    }

    #[test]
    fn test_arbiter_can_decide_without_evidence() {
        let (env, client, _, arbiter) = setup();
        let claimant = Address::generate(&env);
        let respondent = Address::generate(&env);
        let id = client.open_dispute(&claimant, &respondent, &100);
        // Skip evidence step — arbiter decides directly from Open
        client.record_decision(&arbiter, &id, &Outcome::FavourRespondent);
        let (c, r) = client.settle(&arbiter, &id);
        assert_eq!(c, 0);
        assert_eq!(r, 100);
    }

    #[test]
    #[should_panic(expected = "Only arbiter")]
    fn test_non_arbiter_cannot_decide() {
        let (env, client, _, _) = setup();
        let claimant = Address::generate(&env);
        let respondent = Address::generate(&env);
        let id = client.open_dispute(&claimant, &respondent, &100);
        let rando = Address::generate(&env);
        client.record_decision(&rando, &id, &Outcome::FavourClaimant);
    }

    #[test]
    #[should_panic(expected = "Only parties may submit evidence")]
    fn test_third_party_cannot_submit_evidence() {
        let (env, client, _, _) = setup();
        let claimant = Address::generate(&env);
        let respondent = Address::generate(&env);
        let id = client.open_dispute(&claimant, &respondent, &100);
        let rando = Address::generate(&env);
        let hash = BytesN::from_array(&env, &[0u8; 32]);
        client.submit_evidence(&rando, &id, &hash);
    }

    #[test]
    #[should_panic(expected = "Must be in Decision status")]
    fn test_cannot_settle_before_decision() {
        let (env, client, _, arbiter) = setup();
        let claimant = Address::generate(&env);
        let respondent = Address::generate(&env);
        let id = client.open_dispute(&claimant, &respondent, &100);
        client.settle(&arbiter, &id);
    }
}
