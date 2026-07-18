#![cfg(test)]

use super::*;
use soroban_sdk::{symbol_short, testutils::{Address as _, Ledger as _}, token, Address, Env, Symbol};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Returns (env, contract_client, token_id, asset_client, admin).
/// Caller must keep `env` alive and re-derive clients from it when needed.
fn setup_with_fee(fee_bps: i128) -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, IotContract);
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let c = IotContractClient::new(&env, &contract_id);
    c.initialize(&admin, &fee_bps);
    // Whitelist the default token so existing tests keep passing.
    c.add_token(&admin, &token_id);

    (env, contract_id, token_id, admin)
}

fn setup() -> (Env, Address, Address, Address) {
    setup_with_fee(500) // 5 %
}

// ── initialization ────────────────────────────────────────────────────────────

#[test]
fn test_initialize_stores_fee() {
    let (env, cid, _, _) = setup();
    assert_eq!(IotContractClient::new(&env, &cid).get_platform_fee(), 500);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_initialize_twice_panics() {
    let (env, cid, _, admin) = setup();
    IotContractClient::new(&env, &cid).initialize(&admin, &100);
}

#[test]
#[should_panic(expected = "invalid platform fee")]
fn test_initialize_negative_fee_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, IotContract);
    let admin = Address::generate(&env);
    IotContractClient::new(&env, &cid).initialize(&admin, &-1);
}

#[test]
#[should_panic(expected = "invalid platform fee")]
fn test_initialize_fee_exceeds_denominator_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, IotContract);
    let admin = Address::generate(&env);
    IotContractClient::new(&env, &cid).initialize(&admin, &10_001);
}

// ── platform fee management ───────────────────────────────────────────────────

#[test]
fn test_set_platform_fee() {
    let (env, cid, _, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    c.set_platform_fee(&admin, &200);
    assert_eq!(c.get_platform_fee(), 200);
}

#[test]
#[should_panic(expected = "admin required")]
fn test_set_platform_fee_non_admin_panics() {
    let (env, cid, _, _) = setup();
    let impostor = Address::generate(&env);
    IotContractClient::new(&env, &cid).set_platform_fee(&impostor, &200);
}

#[test]
fn test_zero_fee_no_split() {
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    let price = 1_000i128;

    c.init_device(&symbol_short!("d"), &price, &owner);
    asset.mint(&user, &price);
    c.request_access(&symbol_short!("d"), &user, &token_id, &price);

    assert_eq!(tok.balance(&owner), price);
    assert_eq!(tok.balance(&cid), 0);
    assert_eq!(c.get_platform_fee_balance(&token_id), 0);
}

// ── device registration ───────────────────────────────────────────────────────

#[test]
fn test_init_device_stores_price_and_owner() {
    let (env, cid, _, _) = setup();
    let c = IotContractClient::new(&env, &cid);
    let owner = Address::generate(&env);
    c.init_device(&symbol_short!("d1"), &2_000, &owner);
    assert_eq!(c.get_device_price(&symbol_short!("d1")), 2_000);
    assert_eq!(c.get_device_owner(&symbol_short!("d1")).unwrap(), owner);
}

#[test]
#[should_panic(expected = "price must be positive")]
fn test_init_device_zero_price_panics() {
    let (env, cid, _, _) = setup();
    IotContractClient::new(&env, &cid)
        .init_device(&symbol_short!("d1"), &0, &Address::generate(&env));
}

#[test]
#[should_panic(expected = "price must be positive")]
fn test_init_device_negative_price_panics() {
    let (env, cid, _, _) = setup();
    IotContractClient::new(&env, &cid)
        .init_device(&symbol_short!("d1"), &-1, &Address::generate(&env));
}

#[test]
fn test_init_device_re_registration_overwrites() {
    let (env, cid, _, _) = setup();
    let c = IotContractClient::new(&env, &cid);
    let owner2 = Address::generate(&env);
    c.init_device(&symbol_short!("d1"), &100, &Address::generate(&env));
    c.init_device(&symbol_short!("d1"), &999, &owner2);
    assert_eq!(c.get_device_price(&symbol_short!("d1")), 999);
    assert_eq!(c.get_device_owner(&symbol_short!("d1")).unwrap(), owner2);
}

#[test]
fn test_get_nonexistent_device_returns_defaults() {
    let (env, cid, _, _) = setup();
    let c = IotContractClient::new(&env, &cid);
    assert_eq!(c.get_device_price(&symbol_short!("nope")), 0);
    assert!(c.get_device_owner(&symbol_short!("nope")).is_none());
}

// ── payment and access flow ───────────────────────────────────────────────────

#[test]
fn test_request_access_splits_revenue_correctly() {
    // 5 % fee → owner gets 950, contract keeps 50
    let (env, cid, token_id, _) = setup();
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &1_000, &owner);
    asset.mint(&user, &1_000);

    assert!(c.request_access(&symbol_short!("d1"), &user, &token_id, &1_000));
    assert_eq!(tok.balance(&owner), 950);
    assert_eq!(tok.balance(&cid), 50);
    assert_eq!(c.get_platform_fee_balance(&token_id), 50);
    assert_eq!(tok.balance(&user), 0);
}

