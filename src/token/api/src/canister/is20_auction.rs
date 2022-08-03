//! This module contains APIs from IS20 standard providing cycle auction related functionality.

use std::collections::HashMap;

use candid::{CandidType, Deserialize, Principal};
use ic_canister::ic_kit::ic;
use ic_helpers::tokens::Tokens128;

use crate::account::Account;
use crate::canister::erc20_transactions::transfer_balance;
use crate::ledger::Ledger;
use crate::state::{AuctionHistory, Balances, BiddingState, CanisterState};
use crate::types::{AuctionInfo, Cycles, StatsData, Timestamp};

use super::TokenCanisterAPI;

// Minimum bidding amount is required, for every update call costs cycles, and we want bidding
// to add cycles rather then to decrease them. 1M is chosen as one ingress call costs 590K cycles.
const MIN_BIDDING_AMOUNT: Cycles = 1_000_000;

/// Current information about upcoming auction and current cycle bids.
#[derive(CandidType, Debug, Clone, Deserialize)]
pub struct BiddingInfo {
    /// Proportion of the transaction fees that will be distributed to the auction participants.
    ///
    /// The value of 1.0 means that all fees go to the auction, 0.0 means that all the fees go to
    /// the canister owner.
    fee_ratio: f64,

    /// Timestamp of the last auction.
    last_auction: Timestamp,

    /// Period of performing auctions. Auction cannot be started before `last_auction + auction_period`
    /// IC time.
    auction_period: Timestamp,

    /// Total cycles accumulated since the last auction.
    total_cycles: Cycles,

    /// The amount of cycles the caller bid for the upcoming auction.
    caller_cycles: Cycles,

    /// The amount of fees accumulated since the last auction and that will be distributed on the
    /// next auction.
    accumulated_fees: Tokens128,
}

#[derive(CandidType, Debug, Clone, Deserialize, PartialEq)]
pub enum AuctionError {
    /// Provided cycles in the `bid_cycles` call is less then the minimum allowed amount.
    BiddingTooSmall,

    /// There are no cycle bids pending, so the auction cannot be held.
    NoBids,

    /// Auction with the given ID is not found.
    AuctionNotFound,

    /// The specified period between the auctions is not passed yet.
    TooEarlyToBeginAuction,
}

pub(crate) fn bid_cycles(
    canister: &impl TokenCanisterAPI,
    bidder: Principal,
) -> Result<Cycles, AuctionError> {
    let amount = ic::msg_cycles_available();
    if amount < MIN_BIDDING_AMOUNT {
        return Err(AuctionError::BiddingTooSmall);
    }
    let state = canister.state();
    let mut state = state.borrow_mut();
    let bidding_state = &mut state.bidding_state;

    let amount_accepted = ic::msg_cycles_accept(amount);
    bidding_state.cycles_since_auction += amount_accepted;
    *bidding_state.bids.entry(bidder).or_insert(0) += amount_accepted;

    Ok(amount_accepted)
}

pub(crate) fn bidding_info(canister: &impl TokenCanisterAPI) -> BiddingInfo {
    let state = canister.state();
    let state = state.borrow();
    let bidding_state = &state.bidding_state;
    let balances = &state.balances;

    BiddingInfo {
        fee_ratio: bidding_state.fee_ratio,
        last_auction: bidding_state.last_auction,
        auction_period: bidding_state.auction_period,
        total_cycles: bidding_state.cycles_since_auction,
        caller_cycles: bidding_state.bids.get(&ic::caller()).cloned().unwrap_or(0),
        accumulated_fees: accumulated_fees(balances),
    }
}

pub(crate) fn run_auction(canister: &impl TokenCanisterAPI) -> Result<AuctionInfo, AuctionError> {
    let state = canister.state();
    let mut state = state.borrow_mut();

    if !state.bidding_state.is_auction_due() {
        return Err(AuctionError::TooEarlyToBeginAuction);
    }

    let CanisterState {
        ref mut bidding_state,
        ref mut balances,
        ref mut auction_history,
        ref mut ledger,
        ref stats,
        ..
    } = &mut *state;

    let result = perform_auction(ledger, bidding_state, balances, auction_history);
    reset_bidding_state(stats, bidding_state);

    result
}

