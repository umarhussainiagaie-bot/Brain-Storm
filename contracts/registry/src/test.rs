#[cfg(test)]
mod test {
    use crate::{RegistryContract, RegistryContractClient, VerificationLevel};
    use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

    fn setup() -> (Env, RegistryContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register_contract(None, RegistryContract);
        let client = RegistryContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin)
    }

    // ── Init ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_initialize_sets_admin() {
        let (_, client, admin) = setup();
        assert_eq!(client.get_admin(), admin);
    }

    #[test]
    #[should_panic(expected = "Already initialized")]
    fn test_double_init_panics() {
        let (_, client, admin) = setup();
        client.initialize(&admin);
    }

    // ── Curator management ────────────────────────────────────────────────────

    #[test]
    fn test_add_curator() {
        let (env, client, admin) = setup();
        let curator = Address::generate(&env);
        assert!(!client.is_curator(&curator));
        client.add_curator(&admin, &curator);
        assert!(client.is_curator(&curator));
    }

    #[test]
    fn test_remove_curator() {
        let (env, client, admin) = setup();
        let curator = Address::generate(&env);
        client.add_curator(&admin, &curator);
        client.remove_curator(&admin, &curator);
        assert!(!client.is_curator(&curator));
    }

    #[test]
    #[should_panic(expected = "Only admin")]
    fn test_non_admin_cannot_add_curator() {
        let (env, client, _) = setup();
        let rando = Address::generate(&env);
        let curator = Address::generate(&env);
        client.add_curator(&rando, &curator);
    }

    // ── Verification levels ───────────────────────────────────────────────────

    #[test]
    fn test_default_verification_level_is_unverified() {
        let (env, client, _) = setup();
        let user = Address::generate(&env);
        assert_eq!(client.get_verification_level(&user), VerificationLevel::Unverified);
    }

    #[test]
    fn test_admin_can_set_verification_level() {
        let (env, client, admin) = setup();
        let user = Address::generate(&env);
        client.set_verification_level(&admin, &user, &VerificationLevel::Expert);
        assert_eq!(client.get_verification_level(&user), VerificationLevel::Expert);
    }

    #[test]
    fn test_curator_can_set_verification_level() {
        let (env, client, admin) = setup();
        let curator = Address::generate(&env);
        let user = Address::generate(&env);
        client.add_curator(&admin, &curator);
        client.set_verification_level(&curator, &user, &VerificationLevel::Advanced);
        assert_eq!(client.get_verification_level(&user), VerificationLevel::Advanced);
    }

    #[test]
    #[should_panic(expected = "Unauthorized: admin or curator required")]
    fn test_non_curator_cannot_set_level() {
        let (env, client, _) = setup();
        let rando = Address::generate(&env);
        let user = Address::generate(&env);
        client.set_verification_level(&rando, &user, &VerificationLevel::Basic);
    }

    // ── Certified skills ──────────────────────────────────────────────────────

    #[test]
    fn test_add_certified_skill() {
        let (env, client, admin) = setup();
        let user = Address::generate(&env);
        let skill = symbol_short!("rust");
        client.add_certified_skill(&admin, &user, &skill, &0);
        assert!(client.has_certified_skill(&user, &skill));
    }

    #[test]
    fn test_remove_certified_skill() {
        let (env, client, admin) = setup();
        let user = Address::generate(&env);
        let skill = symbol_short!("rust");
        client.add_certified_skill(&admin, &user, &skill, &0);
        client.remove_certified_skill(&admin, &user, &skill);
        assert!(!client.has_certified_skill(&user, &skill));
    }

    #[test]
    fn test_expired_skill_not_returned() {
        let (env, client, admin) = setup();
        let user = Address::generate(&env);
        let skill = symbol_short!("rust");
        // Set expiry to ledger time 1 (already past since env starts at 0)
        client.add_certified_skill(&admin, &user, &skill, &1);
        // Advance ledger time beyond expiry
        env.ledger().set_timestamp(100);
        assert!(!client.has_certified_skill(&user, &skill));
    }

    #[test]
    fn test_non_expired_skill_is_returned() {
        let (env, client, admin) = setup();
        let user = Address::generate(&env);
        let skill = symbol_short!("python");
        // Far-future expiry
        client.add_certified_skill(&admin, &user, &skill, &999_999_999);
        assert!(client.has_certified_skill(&user, &skill));
    }

    #[test]
    fn test_duplicate_skill_not_added_twice() {
        let (env, client, admin) = setup();
        let user = Address::generate(&env);
        let skill = symbol_short!("rust");
        client.add_certified_skill(&admin, &user, &skill, &0);
        client.add_certified_skill(&admin, &user, &skill, &0);
        let skills = client.get_certified_skills(&user);
        assert_eq!(skills.len(), 1);
    }

    #[test]
    #[should_panic(expected = "Unauthorized: admin or curator required")]
    fn test_non_curator_cannot_add_skill() {
        let (env, client, _) = setup();
        let rando = Address::generate(&env);
        let user = Address::generate(&env);
        client.add_certified_skill(&rando, &user, &symbol_short!("rust"), &0);
    }

    // ── Specialisations ───────────────────────────────────────────────────────

    #[test]
    fn test_set_and_get_specialisations() {
        let (env, client, admin) = setup();
        let user = Address::generate(&env);
        let mut specs = Vec::new(&env);
        specs.push_back(symbol_short!("defi"));
        specs.push_back(symbol_short!("nft"));
        client.set_specialisations(&admin, &user, &specs);
        let result = client.get_specialisations(&user);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_empty_specialisations_default() {
        let (env, client, _) = setup();
        let user = Address::generate(&env);
        assert_eq!(client.get_specialisations(&user).len(), 0);
    }

    #[test]
    #[should_panic(expected = "Unauthorized: admin or curator required")]
    fn test_non_curator_cannot_set_specialisations() {
        let (env, client, _) = setup();
        let rando = Address::generate(&env);
        let user = Address::generate(&env);
        client.set_specialisations(&rando, &user, &Vec::new(&env));
    }

    // ── Pagination / filtering (#701) ─────────────────────────────────────────

    #[test]
    fn test_register_and_list_users() {
        let (env, client, _) = setup();
        let u1 = Address::generate(&env);
        let u2 = Address::generate(&env);
        let u3 = Address::generate(&env);
        client.register_user(&u1);
        client.register_user(&u2);
        client.register_user(&u3);
        assert_eq!(client.total_users(), 3);
        let page = client.list_users(&0, &2);
        assert_eq!(page.len(), 2);
        let page2 = client.list_users(&2, &10);
        assert_eq!(page2.len(), 1);
    }

    #[test]
    fn test_list_users_empty() {
        let (_, client, _) = setup();
        assert_eq!(client.list_users(&0, &10).len(), 0);
        assert_eq!(client.total_users(), 0);
    }

    #[test]
    fn test_list_users_offset_beyond_end() {
        let (env, client, _) = setup();
        let u1 = Address::generate(&env);
        client.register_user(&u1);
        // offset past end returns empty
        let page = client.list_users(&100, &10);
        assert_eq!(page.len(), 0);
    }

    #[test]
    fn test_list_users_last_page() {
        let (env, client, _) = setup();
        for _ in 0..5 {
            client.register_user(&Address::generate(&env));
        }
        // Page that starts at 3, asking for 10 → only 2 results
        let page = client.list_users(&3, &10);
        assert_eq!(page.len(), 2);
    }

    #[test]
    fn test_register_idempotent() {
        let (env, client, _) = setup();
        let u = Address::generate(&env);
        client.register_user(&u);
        client.register_user(&u);
        assert_eq!(client.total_users(), 1);
    }

    #[test]
    fn test_list_users_by_level_filter() {
        let (env, client, admin) = setup();
        let u1 = Address::generate(&env);
        let u2 = Address::generate(&env);
        let u3 = Address::generate(&env);
        client.register_user(&u1);
        client.register_user(&u2);
        client.register_user(&u3);
        client.set_verification_level(&admin, &u1, &VerificationLevel::Basic);
        client.set_verification_level(&admin, &u2, &VerificationLevel::Expert);
        // u3 stays Unverified
        let advanced_plus = client.list_users_by_level(&VerificationLevel::Advanced, &0, &10);
        assert_eq!(advanced_plus.len(), 1);
        assert_eq!(advanced_plus.get(0).unwrap(), u2);
        let basic_plus = client.list_users_by_level(&VerificationLevel::Basic, &0, &10);
        assert_eq!(basic_plus.len(), 2);
    }

    #[test]
    fn test_list_users_by_level_pagination() {
        let (env, client, admin) = setup();
        for _ in 0..4 {
            let u = Address::generate(&env);
            client.register_user(&u);
            client.set_verification_level(&admin, &u, &VerificationLevel::Basic);
        }
        let page1 = client.list_users_by_level(&VerificationLevel::Basic, &0, &2);
        assert_eq!(page1.len(), 2);
        let page2 = client.list_users_by_level(&VerificationLevel::Basic, &2, &2);
        assert_eq!(page2.len(), 2);
        let page3 = client.list_users_by_level(&VerificationLevel::Basic, &4, &2);
        assert_eq!(page3.len(), 0);
    }
}