#[test]
fn test_pay_alias_is_equivalent() {
    let (env, cid, token_id, _) = setup();
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &500, &owner);
    asset.mint(&user, &500);

    assert!(c.pay(&symbol_short!("d1"), &user, &token_id, &500));
    assert_eq!(tok.balance(&owner), 475); // 500 - 5%
}

#[test]
fn test_overpayment_is_accepted() {
    // paying more than price is allowed; full amount is transferred
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &100, &owner);
    asset.mint(&user, &200);

    assert!(c.request_access(&symbol_short!("d1"), &user, &token_id, &200));
    assert_eq!(tok.balance(&owner), 200);
}

// ── edge case: insufficient amount ───────────────────────────────────────────

#[test]
fn test_insufficient_amount_returns_false_no_transfer() {
    let (env, cid, token_id, _) = setup();
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &1_000, &owner);
    asset.mint(&user, &999);

    assert!(!c.request_access(&symbol_short!("d1"), &user, &token_id, &999));
    assert_eq!(tok.balance(&user), 999); // no tokens moved
    assert_eq!(tok.balance(&owner), 0);
}

#[test]
fn test_zero_amount_returns_false() {
    let (env, cid, token_id, _) = setup();
    let c = IotContractClient::new(&env, &cid);
    c.init_device(&symbol_short!("d1"), &1_000, &Address::generate(&env));
    assert!(!c.request_access(
        &symbol_short!("d1"),
        &Address::generate(&env),
        &token_id,
        &0
    ));
}

// ── edge case: unregistered device ───────────────────────────────────────────

#[test]
fn test_request_access_unregistered_device_returns_false() {
    let (env, cid, token_id, _) = setup();
    let c = IotContractClient::new(&env, &cid);
    let user = Address::generate(&env);
    token::StellarAssetClient::new(&env, &token_id).mint(&user, &1_000);
    // price == 0 for unknown device → returns false without panic
    assert!(!c.request_access(&symbol_short!("nope"), &user, &token_id, &1_000));
}

// ── edge case: double access ──────────────────────────────────────────────────

#[test]
fn test_double_access_both_succeed() {
    // Contract does not prevent re-payment; each call is independent.
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &100, &owner);
    asset.mint(&user, &200);

    assert!(c.request_access(&symbol_short!("d1"), &user, &token_id, &100));
    assert!(c.request_access(&symbol_short!("d1"), &user, &token_id, &100));
    assert_eq!(tok.balance(&owner), 200);
    assert_eq!(tok.balance(&user), 0);
}

// ── platform fee accumulation ─────────────────────────────────────────────────

#[test]
fn test_platform_fee_accumulates_across_payments() {
    let (env, cid, token_id, _) = setup(); // 5 %
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let owner = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &1_000, &owner);
    for _ in 0..3 {
        let user = Address::generate(&env);
        asset.mint(&user, &1_000);
        c.request_access(&symbol_short!("d1"), &user, &token_id, &1_000);
    }

    assert_eq!(c.get_platform_fee_balance(&token_id), 150); // 50 × 3
}

