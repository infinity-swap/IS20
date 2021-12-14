use crate::state::State;
use crate::types::{Metadata, TokenInfo, TxError, TxReceipt};
use candid::{candid_method, Nat};
use ic_cdk_macros::*;
use ic_kit::{ic, Principal};
use std::collections::HashMap;
use std::iter::FromIterator;
use std::string::String;

fn _transfer(from: Principal, to: Principal, value: Nat) {
    let balances = State::get().balances_mut();
    let from_balance = balance_of(from);
    let from_balance_new = from_balance - value.clone();
    if from_balance_new != 0 {
        balances.insert(from, from_balance_new);
    } else {
        balances.remove(&from);
    }
    let to_balance = balance_of(to);
    let to_balance_new = to_balance + value;
    if to_balance_new != 0 {
        balances.insert(to, to_balance_new);
    }
}

fn _charge_fee(user: Principal, fee_to: Principal, fee: Nat) {
    let stats = State::get().stats();
    if stats.fee > 0u32 {
        _transfer(user, fee_to, fee);
    }
}

#[update(name = "transfer")]
#[candid_method(update)]
fn transfer(to: Principal, value: Nat) -> TxReceipt {
    let from = ic::caller();
    let stats = State::get().stats_mut();
    if balance_of(from) < value.clone() + stats.fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(from, stats.fee_to, stats.fee.clone());
    _transfer(from, to, value.clone());

    let id = State::get()
        .ledger_mut()
        .transfer(from, to, value, stats.fee.clone());
    Ok(id)
}

#[update(name = "transferFrom")]
#[candid_method(update, rename = "transferFrom")]
fn transfer_from(from: Principal, to: Principal, value: Nat) -> TxReceipt {
    let owner = ic::caller();
    let from_allowance = allowance(from, owner);
    let stats = State::get().stats_mut();
    if from_allowance < value.clone() + stats.fee.clone() {
        return Err(TxError::InsufficientAllowance);
    }
    let from_balance = balance_of(from);
    if from_balance < value.clone() + stats.fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(from, stats.fee_to, stats.fee.clone());
    _transfer(from, to, value.clone());
    let allowances = State::get().allowances_mut();
    match allowances.get(&from) {
        Some(inner) => {
            let result = inner.get(&owner).unwrap().clone();
            let mut temp = inner.clone();
            if result.clone() - value.clone() - stats.fee.clone() != 0 {
                temp.insert(owner, result - value.clone() - stats.fee.clone());
                allowances.insert(from, temp);
            } else {
                temp.remove(&owner);
                if temp.is_empty() {
                    allowances.remove(&from);
                } else {
                    allowances.insert(from, temp);
                }
            }
        }
        None => {
            panic!()
        }
    }

    let id = State::get()
        .ledger_mut()
        .transfer_from(owner, from, to, value, stats.fee.clone());
    Ok(id)
}

#[update(name = "approve")]
#[candid_method(update)]
fn approve(spender: Principal, value: Nat) -> TxReceipt {
    let owner = ic::caller();
    let stats = State::get().stats_mut();
    if balance_of(owner) < stats.fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(owner, stats.fee_to, stats.fee.clone());
    let v = value.clone() + stats.fee.clone();
    let allowances = State::get().allowances_mut();
    match allowances.get(&owner) {
        Some(inner) => {
            let mut temp = inner.clone();
            if v != 0 {
                temp.insert(spender, v);
                allowances.insert(owner, temp);
            } else {
                temp.remove(&spender);
                if temp.is_empty() {
                    allowances.remove(&owner);
                } else {
                    allowances.insert(owner, temp);
                }
            }
        }
        None => {
            if v != 0 {
                let mut inner = HashMap::new();
                inner.insert(spender, v);
                let allowances = State::get().allowances_mut();
                allowances.insert(owner, inner);
            }
        }
    }

    let id = State::get()
        .ledger_mut()
        .approve(owner, spender, value, stats.fee.clone());
    Ok(id)
}

#[update(name = "mint")]
#[candid_method(update, rename = "mint")]
fn mint(to: Principal, amount: Nat) -> TxReceipt {
    let caller = ic::caller();
    let stats = State::get().stats_mut();
    if caller != stats.owner {
        return Err(TxError::Unauthorized);
    }
    let to_balance = balance_of(to);
    let balances = State::get().balances_mut();
    balances.insert(to, to_balance + amount.clone());
    stats.total_supply += amount.clone();

    let id = State::get().ledger_mut().mint(caller, to, amount);
    Ok(id)
}