pub(crate) fn auction_info(
    canister: &impl TokenCanisterAPI,
    id: usize,
) -> Result<AuctionInfo, AuctionError> {
    canister
        .state()
        .borrow()
        .auction_history
        .0
        .get(id)
        .cloned()
        .ok_or(AuctionError::AuctionNotFound)
}

fn perform_auction(
    ledger: &mut Ledger,
    bidding_state: &mut BiddingState,
    balances: &mut Balances,
    auction_history: &mut AuctionHistory,
) -> Result<AuctionInfo, AuctionError> {
    if bidding_state.bids.is_empty() {
        return Err(AuctionError::NoBids);
    }

    let total_amount = accumulated_fees(balances);
    let mut transferred_amount = Tokens128::from(0u128);
    let total_cycles = bidding_state.cycles_since_auction;

    let first_id = ledger.len();

    for (bidder, cycles) in &bidding_state.bids {
        let amount = (total_amount * cycles / total_cycles)
            .expect("total cycles is not 0 checked by bids existing")
            .to_tokens128()
            .expect("total cycles is smaller then single user bid cycles");
        transfer_balance(balances, auction_account(), (*bidder).into(), amount)
            .expect("auction principal always have enough balance");
        ledger.auction(*bidder, amount);
        transferred_amount =
            (transferred_amount + amount).expect("can never be larger than total_supply");
    }

    let last_id = ledger.len() - 1;
    let result = AuctionInfo {
        auction_id: auction_history.0.len(),
        auction_time: ic::time(),
        tokens_distributed: transferred_amount,
        cycles_collected: total_cycles,
        fee_ratio: bidding_state.fee_ratio,
        first_transaction_id: first_id,
        last_transaction_id: last_id,
    };

    auction_history.0.push(result.clone());

    Ok(result)
}

fn reset_bidding_state(stats: &StatsData, bidding_state: &mut BiddingState) {
    bidding_state.fee_ratio = get_fee_ratio(stats.min_cycles, ic::balance());
    bidding_state.cycles_since_auction = 0;
    bidding_state.last_auction = ic::time();
    bidding_state.bids = HashMap::new();
}

fn get_fee_ratio(min_cycles: Cycles, current_cycles: Cycles) -> f64 {
    let min_cycles = min_cycles as f64;
    let current_cycles = current_cycles as f64;
    if min_cycles == 0.0 {
        // Setting min_cycles to zero effectively turns off the auction functionality, as all the
        // fees will go to the owner.
        0.0
    } else if current_cycles <= min_cycles {
        1.0
    } else {
        // If current cycles are 10 times larger, then min_cycles, half of the fees go to the auction.
        // If current cycles are 1000 times larger, 17% of the fees go to the auction.
        2f64.powf((min_cycles / current_cycles).log10())
    }
}

pub fn auction_principal() -> Principal {
    // The management canister is not a real canister in IC, so it's usually used as a black hole
    // principal. In our case, we can use this principal as a balance holder for the auction tokens,
    // as no requests can ever be made from this principal.
    Principal::management_canister()
}

pub fn auction_account() -> Account {
    // There are no sub accounts for the auction principal
    Account::new(Principal::management_canister(), None)
}

pub fn accumulated_fees(balances: &Balances) -> Tokens128 {
    balances.balance_of(auction_account())
}

#[cfg(test)]
mod tests {
    use ic_canister::ic_kit::mock_principals::{alice, bob};
    use ic_canister::ic_kit::MockContext;
    use ic_canister::Canister;
    use test_case::test_case;

    use crate::mock::*;
    use crate::types::{Metadata, TxError};

    use super::*;

    fn test_context() -> (&'static mut MockContext, TokenCanisterMock) {
        let context = MockContext::new().with_caller(alice()).inject();

        let canister = TokenCanisterMock::init_instance();
        canister.init(
            Metadata {
                logo: "".to_string(),
                name: "".to_string(),
                symbol: "".to_string(),
                decimals: 8,
                owner: alice(),
                fee: Tokens128::from(0),
                feeTo: alice(),
                isTestToken: None,
            },
            Tokens128::from(1000),
        );

        (context, canister)
    }

    #[test_case(0, 0, 0.0)]
    #[test_case(0, 1000, 0.0)]
    #[test_case(1000, 0, 1.0)]
    #[test_case(1000, 1000, 1.0)]
    #[test_case(1000, 10_000, 0.5)]
    #[test_case(1000, 1_000_000, 0.125)]
    fn fee_ratio_tests(min_cycles: u64, current_cycles: u64, ratio: f64) {
        assert_eq!(get_fee_ratio(min_cycles, current_cycles), ratio);
    }