// ── platform fee withdrawal ───────────────────────────────────────────────────

#[test]
fn test_admin_withdraw_full_balance() {
    let (env, cid, token_id, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &1_000, &owner);
    asset.mint(&user, &1_000);
    c.request_access(&symbol_short!("d1"), &user, &token_id, &1_000);

    c.withdraw_platform_fees(&admin, &token_id, &recipient, &50);
    assert_eq!(tok.balance(&recipient), 50);
    assert_eq!(c.get_platform_fee_balance(&token_id), 0);
}

#[test]
fn test_admin_withdraw_partial_balance() {
    let (env, cid, token_id, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &1_000, &owner);
    asset.mint(&user, &1_000);
    c.request_access(&symbol_short!("d1"), &user, &token_id, &1_000);

    c.withdraw_platform_fees(&admin, &token_id, &recipient, &20);
    assert_eq!(tok.balance(&recipient), 20);
    assert_eq!(c.get_platform_fee_balance(&token_id), 30);
}

#[test]
#[should_panic(expected = "insufficient platform fees")]
fn test_withdraw_more_than_balance_panics() {
    let (env, cid, token_id, admin) = setup();
    let recipient = Address::generate(&env);
    IotContractClient::new(&env, &cid)
        .withdraw_platform_fees(&admin, &token_id, &recipient, &1);
}

#[test]
#[should_panic(expected = "withdraw amount must be positive")]
fn test_withdraw_zero_panics() {
    let (env, cid, token_id, admin) = setup();
    let recipient = Address::generate(&env);
    IotContractClient::new(&env, &cid)
        .withdraw_platform_fees(&admin, &token_id, &recipient, &0);
}

#[test]
#[should_panic(expected = "admin required")]
fn test_withdraw_non_admin_panics() {
    let (env, cid, token_id, _) = setup();
    let impostor = Address::generate(&env);
    let recipient = Address::generate(&env);
    IotContractClient::new(&env, &cid)
        .withdraw_platform_fees(&impostor, &token_id, &recipient, &1);
}

// ── full integration: register → pay → withdraw ───────────────────────────────

#[test]
fn test_full_integration_flow() {
    let (env, cid, token_id, admin) = setup_with_fee(1_000); // 10 %
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);

    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let treasury = Address::generate(&env);

    c.init_device(&symbol_short!("s1"), &1_000, &owner1);
    c.init_device(&symbol_short!("s2"), &2_000, &owner2);

    asset.mint(&user1, &1_000);
    asset.mint(&user2, &2_000);

    assert!(c.request_access(&symbol_short!("s1"), &user1, &token_id, &1_000));
    assert!(c.request_access(&symbol_short!("s2"), &user2, &token_id, &2_000));

    assert_eq!(tok.balance(&owner1), 900);   // 1000 - 10%
    assert_eq!(tok.balance(&owner2), 1_800); // 2000 - 10%
    assert_eq!(c.get_platform_fee_balance(&token_id), 300); // 100+200

    c.withdraw_platform_fees(&admin, &token_id, &treasury, &300);
    assert_eq!(tok.balance(&treasury), 300);
    assert_eq!(c.get_platform_fee_balance(&token_id), 0);
}

// ── bulk access purchase ──────────────────────────────────────────────────────

/// Build a Vec<Symbol> and Vec<i128> from slices for test convenience.
fn make_bulk(
    env: &Env,
    pairs: &[(&str, i128)],
) -> (soroban_sdk::Vec<Symbol>, soroban_sdk::Vec<i128>) {
    let mut ids = soroban_sdk::Vec::new(env);
    let mut amts = soroban_sdk::Vec::new(env);
    for (id, amt) in pairs {
        ids.push_back(Symbol::new(env, id));
        amts.push_back(*amt);
    }
    (ids, amts)
}

