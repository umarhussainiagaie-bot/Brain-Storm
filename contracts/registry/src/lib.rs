#![no_std]
//! Registry contract — verification levels, certified skills, and specialisations.
//!
//! #656 Acceptance Criteria:
//! - Verification levels and skills stored and queryable.
//! - Admin/curator-gated mutations.
//! - Events emitted for UI/indexer consumption.
//! - Tests cover edge cases.

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec,
};

// ── Storage keys ──────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    Curator(Address),
    VerificationLevel(Address),   // persistent: VerificationLevel
    CertifiedSkills(Address),     // persistent: Vec<Symbol>
    Specialisation(Address),      // persistent: Vec<Symbol>
    SkillExpiry(Address, Symbol), // persistent: u64 timestamp (0 = never)
    UserList,                     // instance: Vec<Address> — ordered registration list
}

fn level_ord(level: &VerificationLevel) -> u32 {
    match level {
        VerificationLevel::Unverified => 0,
        VerificationLevel::Basic => 1,
        VerificationLevel::Advanced => 2,
        VerificationLevel::Expert => 3,
    }
}

// ── Types ─────────────────────────────────────────────────────────────────────

/// Numeric verification tier (0 = unverified … 3 = fully verified).
#[contracttype]
#[derive(Clone, PartialEq)]
pub enum VerificationLevel {
    Unverified,
    Basic,
    Advanced,
    Expert,
}

// ── Events ────────────────────────────────────────────────────────────────────
const EVT_VL_SET: Symbol = symbol_short!("vl_set");
const EVT_SKILL_ADD: Symbol = symbol_short!("sk_add");
const EVT_SKILL_RM: Symbol = symbol_short!("sk_rm");
const EVT_SPEC_SET: Symbol = symbol_short!("sp_set");
const EVT_CURATOR_ADD: Symbol = symbol_short!("cur_add");

#[contract]
pub struct RegistryContract;

#[contractimpl]
impl RegistryContract {
    // ── Initialisation ────────────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address) {
        assert!(!env.storage().instance().has(&DataKey::Admin), "Already initialized");
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    // ── Curator management (admin-only) ───────────────────────────────────────

