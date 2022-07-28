use candid::{CandidType, Deserialize, Principal};
use ic_helpers::tokens::Tokens128;

use crate::types::{PaginatedResult, PendingNotifications, TxId, TxRecord};

const MAX_HISTORY_LENGTH: usize = 1_000_000;
const HISTORY_REMOVAL_BATCH_SIZE: usize = 10_000;

#[derive(Debug, Default, CandidType, Deserialize)]
pub struct Ledger {
    history: Vec<TxRecord>,
    vec_offset: u64,
    pub notifications: PendingNotifications,
}

impl Ledger {
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> u64 {
        self.vec_offset + self.history.len() as u64
    }

    fn next_id(&self) -> TxId {
        self.vec_offset + self.history.len() as u64
    }

    pub fn get(&self, id: TxId) -> Option<TxRecord> {
        self.history.get(self.get_index(id)?).cloned()
    }

    pub fn get_transactions(
        &self,
        who: Option<Principal>,
        count: usize,
        transaction_id: Option<TxId>,
    ) -> PaginatedResult {
        let count = count as usize;
        let mut transactions = self
            .history
            .iter()
            .rev()
            .filter(|tx| who.map_or(true, |c| c == tx.from || c == tx.to || Some(c) == tx.caller))
            .filter(|tx| transaction_id.map_or(true, |id| id >= tx.index))
            .take(count + 1)
            .cloned()
            .collect::<Vec<_>>();

        let next_id = if transactions.len() == count + 1 {
            Some(transactions.remove(count).index)
        } else {
            None
        };

        PaginatedResult {
            result: transactions,
            next: next_id,
        }
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &TxRecord> {
        self.history.iter()
    }

    fn get_index(&self, id: TxId) -> Option<usize> {
        if id < self.vec_offset || id > usize::MAX as TxId {
            None
        } else {
            Some((id - self.vec_offset) as usize)
        }
    }

    pub fn get_len_user_history(&self, user: Principal) -> usize {
        self.history
            .iter()
            .filter(|tx| tx.to == user || tx.from == user || tx.caller == Some(user))
            .count()
    }

    pub fn transfer(
        &mut self,
        from: Principal,
        to: Principal,
        amount: Tokens128,
        fee: Tokens128,
    ) -> TxId {
        let id = self.next_id();
        self.push(TxRecord::transfer(id, from, to, amount, fee));

        id
    }

    pub fn batch_transfer(
        &mut self,
        from: Principal,
        transfers: Vec<(Principal, Tokens128)>,
        fee: Tokens128,
    ) -> Vec<TxId> {
        transfers
            .into_iter()
            .map(|(to, amount)| self.transfer(from, to, amount, fee))
            .collect()
    }

    pub fn transfer_from(
        &mut self,
        caller: Principal,
        from: Principal,
        to: Principal,
        amount: Tokens128,
        fee: Tokens128,
    ) -> TxId {
        let id = self.next_id();
        self.push(TxRecord::transfer_from(id, caller, from, to, amount, fee));

        id
    }

    pub fn approve(
        &mut self,
        from: Principal,
        to: Principal,
        amount: Tokens128,
        fee: Tokens128,
    ) -> TxId {
        let id = self.next_id();
        self.push(TxRecord::approve(id, from, to, amount, fee));

        id
    }

    pub fn mint(&mut self, from: Principal, to: Principal, amount: Tokens128) -> TxId {
        let id = self.len();
        self.push(TxRecord::mint(id, from, to, amount));

        id
    }

    pub fn burn(&mut self, caller: Principal, from: Principal, amount: Tokens128) -> TxId {
        let id = self.next_id();
        self.push(TxRecord::burn(id, caller, from, amount));

        id
    }

    pub fn record_auction(&mut self, to: Principal, amount: Tokens128) {
        let id = self.next_id();
        self.push(TxRecord::auction(id, to, amount))
    }

    fn push(&mut self, record: TxRecord) {
        self.history.push(record.clone());
        self.notifications.insert(record.index, None);

        if self.history.len() > MAX_HISTORY_LENGTH + HISTORY_REMOVAL_BATCH_SIZE {
            // We remove first `HISTORY_REMOVAL_BATCH_SIZE` from the history at one go, to prevent
            // often relocation of the history vec.
            // This removal code can later be changed to moving old history records into another
            // storage.
            for record in &self.history[..HISTORY_REMOVAL_BATCH_SIZE] {
                self.notifications.remove(&record.index);
            }
            self.history = self.history[HISTORY_REMOVAL_BATCH_SIZE..].into();
            self.vec_offset += HISTORY_REMOVAL_BATCH_SIZE as u64;
        }
    }
}