#[test]
fn test_bulk_single_device_no_discount() {
    // < 10 devices → no discount; user pays full price, credit stored at face value.
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&Symbol::new(&env, "dev1"), &500, &owner);
    asset.mint(&user, &500);

    let (ids, amts) = make_bulk(&env, &[("dev1", 500)]);
    c.purchase_bulk_access(&user, &token_id, &ids, &amts);

    // Contract holds the funds.
    assert_eq!(tok.balance(&cid), 500);
    assert_eq!(tok.balance(&user), 0);
    // Credit stored at face value.
    assert_eq!(c.get_credit(&user, &Symbol::new(&env, "dev1")), 500);
}

#[test]
fn test_bulk_tier2_discount_10_devices() {
    // 10 devices → 5 % discount; user pays 950 per 1000, credit = 1000.
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    for i in 0..10_u32 {
        let dev = Symbol::new(&env, match i {
            0 => "d0", 1 => "d1", 2 => "d2", 3 => "d3", 4 => "d4",
            5 => "d5", 6 => "d6", 7 => "d7", 8 => "d8", _ => "d9",
        });
        c.init_device(&dev, &1_000, &owner);
    }

    // Mint enough for 10 × 950 (after 5 % discount).
    asset.mint(&user, &9_500);

    let pairs: &[(&str, i128)] = &[
        ("d0",1000),("d1",1000),("d2",1000),("d3",1000),("d4",1000),
        ("d5",1000),("d6",1000),("d7",1000),("d8",1000),("d9",1000),
    ];
    let (ids, amts) = make_bulk(&env, pairs);
    c.purchase_bulk_access(&user, &token_id, &ids, &amts);

    // User spent 9500 (5 % off 10 000).
    assert_eq!(tok.balance(&user), 0);
    assert_eq!(tok.balance(&cid), 9_500);
    // Each credit is still stored at face value (1000).
    assert_eq!(c.get_credit(&user, &Symbol::new(&env, "d0")), 1_000);
}

#[test]
fn test_bulk_credit_redeemed_on_request_access() {
    // Buy a credit, then call request_access → credit consumed, owner paid from contract.
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&Symbol::new(&env, "dev1"), &1_000, &owner);
    asset.mint(&user, &1_000);

    let (ids, amts) = make_bulk(&env, &[("dev1", 1_000)]);
    c.purchase_bulk_access(&user, &token_id, &ids, &amts);

    assert_eq!(c.get_credit(&user, &Symbol::new(&env, "dev1")), 1_000);

    // request_access with amount=0 (credit should cover it).
    assert!(c.request_access(&Symbol::new(&env, "dev1"), &user, &token_id, &0));

    // Owner received funds from contract escrow.
    assert_eq!(tok.balance(&owner), 1_000);
    assert_eq!(tok.balance(&cid), 0);
    // Credit consumed.
    assert_eq!(c.get_credit(&user, &Symbol::new(&env, "dev1")), 0);
}

#[test]
fn test_bulk_credit_partially_consumed() {
    // Credit of 2000 for a 1000-price device: after one access, 1000 remains.
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&Symbol::new(&env, "dev1"), &1_000, &owner);
    asset.mint(&user, &2_000);

    let (ids, amts) = make_bulk(&env, &[("dev1", 2_000)]);
    c.purchase_bulk_access(&user, &token_id, &ids, &amts);

    assert!(c.request_access(&Symbol::new(&env, "dev1"), &user, &token_id, &0));
    assert_eq!(tok.balance(&owner), 1_000);
    assert_eq!(c.get_credit(&user, &Symbol::new(&env, "dev1")), 1_000);
}

#[test]
fn test_bulk_credit_with_platform_fee() {
    // 5 % fee: credit redeemed → owner gets 950, fee balance += 50.
    let (env, cid, token_id, _) = setup(); // 5 %
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&Symbol::new(&env, "dev1"), &1_000, &owner);
    asset.mint(&user, &1_000);

    let (ids, amts) = make_bulk(&env, &[("dev1", 1_000)]);
    c.purchase_bulk_access(&user, &token_id, &ids, &amts);
    assert!(c.request_access(&Symbol::new(&env, "dev1"), &user, &token_id, &0));

    assert_eq!(tok.balance(&owner), 950);
    assert_eq!(c.get_platform_fee_balance(&token_id), 50);
}

