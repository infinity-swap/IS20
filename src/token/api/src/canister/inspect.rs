use crate::state::CanisterState;
use crate::types::TxId;
use candid::{Nat, Principal};
use ic_helpers::tokens::Tokens128;
use ic_storage::IcStorage;

static PUBLIC_METHODS: &[&str] = &[
    "allowance",
    "auctionInfo",
    "balanceOf",
    "biddingInfo",
    "decimals",
    "getAllowanceSize",
    "getHolders",
    "getMetadata",
    "getTokenInfo",
    "getTransaction",
    "getTransactions",
    "getUserApprovals",
    "getUserTransactionAmount",
    "getUserTransactions",
    "historySize",
    "logo",
    "name",
    "owner",
    "symbol",
    "totalSupply",
    "isTestToken",
];

static OWNER_METHODS: &[&str] = &[
    "mint",
    "setAuctionPeriod",
    "setFee",
    "setFeeTo",
    "setLogo",
    "setMinCycles",
    "setName",
    "setOwner",
    "toggleTest",
];

static TRANSACTION_METHODS: &[&str] = &[
    "approve",
    "approveAndNotify",
    "burn",
    "transfer",
    "transferIncludeFee",
];

/// Reason why the method may be accepted.
#[derive(Debug, Clone, Copy)]
pub enum AcceptReason {
    /// The call is a part of the IS20 API and can be performed.
    Valid,
    /// The method isn't a part of the IS20 API, and may require further validation.
    NotIS20Method,
}

/// This function checks if the canister should accept ingress message or not. We allow query
/// calls for anyone, but update calls have different checks to see, if it's reasonable to spend
/// canister cycles on accepting this call. Check the comments in this method for details on
/// the checks for different methods.
pub fn inspect_message(
    state: &CanisterState,
    method: &str,
    caller: Principal,
) -> Result<AcceptReason, &'static str> {
    match method {
        // These are query methods, so no checks are needed.
        #[cfg(feature = "mint_burn")]
        "mint" if state.stats.is_test_token => Ok(AcceptReason::Valid),
        m if PUBLIC_METHODS.contains(&m) => Ok(AcceptReason::Valid),
        // Owner
        m if OWNER_METHODS.contains(&m) && caller == state.stats.owner => Ok(AcceptReason::Valid),
        // Not owner
        m if OWNER_METHODS.contains(&m) => {
            Err("Owner method is called not by an owner. Rejecting.")
        }
        #[cfg(any(feature = "transfer", feature = "mint_burn"))]
        m if TRANSACTION_METHODS.contains(&m) => {
            // These methods requires that the caller have tokens.
            let state = CanisterState::get();
            let state = state.borrow();
            let balances = &state.balances;
            if !balances.0.contains_key(&caller) {
                return Err("Transaction method is not called by a stakeholder. Rejecting.");
            }

            // Anything but the `burn` method
            if caller == state.stats.owner || m != "burn" {
                return Ok(AcceptReason::Valid);
            }

            // It's the `burn` method and the caller isn't the owner.
            let from = ic_cdk::api::call::arg_data::<(Option<Principal>, Nat)>().0;
            if from.is_some() {
                return Err("Only the owner can burn other's tokens. Rejecting.");
            }

            Ok(AcceptReason::Valid)
        }
        #[cfg(feature = "transfer")]
        "transferFrom" => {
            // Check if the caller has allowance for this transfer.
            let allowances = &state.allowances;
            let (from, _, value) =
                ic_cdk::api::call::arg_data::<(Principal, Principal, Tokens128)>();
            if let Some(user_allowances) = allowances.get(&caller) {
                if let Some(allowance) = user_allowances.get(&from) {
                    if value <= *allowance {
                        Ok(AcceptReason::Valid)
                    } else {
                        Err("Allowance amount is less then the requested transfer amount. Rejecting.")
                    }
                } else {
                    Err("Caller is not allowed to transfer tokens for the requested principal. Rejecting.")
                }
            } else {
                Err("Caller is not allowed to transfer tokens for the requested principal. Rejecting.")
            }
        }
        "notify" => {
            // This method can only be called if the notification id is in the pending notifications
            // list.
            let notifications = &state.ledger.notifications;
            let (tx_id,) = ic_cdk::api::call::arg_data::<(TxId,)>();

            if notifications.contains_key(&tx_id) {
                Ok(AcceptReason::Valid)
            } else {
                Err("No pending notification with the given id. Rejecting.")
            }
        }
        "ConsumeNotification" => {
            // This method can only be called if the notification id is in the pending notifications
            // list and the caller is notified canister.
            let notifications = &state.ledger.notifications;
            let (tx_id,) = ic_cdk::api::call::arg_data::<(TxId,)>();

            match notifications.get(&tx_id) {
                Some(Some(x)) if *x != ic_canister::ic_kit::ic::caller() => {
                    return Err("Unauthorized")
                }
                Some(_) => {
                    if !state.ledger.notifications.contains_key(&tx_id) {
                        return Err("Already removed");
                    }
                }
                None => return Err("Transaction does not exist"),
            }

            Ok(AcceptReason::Valid)
        }
        _ => Ok(AcceptReason::NotIS20Method),
    }
}
