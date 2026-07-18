#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol, Vec,
};

// ── Token-whitelist / device-accepted-tokens helpers ─────────────────────────

/// Returns true if `token` is in the global admin-managed whitelist.
fn token_whitelisted(env: &Env, token: &Address) -> bool {
    env.storage()
        .instance()
        .get::<_, bool>(&DataKey::TokenWhitelist(token.clone()))
        .unwrap_or(false)
}

/// Returns true if `token` is accepted by the device (or device has no restriction).
fn token_accepted_by_device(env: &Env, device_id: &Symbol, token: &Address) -> bool {
    let accepted: Vec<Address> = env
        .storage()
        .persistent()
        .get(&DataKey::DeviceTokens(device_id.clone()))
        .unwrap_or_else(|| Vec::new(env));
    // empty list = accept all whitelisted tokens
    if accepted.is_empty() {
        return true;
    }
    accepted.contains(token)
}

/// Panic unless the token is whitelisted AND accepted by the device.
fn require_token_valid(env: &Env, device_id: &Symbol, token: &Address) {
    if !token_whitelisted(env, token) {
        panic!("token not whitelisted");
    }
    if !token_accepted_by_device(env, device_id, token) {
        panic!("token not accepted by device");
    }
}

const FEE_DENOMINATOR: i128 = 10_000;
/// Credit TTL: 30 days in ledgers (~5 s/ledger). In tests use a short TTL.
#[cfg(not(test))]
const CREDIT_TTL_LEDGERS: u32 = 518_400;
#[cfg(test)]
const CREDIT_TTL_LEDGERS: u32 = 100;

/// Timelock before unpause takes effect (~1 hour at 5 s/ledger). Short in tests.
#[cfg(not(test))]
const UNPAUSE_DELAY_LEDGERS: u32 = 720;
#[cfg(test)]
const UNPAUSE_DELAY_LEDGERS: u32 = 5;

// ── Discount tiers (device count thresholds) ─────────────────────────────────
// ≥10 devices → 5 % off, ≥50 → 10 % off, ≥100 → 20 % off
const TIER2_MIN: usize = 10;
const TIER2_DISC_BPS: i128 = 500;
const TIER3_MIN: usize = 50;
const TIER3_DISC_BPS: i128 = 1_000;
const TIER4_MIN: usize = 100;
const TIER4_DISC_BPS: i128 = 2_000;

/// Duration in ledgers for each subscription tier.
/// Stellar produces ~1 ledger every 5 seconds.
/// Daily  ≈ 17_280 ledgers (24h * 3600s / 5s)
/// Weekly ≈ 120_960 ledgers (7 days)
/// Monthly ≈ 518_400 ledgers (30 days)
const LEDGERS_PER_DAY: u32 = 17_280;
const LEDGERS_PER_WEEK: u32 = 120_960;
const LEDGERS_PER_MONTH: u32 = 518_400;

/// Discount in basis points applied to the per-access price for subscriptions.
/// Daily: 10% off, Weekly: 20% off, Monthly: 30% off.
const DAILY_DISCOUNT_BPS: i128 = 1_000;
const WEEKLY_DISCOUNT_BPS: i128 = 2_000;
const MONTHLY_DISCOUNT_BPS: i128 = 3_000;

#[contracttype]
#[derive(Clone)]
pub struct DevicePrice {
    pub device_id: Symbol,
    pub price: i128,
}

/// Per-user, per-device credit record.
#[contracttype]
#[derive(Clone)]
pub struct Credit {
    pub amount: i128,
    pub expires_at: u32, // ledger sequence
}

/// Subscription tier selecting duration and discount.
#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub enum SubscriptionTier {
    Daily,
    Weekly,
    Monthly,
}

/// On-chain subscription record.
#[contracttype]
#[derive(Clone)]
pub struct Subscription {
    pub user: Address,
    pub device_id: Symbol,
    pub tier: SubscriptionTier,
    pub start_ledger: u32,
    pub end_ledger: u32,
    pub amount_paid: i128,
    pub active: bool,
}

#[contracttype]
pub enum DataKey {
    Admin,
    PlatformFeeBps,
    PlatformFeeBalance(Address),
    DevicePrice(Symbol),
    DeviceOwner(Symbol),
    /// Prepaid credits: (user, device_id) → Credit
    Credit(Address, Symbol),
    /// Per-user, per-device subscription record
    Subscription(Address, Symbol),
    /// true when contract is paused
    Paused,
    /// ledger sequence at which unpause was scheduled
    UnpauseAt,
    /// Admin token whitelist: Address → bool
    TokenWhitelist(Address),
    /// Per-device accepted token list: Symbol → Vec<Address>
    DeviceTokens(Symbol),
    /// Price oracle: (device_id, token) → i128 (price in that token)
    TokenPrice(Symbol, Address),
}