#[test]
fn test_bulk_expired_credit_falls_back_to_payment() {
    // In test builds CREDIT_TTL_LEDGERS = 100. Jump to ledger 200 (past TTL but within
    // the contract instance TTL of 4096) so the instance is still live but the credit is not.
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let tok = token::Client::new(&env, &token_id);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&Symbol::new(&env, "dev1"), &1_000, &owner);
    asset.mint(&user, &2_000); // 1000 for bulk credit + 1000 for fallback payment

    let (ids, amts) = make_bulk(&env, &[("dev1", 1_000)]);
    c.purchase_bulk_access(&user, &token_id, &ids, &amts);

    // Jump past the 100-ledger test TTL without archiving the instance (stays < 4096).
    env.ledger().set_sequence_number(200);

    // request_access should fall back to regular payment (credit expired).
    assert!(c.request_access(&Symbol::new(&env, "dev1"), &user, &token_id, &1_000));
    assert_eq!(tok.balance(&owner), 1_000);
}

#[test]
#[should_panic(expected = "device not registered")]
fn test_bulk_unregistered_device_panics() {
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let user = Address::generate(&env);
    asset.mint(&user, &1_000);

    let (ids, amts) = make_bulk(&env, &[("ghost", 1_000)]);
    c.purchase_bulk_access(&user, &token_id, &ids, &amts);
}

#[test]
#[should_panic(expected = "amount below device price")]
fn test_bulk_amount_below_price_panics() {
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let asset = token::StellarAssetClient::new(&env, &token_id);
    let user = Address::generate(&env);
    c.init_device(&Symbol::new(&env, "dev1"), &1_000, &Address::generate(&env));
    asset.mint(&user, &500);

    let (ids, amts) = make_bulk(&env, &[("dev1", 500)]);
    c.purchase_bulk_access(&user, &token_id, &ids, &amts);
}

#[test]
#[should_panic(expected = "empty device list")]
fn test_bulk_empty_list_panics() {
    let (env, cid, token_id, _) = setup_with_fee(0);
    let user = Address::generate(&env);
    let ids: soroban_sdk::Vec<Symbol> = soroban_sdk::Vec::new(&env);
    let amts: soroban_sdk::Vec<i128> = soroban_sdk::Vec::new(&env);
    IotContractClient::new(&env, &cid).purchase_bulk_access(&user, &token_id, &ids, &amts);
}

#[test]
#[should_panic(expected = "device_ids and amounts length mismatch")]
fn test_bulk_length_mismatch_panics() {
    let (env, cid, token_id, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let user = Address::generate(&env);
    c.init_device(&Symbol::new(&env, "dev1"), &500, &Address::generate(&env));

    let mut ids: soroban_sdk::Vec<Symbol> = soroban_sdk::Vec::new(&env);
    ids.push_back(Symbol::new(&env, "dev1"));
    let amts: soroban_sdk::Vec<i128> = soroban_sdk::Vec::new(&env);
    c.purchase_bulk_access(&user, &token_id, &ids, &amts);
}

// ── emergency pause ───────────────────────────────────────────────────────────

#[test]
fn test_pause_blocks_request_access() {
    let (env, cid, token_id, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &1_000, &owner);
    token::StellarAssetClient::new(&env, &token_id).mint(&user, &1_000);

    c.pause(&admin);
    assert!(c.is_paused());
}

#[test]
#[should_panic(expected = "contract paused")]
fn test_request_access_panics_when_paused() {
    let (env, cid, token_id, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &1_000, &owner);
    token::StellarAssetClient::new(&env, &token_id).mint(&user, &1_000);

    c.pause(&admin);
    c.request_access(&symbol_short!("d1"), &user, &token_id, &1_000);
}

#[test]
#[should_panic(expected = "contract paused")]
fn test_purchase_bulk_access_panics_when_paused() {
    let (env, cid, token_id, admin) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&Symbol::new(&env, "dev1"), &500, &owner);
    token::StellarAssetClient::new(&env, &token_id).mint(&user, &500);

    c.pause(&admin);
    let (ids, amts) = make_bulk(&env, &[("dev1", 500)]);
    c.purchase_bulk_access(&user, &token_id, &ids, &amts);
}

