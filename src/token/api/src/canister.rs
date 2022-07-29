use std::cell::RefCell;
use std::rc::Rc;

use ic_auction::api::Auction;
use ic_auction::error::AuctionError;
use ic_auction::AuctionInfo;
use ic_auction::AuctionState;
use ic_auction::BiddingInfo;
use ic_canister::generate_exports;
use ic_canister::Canister;
use ic_canister::MethodType;
use ic_cdk::export::candid::Principal;
use ic_helpers::metrics::Interval;
use ic_storage::IcStorage;

use crate::state::CanisterState;

use ic_canister::{query, update, AsyncReturn};
use ic_helpers::tokens::Tokens128;

use crate::canister::erc20_transactions::{
    approve, burn_as_owner, burn_own_tokens, mint_as_owner, mint_test_token, transfer,
    transfer_from,
};
use crate::canister::is20_notify::{approve_and_notify, consume_notification, notify};
use crate::canister::is20_transactions::{batch_transfer, transfer_include_fee};
use crate::principal::{CheckedPrincipal, Owner};
use crate::types::{
    Metadata, PaginatedResult, StatsData, Timestamp, TokenInfo, TxError, TxId, TxReceipt, TxRecord,
};

pub use inspect::AcceptReason;

pub mod erc20_transactions;

mod inspect;

pub mod is20_auction;
pub mod is20_notify;
pub mod is20_transactions;

pub(crate) const MAX_TRANSACTION_QUERY_LEN: usize = 1000;
// 1 day in seconds.
pub const DEFAULT_AUCTION_PERIOD_SECONDS: Timestamp = 60 * 60 * 24;

pub fn pre_update<T: TokenCanisterAPI>(canister: &T, method_name: &str, method_type: MethodType) {
    <T as Auction>::canister_pre_update(canister, method_name, method_type)
}

pub enum CanisterUpdate {
    Name(String),
    Logo(String),
    Fee(Tokens128),
    FeeTo(Principal),
    Owner(Principal),
    MinCycles(u64),
}

#[allow(non_snake_case)]
pub trait TokenCanisterAPI: Canister + Sized + Auction {
    fn state(&self) -> Rc<RefCell<CanisterState>> {
        CanisterState::get()
    }