#[update(name = "burn")]
#[candid_method(update, rename = "burn")]
fn burn(amount: Nat) -> TxReceipt {
    let caller = ic::caller();
    let stats = State::get().stats_mut();
    let caller_balance = balance_of(caller);
    if caller_balance < amount {
        return Err(TxError::InsufficientBalance);
    }
    let balances = State::get().balances_mut();
    balances.insert(caller, caller_balance - amount.clone());
    stats.total_supply -= amount.clone();

    let id = State::get().ledger_mut().burn(caller, amount);
    Ok(id)
}

#[update(name = "setName")]
#[candid_method(update, rename = "setName")]
fn set_name(name: String) {
    let stats = State::get().stats_mut();
    assert_eq!(ic::caller(), stats.owner);
    stats.name = name;
}

#[update(name = "setLogo")]
#[candid_method(update, rename = "setLogo")]
fn set_logo(logo: String) {
    let stats = State::get().stats_mut();
    assert_eq!(ic::caller(), stats.owner);
    stats.logo = logo;
}

#[update(name = "setFee")]
#[candid_method(update, rename = "setFee")]
fn set_fee(fee: Nat) {
    let stats = State::get().stats_mut();
    assert_eq!(ic::caller(), stats.owner);
    stats.fee = fee;
}

#[update(name = "setFeeTo")]
#[candid_method(update, rename = "setFeeTo")]
fn set_fee_to(fee_to: Principal) {
    let stats = State::get().stats_mut();
    assert_eq!(ic::caller(), stats.owner);
    stats.fee_to = fee_to;
}

#[update(name = "setOwner")]
#[candid_method(update, rename = "setOwner")]
fn set_owner(owner: Principal) {
    let stats = State::get().stats_mut();
    assert_eq!(ic::caller(), stats.owner);
    stats.owner = owner;
}

#[query(name = "balanceOf")]
#[candid_method(query, rename = "balanceOf")]
fn balance_of(id: Principal) -> Nat {
    let balances = State::get().balances();
    match balances.get(&id) {
        Some(balance) => balance.clone(),
        None => Nat::from(0),
    }
}

#[query(name = "allowance")]
#[candid_method(query)]
fn allowance(owner: Principal, spender: Principal) -> Nat {
    let allowances = State::get().allowances();
    match allowances.get(&owner) {
        Some(inner) => match inner.get(&spender) {
            Some(value) => value.clone(),
            None => Nat::from(0),
        },
        None => Nat::from(0),
    }
}

#[query(name = "getLogo")]
#[candid_method(query, rename = "getLogo")]
fn get_logo() -> String {
    let stats = State::get().stats();
    stats.logo.clone()
}

#[query(name = "name")]
#[candid_method(query)]
fn name() -> String {
    let stats = State::get().stats();
    stats.name.clone()
}

#[query(name = "symbol")]
#[candid_method(query)]
fn symbol() -> String {
    let stats = State::get().stats();
    stats.symbol.clone()
}

#[query(name = "decimals")]
#[candid_method(query)]
fn decimals() -> u8 {
    let stats = State::get().stats();
    stats.decimals
}

#[query(name = "totalSupply")]
#[candid_method(query, rename = "totalSupply")]
fn total_supply() -> Nat {
    let stats = State::get().stats();
    stats.total_supply.clone()
}

#[query(name = "owner")]
#[candid_method(query)]
fn owner() -> Principal {
    let stats = State::get().stats();
    stats.owner
}

#[query(name = "getMetadata")]
#[candid_method(query, rename = "getMetadata")]
fn get_metadata() -> Metadata {
    let s = State::get().stats();
    Metadata {
        logo: s.logo.clone(),
        name: s.name.clone(),
        symbol: s.symbol.clone(),
        decimals: s.decimals,
        totalSupply: s.total_supply.clone(),
        owner: s.owner,
        fee: s.fee.clone(),
    }
}

#[query(name = "historySize")]
#[candid_method(query, rename = "historySize")]
fn history_size() -> usize {
    let ledger = State::get().ledger();
    ledger.len()
}

#[query(name = "getTokenInfo")]
#[candid_method(query, rename = "getTokenInfo")]
fn get_token_info() -> TokenInfo {
    let stats = State::get().stats().clone();
    let balance = State::get().balances();

    TokenInfo {
        metadata: get_metadata(),
        feeTo: stats.fee_to,
        historySize: history_size(),
        deployTime: stats.deploy_time,
        holderNumber: balance.len(),
        cycles: ic::balance(),
    }
}

