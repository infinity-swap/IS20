use candid::{CandidType, Deserialize, Principal};
use ic_helpers::tokens::Tokens128;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::Formatter;

mod tx_record;
pub use tx_record::*;

pub type Timestamp = u64;

#[allow(non_snake_case)]
#[derive(Deserialize, CandidType, Clone, Debug)]
pub struct Metadata {
    pub logo: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub totalSupply: Tokens128,
    pub owner: Principal,
    pub fee: Tokens128,
    pub feeTo: Principal,
    pub isTestToken: Option<bool>,
}

#[derive(Deserialize, CandidType, Clone, Debug)]
pub struct StatsData {
    pub logo: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: Tokens128,
    pub owner: Principal,
    pub fee: Tokens128,
    pub fee_to: Principal,
    pub deploy_time: u64,
    pub min_cycles: u64,
    pub is_test_token: bool,
}

impl StatsData {
    pub fn fee_info(&self) -> (Tokens128, Principal) {
        (self.fee, self.fee_to)
    }
}

// 10T cycles is an equivalent of approximately $10. This should be enough to last the canister
// for the default auction cycle, which is 1 day.
pub const DEFAULT_MIN_CYCLES: u64 = 10_000_000_000_000;

impl From<Metadata> for StatsData {
    fn from(md: Metadata) -> Self {
        Self {
            logo: md.logo,
            name: md.name,
            symbol: md.symbol,
            decimals: md.decimals,
            total_supply: md.totalSupply,
            owner: md.owner,
            fee: md.fee,
            fee_to: md.feeTo,
            deploy_time: ic_canister::ic_kit::ic::time(),
            min_cycles: DEFAULT_MIN_CYCLES,
            is_test_token: md.isTestToken.unwrap_or(false),
        }
    }
}

#[allow(non_snake_case)]
#[derive(Deserialize, CandidType, Clone, Debug)]
pub struct TokenInfo {
    pub metadata: Metadata,
    pub feeTo: Principal,
    pub historySize: u64,
    pub deployTime: Timestamp,
    pub holderNumber: usize,
    pub cycles: u64,
}

impl Default for StatsData {
    fn default() -> Self {
        StatsData {
            logo: "".to_string(),
            name: "".to_string(),
            symbol: "".to_string(),
            decimals: 0u8,
            total_supply: Tokens128::from(0u128),
            owner: Principal::anonymous(),
            fee: Tokens128::from(0u128),
            fee_to: Principal::anonymous(),
            deploy_time: 0,
            min_cycles: 0,
            is_test_token: false,
        }
    }
}

pub type Allowances = HashMap<Principal, HashMap<Principal, Tokens128>>;

// TODO: a wrapper over `ic_helpers::TxError`, this is a most likely
// place to make tests fail in amm.
#[derive(CandidType, Debug, PartialEq, Deserialize)]
pub enum TxError {
    InsufficientBalance,
    InsufficientAllowance,
    NoAllowance,
    Unauthorized,
    AmountTooSmall,
    FeeExceededLimit,
    ApproveSucceededButNotifyFailed { tx_error: Box<TxError> },
    NotificationFailed { transaction_id: u64 },
    AlreadyActioned,
    NotificationDoesNotExist,
    TransactionDoesNotExist,
    BadFee { expected_fee: Tokens128 },
    InsufficientFunds { balance: Tokens128 },
    TxTooOld { allowed_window_nanos: u64 },
    TxCreatedInFuture,
    TxDuplicate { duplicate_of: u64 },
    SelfTransfer,
    AmountOverflow,
}

impl std::fmt::Display for TxError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TxError::InsufficientBalance => write!(f, "Insufficient balance"),
            TxError::InsufficientAllowance => write!(f, "Insufficient allowance"),
            TxError::NoAllowance => write!(f, "No allowance"),
            TxError::Unauthorized => write!(f, "Unauthorized"),
            TxError::AmountTooSmall => write!(f, "Amount too small"),
            TxError::FeeExceededLimit => write!(f, "Fee exceeded limit"),
            TxError::ApproveSucceededButNotifyFailed { tx_error } => {
                write!(f, "Approve succeeded but notify failed: {}", tx_error)
            }
            TxError::NotificationFailed { transaction_id } => {
                write!(f, "Notification failed for transaction {}", transaction_id)
            }
            TxError::AlreadyActioned => write!(f, "Already actioned"),
            TxError::NotificationDoesNotExist => write!(f, "Notification does not exist"),
            TxError::TransactionDoesNotExist => write!(f, "Transaction does not exist"),
            TxError::BadFee { expected_fee } => write!(f, "Bad fee: {}", expected_fee),
            TxError::InsufficientFunds { balance } => write!(f, "Insufficient funds: {}", balance),
            TxError::TxTooOld {
                allowed_window_nanos,
            } => write!(f, "Transaction is too old: {}", allowed_window_nanos),
            TxError::TxCreatedInFuture => write!(f, "Transaction created in future"),
            TxError::TxDuplicate { duplicate_of } => {
                write!(f, "Transaction is a duplicate of {}", duplicate_of)
            }
            TxError::SelfTransfer => write!(f, "Self transfer"),
            TxError::AmountOverflow => write!(f, "Amount overflow"),
        }
    }
}

impl Error for TxError {}

pub type TxReceipt = Result<u64, TxError>;

// Notification receiver not set if None
pub type PendingNotifications = HashMap<u64, Option<Principal>>;

#[derive(CandidType, Debug, Clone, Copy, Deserialize, PartialEq)]
pub enum TransactionStatus {
    Succeeded,
    Failed,
}

#[derive(CandidType, Debug, Clone, Copy, Deserialize, PartialEq)]
pub enum Operation {
    Approve,
    Mint,
    Transfer,
    TransferFrom,
    Burn,
    Auction,
}

/// `PaginatedResult` is returned by paginated queries i.e `getTransactions`.
#[derive(Debug, Clone, CandidType, Deserialize)]
pub struct PaginatedResult {
    /// The result is the transactions which is the `count` transactions starting from `next` if it exists.
    pub result: Vec<TxRecord>,

    /// This is  the next `id` of the transaction. The `next` is used as offset for the next query if it exits.
    pub next: Option<TxId>,
}

pub type TxId = u64;
pub type Cycles = u64;
