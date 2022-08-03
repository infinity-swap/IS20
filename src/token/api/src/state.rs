use crate::ledger::Ledger;
use crate::types::{Allowances, Metadata, StatsData};
use candid::{CandidType, Deserialize, Principal};
use ic_auction::AuctionState;
use ic_helpers::tokens::Tokens128;
use ic_storage::stable::Versioned;
use ic_storage::IcStorage;
use std::collections::HashMap;

#[derive(Debug, Default, CandidType, Deserialize, IcStorage)]
pub struct CanisterState {
    pub balances: Balances,
    pub stats: StatsData,
    pub allowances: Allowances,
    pub ledger: Ledger,
}

impl CanisterState {
    pub fn get_metadata(&self) -> Metadata {
        Metadata {
            logo: self.stats.logo.clone(),
            name: self.stats.name.clone(),
            symbol: self.stats.symbol.clone(),
            decimals: self.stats.decimals,
            total_supply: self.stats.total_supply,
            owner: self.stats.owner,
            fee: self.stats.fee,
            feeTo: self.stats.fee_to,
            is_test_token: Some(self.stats.is_test_token),
        }
    }

    pub fn allowance(&self, owner: Principal, spender: Principal) -> Tokens128 {
        match self.allowances.get(&owner) {
            Some(inner) => match inner.get(&spender) {
                Some(value) => *value,
                None => Tokens128::from(0u128),
            },
            None => Tokens128::from(0u128),
        }
    }

    pub fn allowance_size(&self) -> usize {
        self.allowances
            .iter()
            .map(|(_, v)| v.len())
            .reduce(|accum, v| accum + v)
            .unwrap_or(0)
    }

    pub fn user_approvals(&self, who: Principal) -> Vec<(Principal, Tokens128)> {
        match self.allowances.get(&who) {
            Some(allow) => Vec::from_iter(allow.clone().into_iter()),
            None => Vec::new(),
        }
    }
}
impl Versioned for CanisterState {
    type Previous = ();

    fn upgrade((): ()) -> Self {
        Self::default()
    }
}

#[derive(Debug, Default, CandidType, Deserialize)]
pub struct Balances(pub HashMap<Principal, Tokens128>);

impl Balances {
    pub fn balance_of(&self, who: &Principal) -> Tokens128 {
        self.0
            .get(who)
            .cloned()
            .unwrap_or_else(|| Tokens128::from(0u128))
    }

    pub fn get_holders(&self, start: usize, limit: usize) -> Vec<(Principal, Tokens128)> {
        let mut balance = self.0.iter().map(|(&k, v)| (k, *v)).collect::<Vec<_>>();

        // Sort balance and principals by the balance
        balance.sort_by(|a, b| b.1.cmp(&a.1));

        let end = (start + limit).min(balance.len());
        balance[start..end].to_vec()
    }
}

/// A wrapper over stable state that is used only during upgrade process.
/// Since we have two different stable states (canister and auction), we need
/// to wrap it in this struct during canister upgrade.
#[derive(CandidType, Deserialize, Default)]
pub struct StableState {
    pub token_state: CanisterState,
    pub auction_state: AuctionState,
}

impl Versioned for StableState {
    type Previous = ();

    fn upgrade(_prev_state: Self::Previous) -> Self {
        Self::default()
    }
}