    pub fn add_curator(env: Env, admin: Address, curator: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Curator(curator.clone()), &true);
        env.events().publish((EVT_CURATOR_ADD, symbol_short!("addr")), curator);
    }

    pub fn remove_curator(env: Env, admin: Address, curator: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);
        env.storage().instance().remove(&DataKey::Curator(curator));
    }

    pub fn is_curator(env: Env, addr: Address) -> bool {
        env.storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Curator(addr))
            .unwrap_or(false)
    }

    // ── Verification levels ───────────────────────────────────────────────────

    /// Set the verification level for `user` (admin or curator only).
    pub fn set_verification_level(
        env: Env,
        setter: Address,
        user: Address,
        level: VerificationLevel,
    ) {
        setter.require_auth();
        Self::assert_admin_or_curator(&env, &setter);
        env.storage()
            .persistent()
            .set(&DataKey::VerificationLevel(user.clone()), &level);
        // Encode level as u32 for the event payload (contracttype enums aren't always indexer-friendly)
        let level_u32: u32 = match level {
            VerificationLevel::Unverified => 0,
            VerificationLevel::Basic => 1,
            VerificationLevel::Advanced => 2,
            VerificationLevel::Expert => 3,
        };
        env.events()
            .publish((EVT_VL_SET, symbol_short!("user")), (user, level_u32));
    }

    pub fn get_verification_level(env: Env, user: Address) -> VerificationLevel {
        env.storage()
            .persistent()
            .get(&DataKey::VerificationLevel(user))
            .unwrap_or(VerificationLevel::Unverified)
    }

    // ── Certified skills ──────────────────────────────────────────────────────

    /// Add a certified skill to `user`, optionally with an expiry ledger timestamp.
    /// Expiry 0 means the skill never expires.
    pub fn add_certified_skill(
        env: Env,
        setter: Address,
        user: Address,
        skill: Symbol,
        expiry_ts: u64,
    ) {
        setter.require_auth();
        Self::assert_admin_or_curator(&env, &setter);

        let mut skills: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&DataKey::CertifiedSkills(user.clone()))
            .unwrap_or_else(|| Vec::new(&env));

        if !skills.iter().any(|s| s == skill) {
            skills.push_back(skill.clone());
            env.storage()
                .persistent()
                .set(&DataKey::CertifiedSkills(user.clone()), &skills);
        }

        env.storage()
            .persistent()
            .set(&DataKey::SkillExpiry(user.clone(), skill.clone()), &expiry_ts);

        env.events()
            .publish((EVT_SKILL_ADD, symbol_short!("user")), (user, skill, expiry_ts));
    }

    /// Remove a certified skill from `user` (admin or curator only).
    pub fn remove_certified_skill(env: Env, setter: Address, user: Address, skill: Symbol) {
        setter.require_auth();
        Self::assert_admin_or_curator(&env, &setter);

        let skills: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&DataKey::CertifiedSkills(user.clone()))
            .unwrap_or_else(|| Vec::new(&env));

        let mut updated = Vec::new(&env);
        for s in skills.iter() {
            if s != skill {
                updated.push_back(s);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::CertifiedSkills(user.clone()), &updated);
        env.storage()
            .persistent()
            .remove(&DataKey::SkillExpiry(user.clone(), skill.clone()));

        env.events()
            .publish((EVT_SKILL_RM, symbol_short!("user")), (user, skill));
    }

    /// Returns skills that have not expired (or have no expiry set).
    pub fn get_certified_skills(env: Env, user: Address) -> Vec<Symbol> {
        let skills: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&DataKey::CertifiedSkills(user.clone()))
            .unwrap_or_else(|| Vec::new(&env));

        let now = env.ledger().timestamp();
        let mut valid = Vec::new(&env);
        for skill in skills.iter() {
            let expiry: u64 = env
                .storage()
                .persistent()
                .get(&DataKey::SkillExpiry(user.clone(), skill.clone()))
                .unwrap_or(0);
            if expiry == 0 || now < expiry {
                valid.push_back(skill);
            }
        }
        valid
    }

    pub fn has_certified_skill(env: Env, user: Address, skill: Symbol) -> bool {
        let skills = Self::get_certified_skills(env, user);
        skills.iter().any(|s| s == skill)
    }

    // ── Specialisations ───────────────────────────────────────────────────────

    pub fn set_specialisations(
        env: Env,
        setter: Address,
        user: Address,
        specs: Vec<Symbol>,
    ) {
        setter.require_auth();
        Self::assert_admin_or_curator(&env, &setter);
        env.storage()
            .persistent()
            .set(&DataKey::Specialisation(user.clone()), &specs);
        env.events()
            .publish((EVT_SPEC_SET, symbol_short!("user")), user);
    }

    pub fn get_specialisations(env: Env, user: Address) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&DataKey::Specialisation(user))
            .unwrap_or_else(|| Vec::new(&env))
    }

    // ── Pagination + filtering (#701) ─────────────────────────────────────────

    /// Register a user in the global listing (idempotent).
    pub fn register_user(env: Env, user: Address) {
        user.require_auth();
        let mut list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::UserList)
            .unwrap_or_else(|| Vec::new(&env));
        if !list.iter().any(|a| a == user) {
            list.push_back(user.clone());
            env.storage().instance().set(&DataKey::UserList, &list);
        }
    }

    /// Return a page of registered users.
    /// `offset` is zero-based; `limit` is the max entries to return.
    pub fn list_users(env: Env, offset: u32, limit: u32) -> Vec<Address> {
        let list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::UserList)
            .unwrap_or_else(|| Vec::new(&env));
        let total = list.len();
        let start = offset.min(total);
        let end = (offset + limit).min(total);
        let mut page = Vec::new(&env);
        let mut i = start;
        while i < end {
            page.push_back(list.get(i).unwrap());
            i += 1;
        }
        page
    }

    /// Return users filtered by minimum verification level.
    /// `offset`/`limit` apply after filtering.
    pub fn list_users_by_level(
        env: Env,
        min_level: VerificationLevel,
        offset: u32,
        limit: u32,
    ) -> Vec<Address> {
        let list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::UserList)
            .unwrap_or_else(|| Vec::new(&env));

        let min_ord = level_ord(&min_level);
        let mut filtered = Vec::new(&env);
        for addr in list.iter() {
            let level = env
                .storage()
                .persistent()
                .get(&DataKey::VerificationLevel(addr.clone()))
                .unwrap_or(VerificationLevel::Unverified);
            if level_ord(&level) >= min_ord {
                filtered.push_back(addr);
            }
        }

        let total = filtered.len();
        let start = offset.min(total);
        let end = (offset + limit).min(total);
        let mut page = Vec::new(&env);
        let mut i = start;
        while i < end {
            page.push_back(filtered.get(i).unwrap());
            i += 1;
        }
        page
    }

    pub fn total_users(env: Env) -> u32 {
        let list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::UserList)
            .unwrap_or_else(|| Vec::new(&env));
        list.len()
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn assert_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(*caller == admin, "Only admin");
    }

    fn assert_admin_or_curator(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        let is_curator = env
            .storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Curator(caller.clone()))
            .unwrap_or(false);
        assert!(*caller == admin || is_curator, "Unauthorized: admin or curator required");
    }
}

#[cfg(test)]
mod test;