#[test]
fn test_read_only_functions_work_when_paused() {
    let (env, cid, token_id, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    let owner = Address::generate(&env);
    c.init_device(&symbol_short!("d1"), &1_000, &owner);

    c.pause(&admin);

    // All reads must succeed.
    assert!(c.is_paused());
    assert_eq!(c.get_platform_fee(), 500);
    assert_eq!(c.get_platform_fee_balance(&token_id), 0);
    assert_eq!(c.get_device_price(&symbol_short!("d1")), 1_000);
    assert_eq!(c.get_device_owner(&symbol_short!("d1")).unwrap(), owner);
}

#[test]
#[should_panic(expected = "admin required")]
fn test_pause_non_admin_panics() {
    let (env, cid, _, _) = setup();
    IotContractClient::new(&env, &cid).pause(&Address::generate(&env));
}

#[test]
#[should_panic(expected = "not paused")]
fn test_unpause_when_not_paused_panics() {
    let (env, cid, _, admin) = setup();
    IotContractClient::new(&env, &cid).unpause(&admin);
}

#[test]
#[should_panic(expected = "timelock not elapsed")]
fn test_execute_unpause_before_delay_panics() {
    let (env, cid, _, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    c.pause(&admin);
    c.unpause(&admin); // schedules at current_ledger + 5
    // Try to execute immediately (delay = 5 ledgers in test builds).
    c.execute_unpause(&admin);
}

#[test]
fn test_unpause_after_timelock_succeeds() {
    let (env, cid, token_id, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&symbol_short!("d1"), &1_000, &owner);
    token::StellarAssetClient::new(&env, &token_id).mint(&user, &1_000);

    c.pause(&admin);
    assert!(c.is_paused());

    c.unpause(&admin); // schedule
    // Advance ledger past the 5-ledger test delay (stays within instance TTL).
    env.ledger().set_sequence_number(env.ledger().sequence() + 5);
    c.execute_unpause(&admin);

    assert!(!c.is_paused());
    // Payments work again.
    assert!(c.request_access(&symbol_short!("d1"), &user, &token_id, &1_000));
}

#[test]
#[should_panic(expected = "unpause not scheduled")]
fn test_execute_unpause_without_schedule_panics() {
    let (env, cid, _, admin) = setup();
    IotContractClient::new(&env, &cid).execute_unpause(&admin);
}

// ── multi-token payment support ───────────────────────────────────────────────

/// Returns a second token that is NOT yet whitelisted.
fn make_token(env: &Env) -> Address {
    let token_admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(token_admin).address()
}

#[test]
fn test_add_and_query_token_whitelist() {
    let (env, cid, _, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    let tok2 = make_token(&env);

    assert!(!c.is_token_whitelisted(&tok2));
    c.add_token(&admin, &tok2);
    assert!(c.is_token_whitelisted(&tok2));
    c.remove_token(&admin, &tok2);
    assert!(!c.is_token_whitelisted(&tok2));
}

#[test]
#[should_panic(expected = "token not whitelisted")]
fn test_request_access_non_whitelisted_token_panics() {
    let (env, cid, _, _) = setup();
    let c = IotContractClient::new(&env, &cid);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    let bad_token = make_token(&env); // not whitelisted

    c.init_device(&symbol_short!("d1"), &1_000, &owner);
    token::StellarAssetClient::new(&env, &bad_token).mint(&user, &1_000);
    c.request_access(&symbol_short!("d1"), &user, &bad_token, &1_000);
}

#[test]
fn test_request_access_with_whitelisted_second_token() {
    let (env, cid, _, admin) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let tok2 = make_token(&env);
    c.add_token(&admin, &tok2);

    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    c.init_device(&symbol_short!("d1"), &500, &owner);
    token::StellarAssetClient::new(&env, &tok2).mint(&user, &500);

    assert!(c.request_access(&symbol_short!("d1"), &user, &tok2, &500));
    assert_eq!(token::Client::new(&env, &tok2).balance(&owner), 500);
}

#[test]
fn test_set_and_get_device_accepted_tokens() {
    let (env, cid, _, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    let owner = Address::generate(&env);
    let tok2 = make_token(&env);
    c.add_token(&admin, &tok2);

    c.init_device(&symbol_short!("d1"), &1_000, &owner);

    // By default, empty = accept all whitelisted.
    assert_eq!(c.get_device_accepted_tokens(&symbol_short!("d1")).len(), 0);

    // Restrict to tok2 only.
    let mut accepted = soroban_sdk::Vec::new(&env);
    accepted.push_back(tok2.clone());
    c.set_device_accepted_tokens(&owner, &symbol_short!("d1"), &accepted);
    assert_eq!(c.get_device_accepted_tokens(&symbol_short!("d1")).len(), 1);
    assert!(c.get_device_accepted_tokens(&symbol_short!("d1")).contains(&tok2));
}

#[test]
#[should_panic(expected = "token not accepted by device")]
fn test_request_access_token_not_accepted_by_device_panics() {
    let (env, cid, token_id, admin) = setup();
    let c = IotContractClient::new(&env, &cid);
    let owner = Address::generate(&env);
    let tok2 = make_token(&env);
    c.add_token(&admin, &tok2);

    c.init_device(&symbol_short!("d1"), &1_000, &owner);

    // Restrict device to tok2 only.
    let mut accepted = soroban_sdk::Vec::new(&env);
    accepted.push_back(tok2.clone());
    c.set_device_accepted_tokens(&owner, &symbol_short!("d1"), &accepted);

    // Try paying with token_id (whitelisted but not in device list).
    let user = Address::generate(&env);
    token::StellarAssetClient::new(&env, &token_id).mint(&user, &1_000);
    c.request_access(&symbol_short!("d1"), &user, &token_id, &1_000);
}

#[test]
fn test_token_price_oracle_overrides_base_price() {
    let (env, cid, _, admin) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let tok2 = make_token(&env);
    c.add_token(&admin, &tok2);

    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    c.init_device(&symbol_short!("d1"), &1_000, &owner);

    // Set a different price for tok2.
    c.set_token_price(&admin, &symbol_short!("d1"), &tok2, &200);
    assert_eq!(c.get_token_price(&symbol_short!("d1"), &tok2), 200);
    // Falls back to base price when no oracle entry.
    assert_eq!(c.get_token_price(&symbol_short!("d1"), &Address::generate(&env)), 1_000);

    // Pay with tok2 using oracle price.
    token::StellarAssetClient::new(&env, &tok2).mint(&user, &200);
    assert!(c.request_access(&symbol_short!("d1"), &user, &tok2, &200));
    assert_eq!(token::Client::new(&env, &tok2).balance(&owner), 200);
}

#[test]
fn test_bulk_access_with_second_whitelisted_token() {
    let (env, cid, _, admin) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let tok2 = make_token(&env);
    c.add_token(&admin, &tok2);

    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    c.init_device(&Symbol::new(&env, "dev1"), &500, &owner);

    token::StellarAssetClient::new(&env, &tok2).mint(&user, &500);
    let (ids, amts) = make_bulk(&env, &[("dev1", 500)]);
    c.purchase_bulk_access(&user, &tok2, &ids, &amts);

    assert_eq!(c.get_credit(&user, &Symbol::new(&env, "dev1")), 500);
}

#[test]
#[should_panic(expected = "token not whitelisted")]
fn test_bulk_access_non_whitelisted_token_panics() {
    let (env, cid, _, _) = setup_with_fee(0);
    let c = IotContractClient::new(&env, &cid);
    let bad_token = make_token(&env);
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    c.init_device(&Symbol::new(&env, "dev1"), &500, &owner);
    token::StellarAssetClient::new(&env, &bad_token).mint(&user, &500);
    let (ids, amts) = make_bulk(&env, &[("dev1", 500)]);
    c.purchase_bulk_access(&user, &bad_token, &ids, &amts);
}

#[test]
#[should_panic(expected = "admin required")]
fn test_add_token_non_admin_panics() {
    let (env, cid, _, _) = setup();
    let impostor = Address::generate(&env);
    let tok = make_token(&env);
    IotContractClient::new(&env, &cid).add_token(&impostor, &tok);
}