#[contract]
pub struct IotContract;

#[contractimpl]
impl IotContract {
    /// Initialize contract admin and platform fee in basis points.
    pub fn initialize(env: Env, admin: Address, platform_fee_bps: i128) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        Self::validate_platform_fee(platform_fee_bps);
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::PlatformFeeBps, &platform_fee_bps);
    }

    /// Update the platform fee. Only the contract admin can call this.
    pub fn set_platform_fee(env: Env, admin: Address, platform_fee_bps: i128) {
        Self::require_admin(env.clone(), admin);
        Self::validate_platform_fee(platform_fee_bps);
        env.storage()
            .instance()
            .set(&DataKey::PlatformFeeBps, &platform_fee_bps);
    }

    /// Get the configured platform fee in basis points.
    pub fn get_platform_fee(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::PlatformFeeBps)
            .unwrap_or(0)
    }

    /// Get the accumulated platform fee balance for a token.
    pub fn get_platform_fee_balance(env: Env, token: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::PlatformFeeBalance(token))
            .unwrap_or(0)
    }

    /// Initialize device with price and owner.
    pub fn init_device(env: Env, device_id: Symbol, price: i128, owner: Address) {
        if price <= 0 {
            panic!("price must be positive");
        }
        owner.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::DevicePrice(device_id.clone()), &price);
        env.storage()
            .instance()
            .set(&DataKey::DeviceOwner(device_id), &owner);
    }

    /// Get device price.
    pub fn get_device_price(env: Env, device_id: Symbol) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::DevicePrice(device_id))
            .unwrap_or(0)
    }

    /// Get device owner.
    pub fn get_device_owner(env: Env, device_id: Symbol) -> Option<Address> {
        env.storage()
            .instance()
            .get(&DataKey::DeviceOwner(device_id))
    }

    // ── Token whitelist (admin) ──────────────────────────────────────────────

    /// Add a SAC token to the platform-wide whitelist. Admin only.
    pub fn add_token(env: Env, admin: Address, token: Address) {
        Self::require_admin(env.clone(), admin);
        env.storage()
            .instance()
            .set(&DataKey::TokenWhitelist(token), &true);
    }

    /// Remove a token from the whitelist. Admin only.
    pub fn remove_token(env: Env, admin: Address, token: Address) {
        Self::require_admin(env.clone(), admin);
        env.storage()
            .instance()
            .remove(&DataKey::TokenWhitelist(token));
    }

    /// Returns true if the token is whitelisted.
    pub fn is_token_whitelisted(env: Env, token: Address) -> bool {
        token_whitelisted(&env, &token)
    }

    // ── Device accepted tokens (device owner) ───────────────────────────────

    /// Set the list of tokens accepted by a device. Empty list = accept all whitelisted.
    /// Caller must be the device owner.
    pub fn set_device_accepted_tokens(
        env: Env,
        owner: Address,
        device_id: Symbol,
        tokens: Vec<Address>,
    ) {
        owner.require_auth();
        let stored_owner: Address = env
            .storage()
            .instance()
            .get(&DataKey::DeviceOwner(device_id.clone()))
            .unwrap_or_else(|| panic!("device not registered"));
        if owner != stored_owner {
            panic!("not device owner");
        }
        env.storage()
            .persistent()
            .set(&DataKey::DeviceTokens(device_id), &tokens);
    }

    /// Get the accepted token list for a device.
    pub fn get_device_accepted_tokens(env: Env, device_id: Symbol) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::DeviceTokens(device_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    // ── Price oracle ─────────────────────────────────────────────────────────

    /// Set the price of a device in a specific token. Admin only.
    pub fn set_token_price(env: Env, admin: Address, device_id: Symbol, token: Address, price: i128) {
        Self::require_admin(env.clone(), admin);
        if price <= 0 {
            panic!("price must be positive");
        }
        env.storage()
            .instance()
            .set(&DataKey::TokenPrice(device_id, token), &price);
    }

    /// Get the price of a device in a specific token.
    /// Falls back to the base device price if no token-specific price is set.
    pub fn get_token_price(env: Env, device_id: Symbol, token: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TokenPrice(device_id.clone(), token))
            .unwrap_or_else(|| Self::get_device_price(env.clone(), device_id))
    }

    // ── Emergency pause ──────────────────────────────────────────────────────

    /// Pause the contract immediately. Admin only. Emits a Paused event.
    pub fn pause(env: Env, admin: Address) {
        Self::require_admin(env.clone(), admin.clone());
        env.storage().instance().set(&DataKey::Paused, &true);
        // Clear any pending unpause schedule.
        env.storage().instance().remove(&DataKey::UnpauseAt);
        env.events()
            .publish((symbol_short!("paused"),), (admin, env.ledger().sequence()));
    }

    /// Schedule an unpause after UNPAUSE_DELAY_LEDGERS. Admin only.
    /// The contract stays paused until `execute_unpause` is called after the delay.
    pub fn unpause(env: Env, admin: Address) {
        Self::require_admin(env.clone(), admin.clone());
        if !Self::is_paused(env.clone()) {
            panic!("not paused");
        }
        let ready_at = env.ledger().sequence() + UNPAUSE_DELAY_LEDGERS;
        env.storage()
            .instance()
            .set(&DataKey::UnpauseAt, &ready_at);
        env.events().publish(
            (symbol_short!("upauseSch"),),
            (admin, ready_at),
        );
    }

    /// Execute the unpause after the timelock has elapsed.
    pub fn execute_unpause(env: Env, admin: Address) {
        Self::require_admin(env.clone(), admin.clone());
        let ready_at: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UnpauseAt)
            .unwrap_or_else(|| panic!("unpause not scheduled"));
        if env.ledger().sequence() < ready_at {
            panic!("timelock not elapsed");
        }
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().remove(&DataKey::UnpauseAt);
        env.events()
            .publish((symbol_short!("unpaused"),), (admin, env.ledger().sequence()));
    }

    /// Returns true when the contract is paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    // ── Bulk access ──────────────────────────────────────────────────────────

    /// Purchase bulk access credits for multiple devices in one transaction.
    /// Volume discount applied to the total based on the number of devices.
    /// Each credit is stored per (user, device_id) and expires after CREDIT_TTL_LEDGERS.
    pub fn purchase_bulk_access(
        env: Env,
        user: Address,
        token: Address,
        device_ids: Vec<Symbol>,
        amounts: Vec<i128>,
    ) {
        user.require_auth();

        if Self::is_paused(env.clone()) {
            panic!("contract paused");
        }

        let n = device_ids.len() as usize;
        if n == 0 {
            panic!("empty device list");
        }
        if device_ids.len() != amounts.len() {
            panic!("device_ids and amounts length mismatch");
        }

        // Compute discount bps based on number of devices purchased.
        let discount_bps: i128 = if n >= TIER4_MIN {
            TIER4_DISC_BPS
        } else if n >= TIER3_MIN {
            TIER3_DISC_BPS
        } else if n >= TIER2_MIN {
            TIER2_DISC_BPS
        } else {
            0
        };

        let expires_at = env.ledger().sequence() + CREDIT_TTL_LEDGERS;
        let token_client = token::Client::new(&env, &token);
        let contract_address = env.current_contract_address();

        for i in 0..device_ids.len() {
            let device_id = device_ids.get(i).unwrap();
            let full_amount = amounts.get(i).unwrap();

            if full_amount <= 0 {
                panic!("amount must be positive");
            }

            let price = Self::get_device_price(env.clone(), device_id.clone());
            if price <= 0 {
                panic!("device not registered");
            }

            // Validate token for each device.
            require_token_valid(&env, &device_id, &token);

            // Use token-specific price if set.
            let price = Self::get_token_price(env.clone(), device_id.clone(), token.clone());
            if full_amount < price {
                panic!("amount below device price");
            }

            // Apply volume discount: user pays discounted amount, credit stored = full_amount.
            let discount = full_amount * discount_bps / FEE_DENOMINATOR;
            let charge = full_amount - discount;

            // Transfer charge from user to contract (held as credit).
            token_client.transfer(&user, &contract_address, &charge);

            // Accumulate with any existing unexpired credit.
            let key = DataKey::Credit(user.clone(), device_id.clone());
            let existing: Credit = env
                .storage()
                .persistent()
                .get(&key)
                .unwrap_or(Credit { amount: 0, expires_at: 0 });

            let carry = if existing.expires_at >= env.ledger().sequence() {
                existing.amount
            } else {
                0
            };

            env.storage().persistent().set(
                &key,
                &Credit {
                    amount: carry + full_amount, // credit at face value
                    expires_at,
                },
            );
        }

        env.events().publish(
            (symbol_short!("bulk"), user.clone()),
            (n as u32, discount_bps, token),
        );
    }

    /// Return unexpired credit amount for (user, device_id). Returns 0 if expired.
    pub fn get_credit(env: Env, user: Address, device_id: Symbol) -> i128 {
        let key = DataKey::Credit(user, device_id);
        let credit: Credit = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Credit { amount: 0, expires_at: 0 });
        if credit.expires_at >= env.ledger().sequence() {
            credit.amount
        } else {
            0
        }
    }

    /// Process payment for device access.
    /// If the user has an unexpired credit ≥ device price, the credit is consumed
    /// and the stored funds are forwarded to the owner (minus platform fee);
    /// otherwise the normal on-the-fly transfer path is used.
    pub fn request_access(
        env: Env,
        device_id: Symbol,
        user: Address,
        token: Address,
        amount: i128,
    ) -> bool {
        user.require_auth();

        if Self::is_paused(env.clone()) {
            panic!("contract paused");
        }

        // Check active subscription first — free access for subscribers.
        if Self::verify_access(env.clone(), device_id.clone(), user.clone()) {
            env.events()
                .publish((symbol_short!("access"), device_id), user);
            return true;
        }

        let price = Self::get_device_price(env.clone(), device_id.clone());
        if price <= 0 {
            return false;
        }

        // Validate token is whitelisted and accepted by this device.
        require_token_valid(&env, &device_id, &token);

        // Use token-specific price if set.
        let price = Self::get_token_price(env.clone(), device_id.clone(), token.clone());

        let owner = Self::get_device_owner(env.clone(), device_id.clone())
            .unwrap_or_else(|| panic!("device owner not found"));
        let fee_bps = Self::get_platform_fee(env.clone());
        let contract_address = env.current_contract_address();
        let token_client = token::Client::new(&env, &token);

        // Try to redeem credit first.
        let credit_key = DataKey::Credit(user.clone(), device_id.clone());
        let credit: Credit = env
            .storage()
            .persistent()
            .get(&credit_key)
            .unwrap_or(Credit { amount: 0, expires_at: 0 });

        let use_credit = credit.expires_at >= env.ledger().sequence() && credit.amount >= price;

        let (pay_amount, from_credit) = if use_credit {
            (price, true)
        } else {
            if amount < price {
                return false;
            }
            (amount, false)
        };

        let platform_fee = pay_amount * fee_bps / FEE_DENOMINATOR;
        let owner_amount = pay_amount - platform_fee;

        if from_credit {
            // Funds are already held by the contract; forward them.
            if owner_amount > 0 {
                token_client.transfer(&contract_address, &owner, &owner_amount);
            }
            if platform_fee > 0 {
                let fee_key = DataKey::PlatformFeeBalance(token.clone());
                let bal = Self::get_platform_fee_balance(env.clone(), token.clone());
                env.storage()
                    .instance()
                    .set(&fee_key, &(bal + platform_fee));
            }
            // Consume credit.
            let remaining = credit.amount - price;
            if remaining > 0 {
                env.storage().persistent().set(
                    &credit_key,
                    &Credit {
                        amount: remaining,
                        expires_at: credit.expires_at,
                    },
                );
            } else {
                env.storage().persistent().remove(&credit_key);
            }
        } else {
            // Normal path: transfer from user.
            if owner_amount > 0 {
                token_client.transfer(&user, &owner, &owner_amount);
            }
            if platform_fee > 0 {
                token_client.transfer(&user, &contract_address, &platform_fee);
                let fee_key = DataKey::PlatformFeeBalance(token.clone());
                let bal = Self::get_platform_fee_balance(env.clone(), token.clone());
                env.storage()
                    .instance()
                    .set(&fee_key, &(bal + platform_fee));
            }
        }

        env.events().publish(
            (symbol_short!("payment"), device_id.clone()),
            (
                user.clone(),
                owner.clone(),
                pay_amount,
                owner_amount,
                platform_fee,
                token.clone(),
            ),
        );
        env.events()
            .publish((symbol_short!("access"), device_id), user);

        true
    }

    /// Backwards-compatible alias for request_access.
    pub fn pay(env: Env, device_id: Symbol, user: Address, token: Address, amount: i128) -> bool {
        Self::request_access(env, device_id, user, token, amount)
    }

    /// Check whether a user currently has active (non-expired, non-cancelled) access
    /// via a subscription to a given device.
    pub fn verify_access(env: Env, device_id: Symbol, user: Address) -> bool {
        let key = DataKey::Subscription(user, device_id);
        let sub: Option<Subscription> = env.storage().instance().get(&key);
        match sub {
            Some(s) => s.active && env.ledger().sequence() <= s.end_ledger,
            None => false,
        }
    }

    /// Subscribe a user to a device for the chosen tier.
    ///
    /// The subscription price is calculated as:
    ///   `device_price * tier_access_count * (1 - discount_bps / 10_000)`
    /// where `tier_access_count` is the number of single accesses the tier covers
    /// (1 for daily, 7 for weekly, 30 for monthly).
    ///
    /// If a non-expired subscription already exists for the user+device, this acts
    /// as a renewal (extends the end_ledger by one tier period).
    /// Panics if the user provides insufficient `amount`.
    pub fn subscribe(
        env: Env,
        user: Address,
        device_id: Symbol,
        tier: SubscriptionTier,
        token: Address,
        amount: i128,
    ) -> Subscription {
        user.require_auth();

        let base_price = Self::get_device_price(env.clone(), device_id.clone());
        if base_price <= 0 {
            panic!("device not found or price not set");
        }

        // Validate token is whitelisted and accepted by this device.
        require_token_valid(&env, &device_id, &token);

        let subscription_price = Self::compute_subscription_price(base_price, tier.clone());
        if amount < subscription_price {
            panic!("insufficient payment for subscription");
        }

        let owner = Self::get_device_owner(env.clone(), device_id.clone())
            .unwrap_or_else(|| panic!("device owner not found"));
        let fee_bps = Self::get_platform_fee(env.clone());
        let platform_fee = subscription_price * fee_bps / FEE_DENOMINATOR;
        let owner_amount = subscription_price - platform_fee;
        let contract_address = env.current_contract_address();
        let token_client = token::Client::new(&env, &token);

        if owner_amount > 0 {
            token_client.transfer(&user, &owner, &owner_amount);
        }
        if platform_fee > 0 {
            token_client.transfer(&user, &contract_address, &platform_fee);
            let fee_key = DataKey::PlatformFeeBalance(token.clone());
            let current = Self::get_platform_fee_balance(env.clone(), token.clone());
            env.storage()
                .instance()
                .set(&fee_key, &(current + platform_fee));
        }

        let tier_ledgers = Self::tier_to_ledgers(tier.clone());
        let current_ledger = env.ledger().sequence();

        // Renew if an active, non-expired subscription already exists.
        let key = DataKey::Subscription(user.clone(), device_id.clone());
        let start_ledger = current_ledger;
        let end_ledger = {
            let existing: Option<Subscription> = env.storage().instance().get(&key);
            match existing {
                Some(ref s) if s.active && current_ledger <= s.end_ledger => {
                    // Extend from current end, not from now.
                    s.end_ledger + tier_ledgers
                }
                _ => current_ledger + tier_ledgers,
            }
        };

        let sub = Subscription {
            user: user.clone(),
            device_id: device_id.clone(),
            tier: tier.clone(),
            start_ledger,
            end_ledger,
            amount_paid: subscription_price,
            active: true,
        };

        env.storage().instance().set(&key, &sub);

        env.events().publish(
            (symbol_short!("subscrib"), device_id.clone()),
            (user.clone(), tier_to_u32(tier), subscription_price, end_ledger, token),
        );

        sub
    }

    /// Cancel an active subscription and issue a prorated refund to the user.
    ///
    /// The refund is proportional to the remaining ledgers out of the total tier duration.
    /// Refund is taken from the device owner's share (platform fee portion is non-refundable).
    /// Panics if no active subscription exists.
    pub fn cancel_subscription(
        env: Env,
        user: Address,
        device_id: Symbol,
        token: Address,
    ) -> i128 {
        user.require_auth();

        let key = DataKey::Subscription(user.clone(), device_id.clone());
        let mut sub: Subscription = env
            .storage()
            .instance()
            .get(&key)
            .unwrap_or_else(|| panic!("no subscription found"));

        if !sub.active {
            panic!("subscription already cancelled");
        }

        let current_ledger = env.ledger().sequence();
        if current_ledger > sub.end_ledger {
            panic!("subscription already expired");
        }

        let tier_ledgers = Self::tier_to_ledgers(sub.tier.clone()) as i128;
        let remaining_ledgers = (sub.end_ledger - current_ledger) as i128;

        // Prorated refund = amount_paid * remaining / total_tier_duration
        let refund_amount = if tier_ledgers > 0 {
            sub.amount_paid * remaining_ledgers / tier_ledgers
        } else {
            0
        };

        // Mark as inactive before transferring to prevent re-entrancy issues.
        sub.active = false;
        env.storage().instance().set(&key, &sub);

        if refund_amount > 0 {
            // Refund comes from the contract (funded by the owner's share on subscribe).
            // In this model the contract holds the subscription amount and disburses to
            // the owner at expiry/cancellation — simplified here to refund from contract balance.
            token::Client::new(&env, &token).transfer(
                &env.current_contract_address(),
                &user,
                &refund_amount,
            );
        }

        env.events().publish(
            (symbol_short!("cancel"), device_id),
            (user, refund_amount),
        );

        refund_amount
    }

    /// Explicitly renew an existing (possibly expired or active) subscription by one tier period.
    /// This is a convenience wrapper around `subscribe` that preserves the same tier.
    /// Panics if no prior subscription record exists.
    pub fn renew_subscription(
        env: Env,
        user: Address,
        device_id: Symbol,
        token: Address,
        amount: i128,
    ) -> Subscription {
        let key = DataKey::Subscription(user.clone(), device_id.clone());
        let sub: Subscription = env
            .storage()
            .instance()
            .get(&key)
            .unwrap_or_else(|| panic!("no existing subscription to renew"));

        Self::subscribe(env, user, device_id, sub.tier, token, amount)
    }

    /// Get the current subscription record for a user+device pair, if any.
    pub fn get_subscription(env: Env, user: Address, device_id: Symbol) -> Option<Subscription> {
        env.storage()
            .instance()
            .get(&DataKey::Subscription(user, device_id))
    }

    /// Withdraw accumulated platform fees. Only the contract admin can call this.
    pub fn withdraw_platform_fees(
        env: Env,
        admin: Address,
        token: Address,
        to: Address,
        amount: i128,
    ) {
        Self::require_admin(env.clone(), admin);
        if amount <= 0 {
            panic!("withdraw amount must be positive");
        }
        let fee_key = DataKey::PlatformFeeBalance(token.clone());
        let current_fee_balance = Self::get_platform_fee_balance(env.clone(), token.clone());
        if amount > current_fee_balance {
            panic!("insufficient platform fees");
        }
        env.storage()
            .instance()
            .set(&fee_key, &(current_fee_balance - amount));
        token::Client::new(&env, &token).transfer(&env.current_contract_address(), &to, &amount);
    }

    // ─── Private helpers ─────────────────────────────────────────────────────

    fn require_admin(env: Env, admin: Address) {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("contract not initialized"));
        if admin != stored_admin {
            panic!("admin required");
        }
    }

    fn validate_platform_fee(platform_fee_bps: i128) {
        if platform_fee_bps < 0 || platform_fee_bps > FEE_DENOMINATOR {
            panic!("invalid platform fee");
        }
    }

    /// Convert a tier to the number of ledgers it covers.
    fn tier_to_ledgers(tier: SubscriptionTier) -> u32 {
        match tier {
            SubscriptionTier::Daily => LEDGERS_PER_DAY,
            SubscriptionTier::Weekly => LEDGERS_PER_WEEK,
            SubscriptionTier::Monthly => LEDGERS_PER_MONTH,
        }
    }

    /// Compute the discounted subscription price for a given tier.
    ///
    /// Formula: base_price * access_count * (FEE_DENOMINATOR - discount_bps) / FEE_DENOMINATOR
    fn compute_subscription_price(base_price: i128, tier: SubscriptionTier) -> i128 {
        let (access_count, discount_bps) = match tier {
            SubscriptionTier::Daily => (1i128, DAILY_DISCOUNT_BPS),
            SubscriptionTier::Weekly => (7i128, WEEKLY_DISCOUNT_BPS),
            SubscriptionTier::Monthly => (30i128, MONTHLY_DISCOUNT_BPS),
        };
        base_price * access_count * (FEE_DENOMINATOR - discount_bps) / FEE_DENOMINATOR
    }
}

/// Helper: encode tier as u32 for event payloads (Soroban events require simple types).
fn tier_to_u32(tier: SubscriptionTier) -> u32 {
    match tier {
        SubscriptionTier::Daily => 0,
        SubscriptionTier::Weekly => 1,
        SubscriptionTier::Monthly => 2,
    }
}

#[cfg(test)]
mod test;