    /// The `inspect_message()` call is not exported by default. Add your custom #[inspect_message]
    /// function and use this method there to export the `inspect_message()` call.
    fn inspect_message(
        state: &CanisterState,
        method: &str,
        caller: Principal,
    ) -> Result<AcceptReason, &'static str> {
        inspect::inspect_message(state, method, caller)
    }

    #[query(trait = true)]
    fn isTestToken(&self) -> bool {
        self.state().borrow().stats.is_test_token
    }

    #[query(trait = true)]
    fn name(&self) -> String {
        self.state().borrow().stats.name.clone()
    }

    #[query(trait = true)]
    fn symbol(&self) -> String {
        self.state().borrow().stats.symbol.clone()
    }

    #[query(trait = true)]
    fn logo(&self) -> String {
        self.state().borrow().stats.logo.clone()
    }

    #[query(trait = true)]
    fn decimals(&self) -> u8 {
        self.state().borrow().stats.decimals
    }

    #[query(trait = true)]
    fn totalSupply(&self) -> Tokens128 {
        self.state().borrow().stats.total_supply
    }

    #[query(trait = true)]
    fn owner(&self) -> Principal {
        self.state().borrow().stats.owner
    }

    #[query(trait = true)]
    fn getMetadata(&self) -> Metadata {
        self.state().borrow().get_metadata()
    }

    #[query(trait = true)]
    fn getTokenInfo(&self) -> TokenInfo {
        let StatsData {
            fee_to,
            deploy_time,
            ..
        } = self.state().borrow().stats;
        TokenInfo {
            metadata: self.state().borrow().get_metadata(),
            feeTo: fee_to,
            historySize: self.state().borrow().ledger.len(),
            deployTime: deploy_time,
            holderNumber: self.state().borrow().balances.0.len(),
            cycles: ic_canister::ic_kit::ic::balance(),
        }
    }

    #[query(trait = true)]
    fn getHolders(&self, start: usize, limit: usize) -> Vec<(Principal, Tokens128)> {
        self.state().borrow().balances.get_holders(start, limit)
    }

    #[query(trait = true)]
    fn getAllowanceSize(&self) -> usize {
        self.state().borrow().allowance_size()
    }

    #[query(trait = true)]
    fn getUserApprovals(&self, who: Principal) -> Vec<(Principal, Tokens128)> {
        self.state().borrow().user_approvals(who)
    }

    #[query(trait = true)]
    fn balanceOf(&self, holder: Principal) -> Tokens128 {
        self.state().borrow().balances.balance_of(&holder)
    }

    #[query(trait = true)]
    fn allowance(&self, owner: Principal, spender: Principal) -> Tokens128 {
        self.state().borrow().allowance(owner, spender)
    }

    #[query(trait = true)]
    fn historySize(&self) -> u64 {
        self.state().borrow().ledger.len()
    }

    fn update_stats(&self, _caller: CheckedPrincipal<Owner>, update: CanisterUpdate) {
        use CanisterUpdate::*;
        match update {
            Name(name) => self.state().borrow_mut().stats.name = name,
            Logo(logo) => self.state().borrow_mut().stats.logo = logo,
            Fee(fee) => self.state().borrow_mut().stats.fee = fee,
            FeeTo(fee_to) => self.state().borrow_mut().stats.fee_to = fee_to,
            Owner(owner) => self.state().borrow_mut().stats.owner = owner,
            MinCycles(min_cycles) => self.state().borrow_mut().stats.min_cycles = min_cycles,
            // AuctionPeriod(period_sec) => {
            //     self.state().borrow_mut().bidding_state.auction_period = period_sec * 1_000_000
            // }
        }
    }

    #[update(trait = true)]
    fn setName(&self, name: String) -> Result<(), TxError> {
        let caller = CheckedPrincipal::owner(&self.state().borrow_mut().stats)?;
        self.update_stats(caller, CanisterUpdate::Name(name));
        Ok(())
    }

    #[update(trait = true)]
    fn setLogo(&self, logo: String) -> Result<(), TxError> {
        let caller = CheckedPrincipal::owner(&self.state().borrow_mut().stats)?;
        self.update_stats(caller, CanisterUpdate::Logo(logo));
        Ok(())
    }

    #[update(trait = true)]
    fn setFee(&self, fee: Tokens128) -> Result<(), TxError> {
        let caller = CheckedPrincipal::owner(&self.state().borrow_mut().stats)?;
        self.update_stats(caller, CanisterUpdate::Fee(fee));
        Ok(())
    }

    #[update(trait = true)]
    fn setFeeTo(&self, fee_to: Principal) -> Result<(), TxError> {
        let caller = CheckedPrincipal::owner(&self.state().borrow_mut().stats)?;
        self.update_stats(caller, CanisterUpdate::FeeTo(fee_to));
        Ok(())
    }

    #[update(trait = true)]
    fn setOwner(&self, owner: Principal) -> Result<(), TxError> {
        let caller = CheckedPrincipal::owner(&self.state().borrow_mut().stats)?;
        self.update_stats(caller, CanisterUpdate::Owner(owner));
        Ok(())
    }

    #[update(trait = true)]
    fn approve(&self, spender: Principal, amount: Tokens128) -> TxReceipt {
        let caller = CheckedPrincipal::with_recipient(spender)?;
        approve(self, caller, amount)
    }

    /********************** TRANSFERS ***********************/
    #[cfg_attr(feature = "transfer", update(trait = true))]
    fn transfer(
        &self,
        to: Principal,
        amount: Tokens128,
        fee_limit: Option<Tokens128>,
    ) -> TxReceipt {
        let caller = CheckedPrincipal::with_recipient(to)?;
        transfer(self, caller, amount, fee_limit)
    }

    #[cfg_attr(feature = "transfer", update(trait = true))]
    fn transferFrom(&self, from: Principal, to: Principal, amount: Tokens128) -> TxReceipt {
        let caller = CheckedPrincipal::from_to(from, to)?;
        transfer_from(self, caller, amount)
    }

    /// Transfers `value` amount to the `to` principal, applying American style fee. This means, that
    /// the recipient will receive `value - fee`, and the sender account will be reduced exactly by `value`.
    ///
    /// Note, that the `value` cannot be less than the `fee` amount. If the value given is too small,
    /// transaction will fail with `TxError::AmountTooSmall` error.
    #[cfg_attr(feature = "transfer", update(trait = true))]
    fn transferIncludeFee(&self, to: Principal, amount: Tokens128) -> TxReceipt {
        let caller = CheckedPrincipal::with_recipient(to)?;
        transfer_include_fee(self, caller, amount)
    }

    /// Takes a list of transfers, each of which is a pair of `to` and `value` fields, it returns a `TxReceipt` which contains
    /// a vec of transaction index or an error message. The list of transfers is processed in the order they are given. if the `fee`
    /// is set, the `fee` amount is applied to each transfer.
    /// The balance of the caller is reduced by sum of `value + fee` amount for each transfer. If the total sum of `value + fee` for all transfers,
    /// is less than the `balance` of the caller, the transaction will fail with `TxError::InsufficientBalance` error.
    #[cfg_attr(feature = "transfer", update(trait = true))]
    fn batchTransfer(&self, transfers: Vec<(Principal, Tokens128)>) -> Result<Vec<TxId>, TxError> {
        for (to, _) in transfers.clone() {
            let _ = CheckedPrincipal::with_recipient(to)?;
        }
        batch_transfer(self, transfers)
    }

    #[cfg_attr(feature = "mint_burn", update(trait = true))]
    fn mint(&self, to: Principal, amount: Tokens128) -> TxReceipt {
        if self.isTestToken() {
            let test_user = CheckedPrincipal::test_user(&self.state().borrow().stats)?;
            mint_test_token(&mut *self.state().borrow_mut(), test_user, to, amount)
        } else {
            let owner = CheckedPrincipal::owner(&self.state().borrow().stats)?;
            mint_as_owner(&mut *self.state().borrow_mut(), owner, to, amount)
        }
    }

    /// Burn `amount` of tokens from `from` principal.
    /// If `from` is None, then caller's tokens will be burned.
    /// If `from` is Some(_) but method called not by owner, `TxError::Unauthorized` will be returned.
    /// If owner calls this method and `from` is Some(who), then who's tokens will be burned.
    #[cfg_attr(feature = "mint_burn", update(trait = true))]
    fn burn(&self, from: Option<Principal>, amount: Tokens128) -> TxReceipt {
        match from {
            None => burn_own_tokens(&mut *self.state().borrow_mut(), amount),
            Some(from) if from == ic_canister::ic_kit::ic::caller() => {
                burn_own_tokens(&mut *self.state().borrow_mut(), amount)
            }
            Some(from) => {
                let caller = CheckedPrincipal::owner(&self.state().borrow().stats)?;
                burn_as_owner(&mut *self.state().borrow_mut(), caller, from, amount)
            }
        }
    }

    #[update(trait = true)]
    fn consume_notification<'a>(&'a self, transaction_id: TxId) -> AsyncReturn<TxReceipt> {
        let fut = async move { consume_notification(self, transaction_id).await };

        Box::pin(fut)
    }

    /********************** Auction ***********************/
    // These are re-exports of the `Auction` methods but with
    // camelCase names to follow is20 spec. As by default the
    // idl of the `Auction` is not exported as it is not merged
    // the `Auction` methods won't clash with these ones.

    #[update(trait = true)]
    fn disburseRewards(&self) -> Result<AuctionInfo, AuctionError> {
        self.disburse_rewards()
    }

    /// Starts the cycle auction.
    ///
    /// This method can be called only once in a [BiddingState.auction_period]. If the time elapsed
    /// since the last auction is less than the set period, [AuctionError::TooEarly] will be returned.
    ///
    /// The auction will distribute the accumulated fees in proportion to the user cycle bids, and
    /// then will update the fee ratio until the next auction.
    #[update(trait = true)]
    fn runAuction(&self) -> Result<AuctionInfo, AuctionError> {
        self.run_auction()
    }

    /// Bid cycles for the next cycle auction.
    ///
    /// This method must be called with the cycles provided in the call. The amount of cycles cannot be
    /// less than 1_000_000. The provided cycles are accepted by the canister, and the user bid is
    /// saved for the next auction.
    #[update(trait = true)]
    fn bidCycles(&self, bidder: Principal) -> Result<u64, AuctionError> {
        self.bid_cycles(bidder)
    }

    /// Current information about bids and auction.
    #[update(trait = true)]
    fn biddingInfo(&self) -> BiddingInfo {
        self.bidding_info()
    }

    /// Returns the information about a previously held auction.
    #[update(trait = true)]
    fn auctionInfo(&self, id: usize) -> Result<AuctionInfo, AuctionError> {
        self.auction_info(id)
    }

    /// Returns the minimum cycles set for the canister.
    ///
    /// This value affects the fee ratio set by the auctions. The more cycles available in the canister
    /// the less proportion of the fees will be transferred to the auction participants. If the amount
    /// of cycles in the canister drops below this value, all the fees will be used for cycle auction.
    #[update(trait = true)]
    fn getMinCycles(&self) -> u64 {
        self.get_min_cycles()
    }

    /// Sets the minimum cycles for the canister. For more information about this value, read [get_min_cycles].
    ///
    /// Only the owner is allowed to call this method.
    #[update(trait = true)]
    fn setMinCycles(&self, min_cycles: u64) -> Result<(), AuctionError> {
        self.set_min_cycles(min_cycles)
    }

    /// Sets the minimum time between two consecutive auctions, in seconds.
    ///
    /// Only the owner is allowed to call this method.
    #[update(trait = true)]
    fn setAuctionPeriod(&self, interval: Interval) -> Result<(), AuctionError> {
        self.set_auction_period(interval)
    }

    /// Update the controller of the auction.
    ///
    /// Only owner is allowed to call this method.
    #[update(trait = true)]
    fn setAuctionController(&self, controller: Principal) -> Result<(), AuctionError> {
        self.set_controller(controller)
    }

    #[update(trait = true)]
    fn approveAndNotify<'a>(
        &'a self,
        spender: Principal,
        amount: Tokens128,
    ) -> AsyncReturn<TxReceipt> {
        let caller = CheckedPrincipal::with_recipient(spender);
        let fut = async move { approve_and_notify(self, caller?, amount).await };
        Box::pin(fut)
    }

    #[update(trait = true)]
    fn notify<'a>(&'a self, transaction_id: TxId, to: Principal) -> AsyncReturn<TxReceipt> {
        let fut = async move { notify(self, transaction_id, to).await };

        Box::pin(fut)
    }

    /********************** Transactions ***********************/
    #[query(trait = true)]
    fn getTransaction(&self, id: TxId) -> TxRecord {
        self.state().borrow().ledger.get(id).unwrap_or_else(|| {
            ic_canister::ic_kit::ic::trap(&format!("Transaction {} does not exist", id))
        })
    }

    /// Returns a list of transactions in paginated form. The `who` is optional, if given, only transactions of the `who` are
    /// returned. `count` is the number of transactions to return, `transaction_id` is the transaction index which is used as
    /// the offset of the first transaction to return, any
    ///
    /// It returns `PaginatedResult` a struct, which contains `result` which is a list of transactions `Vec<TxRecord>` that meet the requirements of the query,
    /// and `next_id` which is the index of the next transaction to return.
    #[query(trait = true)]
    fn getTransactions(
        &self,
        who: Option<Principal>,
        count: usize,
        transaction_id: Option<TxId>,
    ) -> PaginatedResult {
        // We don't trap if the transaction count is greater than the MAX_TRANSACTION_QUERY_LEN, we take the MAX_TRANSACTION_QUERY_LEN instead.
        self.state().borrow().ledger.get_transactions(
            who,
            count.min(MAX_TRANSACTION_QUERY_LEN),
            transaction_id,
        )
    }

    /// Returns the total number of transactions related to the user `who`.
    #[query(trait = true)]
    fn getUserTransactionCount(&self, who: Principal) -> usize {
        self.state().borrow().ledger.get_len_user_history(who)
    }

    // Important: This function *must* be defined to be the
    // last one in the trait because it depends on the order
    // of expansion of update/query(trait = true) methods.
    fn get_idl() -> ic_canister::Idl {
        ic_canister::generate_idl!()
    }
}

generate_exports!(TokenCanisterAPI, TokenCanisterExports);

impl Auction for TokenCanisterExports {
    fn auction_state(&self) -> Rc<RefCell<AuctionState>> {
        AuctionState::get()
    }

    fn disburse_rewards(&self) -> Result<ic_auction::AuctionInfo, AuctionError> {
        is20_auction::disburse_rewards(self)
    }
}
