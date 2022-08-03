use candid::Principal;
use ic_auction::{api::Auction, error::AuctionError, AuctionInfo, AuctionState};
use ic_canister::{init, post_upgrade, pre_upgrade, Canister, PreUpdate};

#[cfg(not(feature = "no_api"))]
use ic_cdk_macros::inspect_message;

use ic_canister::query;
use ic_helpers::{
    candid_header::{candid_header, CandidHeader},
    metrics::Interval,
};
use ic_storage::IcStorage;
use std::{cell::RefCell, rc::Rc};
use token_api::{
    canister::{TokenCanisterAPI, DEFAULT_AUCTION_PERIOD_SECONDS},
    state::{CanisterState, StableState},
    types::Metadata,
};

#[derive(Debug, Clone, Canister)]
#[canister_no_upgrade_methods]
pub struct TokenCanister {
    #[id]
    principal: Principal,
}

impl TokenCanister {
    #[init]
    pub fn init(&self, metadata: Metadata) {
        self.state()
            .borrow_mut()
            .balances
            .0
            .insert(metadata.owner, metadata.total_supply);

        self.state().borrow_mut().ledger.mint(
            metadata.owner,
            metadata.owner,
            metadata.total_supply,
        );

        self.state().borrow_mut().stats = metadata.into();

        let auction_state = self.auction_state();
        auction_state.replace(AuctionState::new(
            Interval::Period {
                seconds: DEFAULT_AUCTION_PERIOD_SECONDS,
            },
            ic_canister::ic_kit::ic::caller(),
        ));
    }

    #[pre_upgrade]
    fn pre_upgrade(&self) {
        let token_state = Rc::<RefCell<CanisterState>>::try_unwrap(self.state())
            .expect("Someone has the token factory state borrowed.")
            .into_inner();

        let auction_state = Rc::<RefCell<AuctionState>>::try_unwrap(self.auction_state())
            .expect("Someone has the base factory state borrowed. This is a program bug because state lock was bypassed.")
            .into_inner();

        ic_storage::stable::write(&StableState {
            token_state,
            auction_state,
        })
        .expect("failed to serialize state to the stable storage");
    }

    #[post_upgrade]
    fn post_upgrade(&self) {
        let stable_state = ic_storage::stable::read::<StableState>()
            .expect("failed to read stable state from the stable storage");

        let StableState {
            token_state,
            auction_state,
        } = stable_state;

        self.state().replace(token_state);
        self.auction_state().replace(auction_state);
    }

    #[query]
    pub fn state_check(&self) -> CandidHeader {
        candid_header::<CanisterState>()
    }
}

#[cfg(not(feature = "no_api"))]
#[inspect_message]
fn inspect_message() {
    use ic_storage::IcStorage;
    use token_api::canister::AcceptReason;

    let method = ic_cdk::api::call::method_name();

    let state = CanisterState::get();
    let state = state.borrow();
    let caller = ic_cdk::api::caller();

    let accept_reason = match TokenCanister::inspect_message(&state, &method, caller) {
        Ok(accept_reason) => accept_reason,
        Err(msg) => ic_cdk::trap(msg),
    };

    match accept_reason {
        AcceptReason::Valid => ic_cdk::api::call::accept_message(),
        AcceptReason::NotIS20Method => ic_cdk::trap("Unknown method"),
    }
}

impl PreUpdate for TokenCanister {
    fn pre_update(&self, method_name: &str, method_type: ic_canister::MethodType) {
        token_api::canister::pre_update(self, method_name, method_type);
    }
}

impl TokenCanisterAPI for TokenCanister {
    // Overwrite default implementation of `TokenCanisterAPI::state` getter
    // to use `impl` crate-local storage instead of what is defined in `api` crate.
    fn state(&self) -> Rc<RefCell<CanisterState>> {
        CanisterState::get()
    }
}

impl Auction for TokenCanister {
    // Overwrite default implementation of `Auction::auction_state` getter
    // to use `impl` crate-local storage instead of what is defined in `ic-auction` crate.
    fn auction_state(&self) -> Rc<RefCell<AuctionState>> {
        AuctionState::get()
    }

    fn disburse_rewards(&self) -> Result<AuctionInfo, AuctionError> {
        token_api::canister::is20_auction::disburse_rewards(self)
    }
}
