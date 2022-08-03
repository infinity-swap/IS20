use std::cell::RefCell;
use std::rc::Rc;

use ic_canister::generate_exports;
use ic_canister::Canister;
use ic_canister::MethodType;
use ic_canister::{query, update};
use ic_cdk::export::candid::Principal;
use ic_helpers::ledger::AccountIdentifier;
use ic_helpers::tokens::Tokens128;
use ic_storage::IcStorage;

pub use inspect::AcceptReason;

use crate::account::CheckedAccount;
use crate::account::{Account, Subaccount};
use crate::canister::erc20_transactions::{
    burn_as_owner, burn_own_tokens, claim, icrc1_transfer, mint_as_owner, mint_test_token,
    mint_to_accountid,
};
use crate::canister::is20_auction::{
    auction_info, bid_cycles, bidding_info, run_auction, AuctionError, BiddingInfo,
};
use crate::canister::is20_transactions::icrc1_transfer_include_fee;
use crate::principal::{CheckedPrincipal, Owner};
use crate::state::CanisterState;
use crate::types::BalanceArgs;
use crate::types::BatchTransferArgs;
use crate::types::StandardRecord;
use crate::types::TransferArgs;
use crate::types::Value;
use crate::types::{
    AuctionInfo, PaginatedResult, StatsData, Timestamp, TokenInfo, TxError, TxId, TxReceipt,
    TxRecord,
};

use self::is20_transactions::batch_transfer;

mod inspect;

pub mod erc20_transactions;

pub mod is20_auction;
pub mod is20_transactions;

pub(crate) const MAX_TRANSACTION_QUERY_LEN: usize = 1000;
// 1 day in nanoseconds.
pub const DEFAULT_AUCTION_PERIOD: Timestamp = 24 * 60 * 60 * 1_000_000;

pub fn pre_update(canister: &impl TokenCanisterAPI, method_name: &str, _method_type: MethodType) {
    if method_name != "runAuction" {
        if let Err(auction_error) = canister.runAuction() {
            ic_cdk::println!("Auction error: {auction_error:#?}");
        }
    }
}

pub enum CanisterUpdate {
    Name(String),
    Logo(String),
    Fee(Tokens128),
    FeeTo(Principal),
    Owner(Principal),
    MinCycles(u64),
    AuctionPeriod(u64),
}

#[allow(non_snake_case)]
pub trait TokenCanisterAPI: Canister + Sized {
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
    fn icrc1_name(&self) -> String {
        self.state().borrow().stats.name.clone()
    }

