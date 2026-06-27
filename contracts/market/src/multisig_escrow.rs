//! Multi-signature escrow variant for high-value jobs (#658).

use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol, Vec};

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum MsEscrowStatus {
    Pending,   // collecting approvals
    Released,  // threshold met, funds released
    TimedOut,  // expired before threshold reached
}

#[contracttype]
#[derive(Clone)]
pub struct MsEscrow {
    pub id: u64,
    pub payer: Address,
    pub payee: Address,
    pub amount: i128,
    pub signers: Vec<Address>,
    pub threshold: u32,
    pub approvals: Vec<Address>,
    pub expires_ledger: u32,
    pub status: MsEscrowStatus,
}

#[contracttype]
pub enum MsKey {
    Escrow(u64),
    NextId,
}

const EVT_MS_CREATE: Symbol = symbol_short!("ms_create");
const EVT_MS_APPROVE: Symbol = symbol_short!("ms_appr");
const EVT_MS_RELEASE: Symbol = symbol_short!("ms_rel");
const EVT_MS_TIMEOUT: Symbol = symbol_short!("ms_tout");

pub fn create_ms_escrow(
    env: &Env,
    payer: Address,
    payee: Address,
    amount: i128,
    signers: Vec<Address>,
    threshold: u32,
    timeout_ledgers: u32,
) -> u64 {
    payer.require_auth();
    assert!(amount > 0, "Amount must be positive");
    assert!(!signers.is_empty(), "Need at least one signer");
    assert!(threshold > 0 && threshold <= signers.len() as u32, "Invalid threshold");

    let id: u64 = env
        .storage()
        .instance()
        .get(&MsKey::NextId)
        .unwrap_or(1);

    let escrow = MsEscrow {
        id,
        payer: payer.clone(),
        payee,
        amount,
        signers,
        threshold,
        approvals: Vec::new(env),
        expires_ledger: env.ledger().sequence() + timeout_ledgers,
        status: MsEscrowStatus::Pending,
    };

    env.storage().persistent().set(&MsKey::Escrow(id), &escrow);
    env.storage().instance().set(&MsKey::NextId, &(id + 1));

    env.events().publish((EVT_MS_CREATE,), (id, payer, amount));
    id
}

pub fn approve_ms_escrow(env: &Env, escrow_id: u64, signer: Address) {
    signer.require_auth();

    let mut escrow: MsEscrow = env
        .storage()
        .persistent()
        .get(&MsKey::Escrow(escrow_id))
        .expect("Escrow not found");

    assert!(escrow.status == MsEscrowStatus::Pending, "Escrow not pending");
    assert!(
        env.ledger().sequence() < escrow.expires_ledger,
        "Escrow expired"
    );
    assert!(
        escrow.signers.iter().any(|s| s == signer),
        "Not an authorized signer"
    );
    assert!(
        !escrow.approvals.iter().any(|a| a == signer),
        "Already approved"
    );

    escrow.approvals.push_back(signer.clone());

    if escrow.approvals.len() as u32 >= escrow.threshold {
        escrow.status = MsEscrowStatus::Released;
        env.events()
            .publish((EVT_MS_RELEASE,), (escrow_id, escrow.payee.clone(), escrow.amount));
    }

    env.storage().persistent().set(&MsKey::Escrow(escrow_id), &escrow);
    env.events().publish((EVT_MS_APPROVE,), (escrow_id, signer));
}

/// Mark escrow as timed-out if expired and threshold not met.
/// Returns true if the fallback (refund to payer) should be executed.
pub fn timeout_ms_escrow(env: &Env, escrow_id: u64) -> bool {
    let mut escrow: MsEscrow = env
        .storage()
        .persistent()
        .get(&MsKey::Escrow(escrow_id))
        .expect("Escrow not found");

    assert!(escrow.status == MsEscrowStatus::Pending, "Escrow not pending");
    assert!(
        env.ledger().sequence() >= escrow.expires_ledger,
        "Escrow has not expired yet"
    );

    escrow.status = MsEscrowStatus::TimedOut;
    env.storage().persistent().set(&MsKey::Escrow(escrow_id), &escrow);
    env.events().publish((EVT_MS_TIMEOUT,), (escrow_id, escrow.payer));
    true
}

pub fn get_ms_escrow(env: &Env, escrow_id: u64) -> Option<MsEscrow> {
    env.storage().persistent().get(&MsKey::Escrow(escrow_id))
}