#[query(name = "getHolders")]
#[candid_method(query, rename = "getHolders")]
fn get_holders(start: usize, limit: usize) -> Vec<(Principal, Nat)> {
    let mut balance = Vec::new();
    for (k, v) in State::get().balances() {
        balance.push((*k, v.clone()));
    }
    balance.sort_by(|a, b| b.1.cmp(&a.1));
    let limit: usize = if start + limit > balance.len() {
        balance.len() - start
    } else {
        limit
    };
    balance[start..start + limit].to_vec()
}

#[query(name = "getAllowanceSize")]
#[candid_method(query, rename = "getAllowanceSize")]
fn get_allowance_size() -> usize {
    let mut size = 0;
    let allowances = State::get().allowances();
    for (_, v) in allowances.iter() {
        size += v.len();
    }
    size
}

#[query(name = "getUserApprovals")]
#[candid_method(query, rename = "getUserApprovals")]
fn get_user_approvals(who: Principal) -> Vec<(Principal, Nat)> {
    let allowances = State::get().allowances();
    match allowances.get(&who) {
        Some(allow) => Vec::from_iter(allow.clone().into_iter()),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ic_kit::mock_principals::{alice, bob, john};
    use ic_kit::MockContext;
    use std::collections::HashSet;

    fn init_context() -> &'static mut MockContext {
        let context = MockContext::new().with_caller(alice()).inject();

        crate::init(
            "".into(),
            "".into(),
            "".into(),
            8,
            Nat::from(1000),
            alice(),
            Nat::from(0),
            alice(),
        );
        context
    }

    #[test]
    fn transfer_without_fee() {
        init_context();
        assert_eq!(Nat::from(1000), balance_of(alice()));

        assert!(transfer(bob(), Nat::from(100)).is_ok());
        assert_eq!(balance_of(bob()), Nat::from(100));
        assert_eq!(balance_of(alice()), Nat::from(900));
    }

    #[test]
    fn transfer_with_fee() {
        MockContext::new().with_caller(alice()).inject();

        crate::init(
            "".into(),
            "".into(),
            "".into(),
            8,
            Nat::from(1000),
            alice(),
            Nat::from(100),
            john(),
        );

        assert!(transfer(bob(), Nat::from(100)).is_ok());
        assert_eq!(balance_of(bob()), Nat::from(100));
        assert_eq!(balance_of(alice()), Nat::from(800));
        assert_eq!(balance_of(john()), Nat::from(100));
    }

    #[test]
    fn transfer_insufficient_balance() {
        init_context();
        assert_eq!(
            transfer(bob(), Nat::from(1001)),
            Err(TxError::InsufficientBalance)
        );
        assert_eq!(balance_of(alice()), Nat::from(1000));
        assert_eq!(balance_of(bob()), Nat::from(0));
    }

    #[test]
    fn transfer_wrong_caller() {
        let context = init_context();
        context.update_caller(bob());
        assert_eq!(
            transfer(bob(), Nat::from(100)),
            Err(TxError::InsufficientBalance)
        );
        assert_eq!(balance_of(alice()), Nat::from(1000));
        assert_eq!(balance_of(bob()), Nat::from(0));
    }

    #[test]
    fn mint_by_owner() {
        init_context();
        assert!(mint(alice(), Nat::from(2000)).is_ok());
        assert!(mint(bob(), Nat::from(5000)).is_ok());
        assert_eq!(balance_of(alice()), Nat::from(3000));
        assert_eq!(balance_of(bob()), Nat::from(5000));
        assert_eq!(get_metadata().totalSupply, Nat::from(8000));
    }

    #[test]
    fn mint_not_by_owner() {
        let context = init_context();
        context.update_caller(bob());
        assert_eq!(mint(alice(), Nat::from(100)), Err(TxError::Unauthorized));
    }

    #[test]
    fn burn_by_owner() {
        init_context();
        assert!(burn(Nat::from(100)).is_ok());
        assert_eq!(balance_of(alice()), Nat::from(900));
        assert_eq!(get_metadata().totalSupply, Nat::from(900));
    }

    #[test]
    fn burn_too_much() {
        init_context();
        assert_eq!(burn(Nat::from(1001)), Err(TxError::InsufficientBalance));
        assert_eq!(balance_of(alice()), Nat::from(1000));
        assert_eq!(get_metadata().totalSupply, Nat::from(1000));
    }

    #[test]
    fn burn_by_wrong_user() {
        let context = init_context();
        context.update_caller(bob());
        assert_eq!(burn(Nat::from(100)), Err(TxError::InsufficientBalance));
        assert_eq!(balance_of(alice()), Nat::from(1000));
        assert_eq!(get_metadata().totalSupply, Nat::from(1000));
    }

    #[test]
    fn transfer_from_with_approve() {
        let context = init_context();
        assert!(approve(bob(), Nat::from(500)).is_ok());
        context.update_caller(bob());
        assert!(transfer_from(alice(), john(), Nat::from(100)).is_ok());
        assert_eq!(balance_of(alice()), Nat::from(900));
        assert_eq!(balance_of(john()), Nat::from(100));
        assert!(transfer_from(alice(), john(), Nat::from(100)).is_ok());
        assert_eq!(balance_of(alice()), Nat::from(800));
        assert_eq!(balance_of(john()), Nat::from(200));
        assert!(transfer_from(alice(), john(), Nat::from(300)).is_ok());

        assert_eq!(balance_of(alice()), Nat::from(500));
        assert_eq!(balance_of(bob()), Nat::from(0));
        assert_eq!(balance_of(john()), Nat::from(500));
    }

    #[test]
    fn insufficient_allowance() {
        let context = init_context();
        assert!(approve(bob(), Nat::from(500)).is_ok());
        context.update_caller(bob());
        assert_eq!(
            transfer_from(alice(), john(), Nat::from(600)),
            Err(TxError::InsufficientAllowance)
        );
        assert_eq!(balance_of(alice()), Nat::from(1000));
        assert_eq!(balance_of(john()), Nat::from(0));
    }

    #[test]
    fn transfer_from_without_approve() {
        let context = init_context();
        context.update_caller(bob());
        assert_eq!(
            transfer_from(alice(), john(), Nat::from(600)),
            Err(TxError::InsufficientAllowance)
        );
        assert_eq!(balance_of(alice()), Nat::from(1000));
        assert_eq!(balance_of(john()), Nat::from(0));
    }

    #[test]
    fn multiple_approves() {
        init_context();
        assert!(approve(bob(), Nat::from(500)).is_ok());
        assert_eq!(get_user_approvals(alice()), vec![(bob(), Nat::from(500))]);

        assert!(approve(bob(), Nat::from(200)).is_ok());
        assert_eq!(get_user_approvals(alice()), vec![(bob(), Nat::from(200))]);

        assert!(approve(john(), Nat::from(1000)).is_ok());

        // Convert vectors to sets before comparing to make comparison unaffected by the element
        // order.
        assert_eq!(
            HashSet::<&(Principal, Nat)>::from_iter(get_user_approvals(alice()).iter()),
            HashSet::from_iter(vec![(bob(), Nat::from(200)), (john(), Nat::from(1000))].iter())
        );
    }

    #[test]
    fn approve_over_balance() {
        let context = init_context();
        assert!(approve(bob(), Nat::from(1500)).is_ok());
        context.update_caller(bob());
        assert!(transfer_from(alice(), john(), Nat::from(500)).is_ok());
        assert_eq!(balance_of(alice()), Nat::from(500));
        assert_eq!(balance_of(john()), Nat::from(500));

        assert_eq!(
            transfer_from(alice(), john(), Nat::from(600)),
            Err(TxError::InsufficientBalance)
        );
        assert_eq!(balance_of(alice()), Nat::from(500));
        assert_eq!(balance_of(john()), Nat::from(500));
    }

    #[test]
    fn transfer_from_with_fee() {
        let context = MockContext::new().with_caller(alice()).inject();

        crate::init(
            "".into(),
            "".into(),
            "".into(),
            8,
            Nat::from(1000),
            alice(),
            Nat::from(100),
            bob(),
        );
        assert!(approve(bob(), Nat::from(1500)).is_ok());
        assert_eq!(balance_of(bob()), Nat::from(100));
        context.update_caller(bob());

        assert!(transfer_from(alice(), john(), Nat::from(300)).is_ok());
        assert_eq!(balance_of(bob()), Nat::from(200));
        assert_eq!(balance_of(alice()), Nat::from(500));
        assert_eq!(balance_of(john()), Nat::from(300));
    }
}