    #[query(trait = true)]
    fn icrc1_symbol(&self) -> String {
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
    fn icrc1_total_supply(&self) -> Tokens128 {
        self.state().borrow().balances.total_supply()
    }

    #[query(trait = true)]
    fn owner(&self) -> Principal {
        self.state().borrow().stats.owner
    }
    /// Returns the default transfer fee.
    #[query(trait = true)]
    fn icrc1_fee(&self) -> Tokens128 {
        self.state().borrow().stats.fee
    }
    #[query(trait = true)]
    fn icrc1_metadata(&self) -> Vec<(String, Value)> {
        self.state().borrow().icrc1_metadata()
    }

    #[query(trait = true)]
    fn icrc1_supported_standards(&self) -> Vec<StandardRecord> {
        self.state().borrow().stats.supported_standards()
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

    /// This method retreieves holders of `Account` and their amounts.
    #[query(trait = true)]
    fn getHolders(&self, start: usize, limit: usize) -> Vec<(Account, Tokens128)> {
        self.state().borrow().balances.get_holders(start, limit)
    }

    #[query(trait = true)]
    fn icrc1_balance_of(&self, balance_arg: BalanceArgs) -> Tokens128 {
        let account = Account::new(balance_arg.of, balance_arg.subaccount);
        self.state().borrow().balances.balance_of(account)
    }

    /// This method returns the pending `claim` for the `Account`.
    #[query(trait = true)]
    fn getClaim(&self, subaccount: Option<Subaccount>) -> Result<Tokens128, TxError> {
        self.state().borrow().get_claim(subaccount)
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
            AuctionPeriod(period_sec) => {
                self.state().borrow_mut().bidding_state.auction_period = period_sec * 1_000_000
            }
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

    /********************** TRANSFERS ***********************/
    #[cfg_attr(feature = "transfer", update(trait = true))]
    fn icrc1_transfer(&self, transfer: TransferArgs) -> TxReceipt {
        let recipient = Account::new(transfer.to.principal, transfer.to.subaccount);

        let account = CheckedAccount::with_recipient(recipient, transfer.from_subaccount)?;

        icrc1_transfer(self, account, transfer)
    }

    /// Transfers `value` amount to the `to` principal, applying American style fee. This means, that
    /// the recipient will receive `value - fee`, and the sender account will be reduced exactly by `value`.
    ///
    /// Note, that the `value` cannot be less than the `fee` amount. If the value given is too small,
    /// transaction will fail with `TxError::AmountTooSmall` error.
    #[cfg_attr(feature = "transfer", update(trait = true))]
    fn icrc1_transferIncludeFee(
        &self,
        from_subaccount: Option<Subaccount>,
        to: Principal,
        to_subaccount: Option<Subaccount>,
        amount: Tokens128,
        memo: Option<u64>,
        created_at_time: Option<Timestamp>,
    ) -> TxReceipt {
        let recipient = Account::new(to, to_subaccount);

        let account = CheckedAccount::with_recipient(recipient, from_subaccount)?;

        icrc1_transfer_include_fee(
            self,
            account.inner(),
            account.recipient(),
            amount,
            memo,
            created_at_time,
        )
    }

    /// Takes a list of transfers, each of which is a pair of `to` and `value` fields, it returns a `TxReceipt` which contains
    /// a vec of transaction index or an error message. The list of transfers is processed in the order they are given. if the `fee`
    /// is set, the `fee` amount is applied to each transfer.
    /// The balance of the caller is reduced by sum of `value + fee` amount for each transfer. If the total sum of `value + fee` for all transfers,
    /// is less than the `balance` of the caller, the transaction will fail with `TxError::InsufficientBalance` error.
    #[cfg_attr(feature = "transfer", update(trait = true))]
    fn batchTransfer(
        &self,
        from_subaccount: Option<Subaccount>,
        transfers: Vec<BatchTransferArgs>,
    ) -> Result<Vec<TxId>, TxError> {
        for x in &transfers {
            let recipient = Account::new(x.receiver.principal, x.receiver.subaccount);
            CheckedAccount::with_recipient(recipient, from_subaccount)?;
        }
        batch_transfer(self, from_subaccount, transfers)
    }

    #[cfg_attr(feature = "mint_burn", update(trait = true))]
    fn icrc1_mint(
        &self,
        to: Principal,
        to_subaccount: Option<Subaccount>,
        amount: Tokens128,
    ) -> TxReceipt {
        if self.isTestToken() {
            let test_user = CheckedPrincipal::test_user(&self.state().borrow().stats)?;
            mint_test_token(
                &mut *self.state().borrow_mut(),
                test_user,
                to,
                to_subaccount,
                amount,
            )
        } else {
            let owner = CheckedPrincipal::owner(&self.state().borrow().stats)?;
            mint_as_owner(
                &mut *self.state().borrow_mut(),
                owner,
                to,
                to_subaccount,
                amount,
            )
        }
    }

    /// Burn `amount` of tokens from `from` principal.
    /// If `from` is None, then caller's tokens will be burned.
    /// If `from` is Some(_) but method called not by owner, `TxError::Unauthorized` will be returned.
    /// If owner calls this method and `from` is Some(who), then who's tokens will be burned.
    #[cfg_attr(feature = "mint_burn", update(trait = true))]
    fn icrc1_burn(
        &self,
        from: Option<Principal>,
        from_subaccount: Option<Subaccount>,
        amount: Tokens128,
    ) -> TxReceipt {
        match from {
            None => burn_own_tokens(&mut *self.state().borrow_mut(), from_subaccount, amount),
            Some(from) if from == ic_canister::ic_kit::ic::caller() => {
                burn_own_tokens(&mut *self.state().borrow_mut(), from_subaccount, amount)
            }
            Some(from) => {
                let caller = CheckedPrincipal::owner(&self.state().borrow().stats)?;
                burn_as_owner(
                    &mut *self.state().borrow_mut(),
                    caller,
                    from,
                    from_subaccount,
                    amount,
                )
            }
        }
    }

    /// This function mints to `AccountIdentifier`, this is different from `Account`, this adds support for minting to `AccountIdentifier`
    ///
    #[cfg_attr(feature = "mint_burn", update(trait = true))]
    fn mintToAccountId(&self, to: AccountIdentifier, amount: Tokens128) -> Result<(), TxError> {
        let _ = CheckedPrincipal::owner(&self.state().borrow().stats)?;
        mint_to_accountid(&mut *self.state().borrow_mut(), to, amount)
    }

    /// When we mint to `AccountIdentifier`, Only the user who has been minted can claim the amount that has been minted to `AccountIdentifier`, if another user claims the `claim`, it fails with Error `ClaimNotAllowed`.
    #[update(trait = true)]
    fn claim(&self, account: AccountIdentifier, subaccount: Option<Subaccount>) -> TxReceipt {
        claim(&mut *self.state().borrow_mut(), account, subaccount)
    }

    /********************** AUCTION ***********************/
    /// Bid cycles for the next cycle auction.
    ///
    /// This method must be called with the cycles provided in the call. The amount of cycles cannot be
    /// less than 1_000_000. The provided cycles are accepted by the canister, and the user bid is
    /// saved for the next auction.
    #[update(trait = true)]
    fn bidCycles(&self, bidder: Principal) -> Result<u64, AuctionError> {
        bid_cycles(self, bidder)
    }

    /// Current information about bids and auction.
    #[update(trait = true)]
    fn biddingInfo(&self) -> BiddingInfo {
        bidding_info(self)
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
        run_auction(self)
    }

    /// Returns the information about a previously held auction.
    #[update(trait = true)]
    fn auctionInfo(&self, id: usize) -> Result<AuctionInfo, AuctionError> {
        auction_info(self, id)
    }

    /// Returns the minimum cycles set for the canister.
    ///
    /// This value affects the fee ratio set by the auctions. The more cycles available in the canister
    /// the less proportion of the fees will be transferred to the auction participants. If the amount
    /// of cycles in the canister drops below this value, all the fees will be used for cycle auction.
    #[update(trait = true)]
    fn getMinCycles(&self) -> u64 {
        self.state().borrow().stats.min_cycles
    }

    /// Sets the minimum cycles for the canister. For more information about this value, read [get_min_cycles].
    ///
    /// Only the owner is allowed to call this method.
    #[update(trait = true)]
    fn setMinCycles(&self, min_cycles: u64) -> Result<(), TxError> {
        let caller = CheckedPrincipal::owner(&self.state().borrow_mut().stats)?;
        self.update_stats(caller, CanisterUpdate::MinCycles(min_cycles));
        Ok(())
    }

    /// Sets the minimum time between two consecutive auctions, in seconds.
    ///
    /// Only the owner is allowed to call this method.
    #[update(trait = true)]
    fn setAuctionPeriod(&self, period_sec: u64) -> Result<(), TxError> {
        let caller = CheckedPrincipal::owner(&self.state().borrow_mut().stats)?;
        // IC timestamp is in nanoseconds, thus multiplying
        self.update_stats(caller, CanisterUpdate::AuctionPeriod(period_sec));
        Ok(())
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