    #[test]
    fn bidding_cycles() {
        let (context, canister) = test_context();
        context.update_caller(bob());
        context.update_msg_cycles(2_000_000);

        canister.bidCycles(bob()).unwrap();
        let info = canister.biddingInfo();
        assert_eq!(info.total_cycles, 2_000_000);
        assert_eq!(info.caller_cycles, 2_000_000);

        context.update_caller(alice());
        let info = canister.biddingInfo();
        assert_eq!(info.total_cycles, 2_000_000);
        assert_eq!(info.caller_cycles, 0);
    }

    #[test]
    fn bidding_cycles_under_limit() {
        let (context, canister) = test_context();
        context.update_msg_cycles(MIN_BIDDING_AMOUNT - 1);
        assert_eq!(
            canister.bidCycles(alice()),
            Err(AuctionError::BiddingTooSmall)
        );
    }

    #[test]
    fn bidding_multiple_times() {
        let (context, canister) = test_context();
        context.update_msg_cycles(2_000_000);
        canister.bidCycles(alice()).unwrap();

        context.update_msg_cycles(2_000_000);
        canister.bidCycles(alice()).unwrap();

        assert_eq!(canister.biddingInfo().caller_cycles, 4_000_000);
    }

    #[test]
    fn auction_test() {
        let (context, canister) = test_context();
        context.update_msg_cycles(2_000_000);
        bid_cycles(&canister, alice()).unwrap();

        context.update_msg_cycles(4_000_000);
        bid_cycles(&canister, bob()).unwrap();

        canister.state().borrow_mut().balances.insert(
            auction_principal(),
            None,
            Tokens128::from(6000),
        );

        canister
            .state()
            .borrow()
            .balances
            .balance_of(auction_account());

        let result = canister.runAuction().unwrap();
        assert_eq!(result.cycles_collected, 6_000_000);
        assert_eq!(result.first_transaction_id, 1);
        assert_eq!(result.last_transaction_id, 2);
        assert_eq!(result.tokens_distributed, Tokens128::from(6_000));

        assert_eq!(
            canister.state().borrow().balances.balance_of(bob().into()),
            Tokens128::from(4_000)
        );

        let retrieved_result = canister.auctionInfo(result.auction_id).unwrap();
        assert_eq!(retrieved_result, result);
    }

    #[test]
    fn auction_without_bids() {
        let (_, canister) = test_context();
        assert_eq!(canister.runAuction(), Err(AuctionError::NoBids));
    }

    #[test]
    fn auction_not_in_time() {
        let (context, canister) = test_context();
        context.update_msg_cycles(2_000_000);
        canister.bidCycles(alice()).unwrap();

        {
            let state = canister.state();
            let state = &mut state.borrow_mut().bidding_state;
            state.last_auction = ic::time() - 100_000;
            state.auction_period = 1_000_000_000;
        }

        assert_eq!(
            canister.runAuction(),
            Err(AuctionError::TooEarlyToBeginAuction)
        );
    }

    #[test]
    fn fee_ratio_update() {
        let (context, canister) = test_context();
        context.update_balance(1_000_000_000);

        canister.state().borrow_mut().stats.min_cycles = 1_000_000;
        canister.runAuction().unwrap_err();

        assert_eq!(canister.state().borrow().bidding_state.fee_ratio, 0.125);
    }

    #[test]
    fn setting_min_cycles() {
        let (_, canister) = test_context();
        canister.setMinCycles(100500).unwrap();
        assert_eq!(canister.getMinCycles(), 100500);
    }

    #[test]
    fn setting_min_cycles_not_authorized() {
        let (context, canister) = test_context();
        context.update_caller(bob());
        assert_eq!(canister.setMinCycles(100500), Err(TxError::Unauthorized));
    }

    #[test]
    fn setting_auction_period() {
        let (_, canister) = test_context();
        canister.setAuctionPeriod(100500).unwrap();
        assert_eq!(canister.biddingInfo().auction_period, 100500 * 1000000);
    }

    #[test]
    fn setting_auction_period_not_authorized() {
        let (context, canister) = test_context();
        context.update_caller(bob());
        assert_eq!(
            canister.setAuctionPeriod(100500),
            Err(TxError::Unauthorized)
        );
    }
}
