use std::collections::HashMap;

use derive_more::IntoIterator;
use indexmap::IndexMap;
use starknet_api::core::{ClassHash, ContractAddress, Nonce};
use starknet_api::hash::StarkFelt;
use starknet_api::state::{StateDiff, StorageKey};

use super::state_api::TransactionalState;
use crate::execution::contract_class::ContractClass;
use crate::state::errors::StateError;
use crate::state::state_api::{State, StateReader, StateResult};
use crate::utils::subtract_mappings;

#[cfg(test)]
#[path = "cached_state_test.rs"]
mod test;

pub type ContractClassMapping = HashMap<ClassHash, ContractClass>;

pub struct StateWrapper<'a, T: State>(&'a mut T);
impl<'a, T: State> StateWrapper<'a, T> {
    pub fn new(state: &'a mut T) -> Self {
        Self(state)
    }
}
impl<'a, T: State> StateReader for StateWrapper<'a, T> {
    fn get_storage_at(
        &mut self,
        contract_address: ContractAddress,
        key: StorageKey,
    ) -> StateResult<StarkFelt> {
        self.0.get_storage_at(contract_address, key)
    }

    fn get_nonce_at(&mut self, contract_address: ContractAddress) -> StateResult<Nonce> {
        self.0.get_nonce_at(contract_address)
    }

    fn get_class_hash_at(&mut self, contract_address: ContractAddress) -> StateResult<ClassHash> {
        self.0.get_class_hash_at(contract_address)
    }

    fn get_contract_class(&mut self, class_hash: &ClassHash) -> StateResult<ContractClass> {
        self.0.get_contract_class(class_hash)
    }
}
impl<'a, T: State> State for StateWrapper<'a, T> {
    fn set_storage_at(
        &mut self,
        contract_address: ContractAddress,
        key: StorageKey,
        value: StarkFelt,
    ) {
        self.0.set_storage_at(contract_address, key, value)
    }
    fn increment_nonce(&mut self, contract_address: ContractAddress) -> StateResult<()> {
        self.0.increment_nonce(contract_address)
    }
    fn set_class_hash_at(
        &mut self,
        contract_address: ContractAddress,
        class_hash: ClassHash,
    ) -> StateResult<()> {
        self.0.set_class_hash_at(contract_address, class_hash)
    }
    fn set_contract_class(
        &mut self,
        class_hash: &ClassHash,
        contract_class: ContractClass,
    ) -> StateResult<()> {
        self.0.set_contract_class(class_hash, contract_class)
    }
    fn to_state_diff(&self) -> StateDiff {
        self.0.to_state_diff()
    }
}

/// Caches read and write requests.
///
/// Writer functionality is builtin, whereas Reader functionality is injected through
/// initialization.
#[derive(Debug, Default)]
pub struct CachedState<S: StateReader> {
    pub state: S,
    // Invariant: read/write access is managed by CachedState.
    cache: StateCache,
    class_hash_to_class: ContractClassMapping,
}

impl<S: StateReader> CachedState<S> {
    pub fn new(state: S) -> Self {
        Self { state, cache: StateCache::default(), class_hash_to_class: HashMap::default() }
    }

    pub fn merge(&mut self, child: CachedState<Self>) {
        self.cache.nonce_writes.extend(child.cache.nonce_writes);
        self.cache.class_hash_writes.extend(child.cache.class_hash_writes);
        self.cache.storage_writes.extend(child.cache.storage_writes);
        self.class_hash_to_class.extend(child.class_hash_to_class);
    }

    fn abort(self) {}
}

impl<S: StateReader> StateReader for CachedState<S> {
    fn get_storage_at(
        &mut self,
        contract_address: ContractAddress,
        key: StorageKey,
    ) -> StateResult<StarkFelt> {
        if self.cache.get_storage_at(contract_address, key).is_none() {
            let storage_value = self.state.get_storage_at(contract_address, key)?;
            self.cache.set_storage_initial_value(contract_address, key, storage_value);
        }

        let value = self.cache.get_storage_at(contract_address, key).unwrap_or_else(|| {
            panic!("Cannot retrieve '{contract_address:?}' and '{key:?}' from the cache.")
        });
        Ok(*value)
    }

    fn get_nonce_at(&mut self, contract_address: ContractAddress) -> StateResult<Nonce> {
        if self.cache.get_nonce_at(contract_address).is_none() {
            let nonce = self.state.get_nonce_at(contract_address)?;
            self.cache.set_nonce_initial_value(contract_address, nonce);
        }

        let nonce = self
            .cache
            .get_nonce_at(contract_address)
            .unwrap_or_else(|| panic!("Cannot retrieve '{contract_address:?}' from the cache."));
        Ok(*nonce)
    }

    fn get_class_hash_at(&mut self, contract_address: ContractAddress) -> StateResult<ClassHash> {
        if self.cache.get_class_hash_at(contract_address).is_none() {
            let class_hash = self.state.get_class_hash_at(contract_address)?;
            self.cache.set_class_hash_initial_value(contract_address, class_hash);
        }

        let class_hash = self
            .cache
            .get_class_hash_at(contract_address)
            .unwrap_or_else(|| panic!("Cannot retrieve '{contract_address:?}' from the cache."));
        Ok(*class_hash)
    }

    fn get_contract_class(&mut self, class_hash: &ClassHash) -> StateResult<ContractClass> {
        if !self.class_hash_to_class.contains_key(class_hash) {
            let contract_class = self.state.get_contract_class(class_hash)?;
            self.class_hash_to_class.insert(*class_hash, contract_class);
        }

        let contract_class = self
            .class_hash_to_class
            .get(class_hash)
            .expect("The class hash must appear in the cache.");
        Ok(contract_class.clone())
    }
}

impl<S: StateReader> State for CachedState<S> {
    fn set_storage_at(
        &mut self,
        contract_address: ContractAddress,
        key: StorageKey,
        value: StarkFelt,
    ) {
        self.cache.set_storage_value(contract_address, key, value);
    }

    fn increment_nonce(&mut self, contract_address: ContractAddress) -> StateResult<()> {
        let current_nonce = self.get_nonce_at(contract_address)?;
        let current_nonce_as_u64 = usize::try_from(current_nonce.0)? as u64;
        let next_nonce_val = 1_u64 + current_nonce_as_u64;
        let next_nonce = Nonce(StarkFelt::from(next_nonce_val));
        self.cache.set_nonce_value(contract_address, next_nonce);

        Ok(())
    }

    fn set_class_hash_at(
        &mut self,
        contract_address: ContractAddress,
        class_hash: ClassHash,
    ) -> StateResult<()> {
        if contract_address == ContractAddress::default() {
            return Err(StateError::OutOfRangeContractAddress);
        }

        let current_class_hash = self.get_class_hash_at(contract_address)?;
        if current_class_hash != ClassHash::default() {
            return Err(StateError::UnavailableContractAddress(contract_address));
        }

        self.cache.set_class_hash_write(contract_address, class_hash);
        Ok(())
    }

    fn set_contract_class(
        &mut self,
        class_hash: &ClassHash,
        contract_class: ContractClass,
    ) -> StateResult<()> {
        self.class_hash_to_class.insert(*class_hash, contract_class);
        Ok(())
    }

    fn to_state_diff(&self) -> StateDiff {
        type StorageDiff = IndexMap<ContractAddress, IndexMap<StorageKey, StarkFelt>>;
        let state_cache_diff = self.cache.get_state_diff();

        StateDiff {
            deployed_contracts: IndexMap::from_iter(state_cache_diff.class_hash_writes),
            storage_diffs: StorageDiff::from(StorageView(state_cache_diff.storage_writes)),
            declared_classes: IndexMap::new(),
            nonces: IndexMap::from_iter(state_cache_diff.nonce_writes),
        }
    }
}

impl<S: State> TransactionalState<S> for CachedState<S> {
    fn commit(mut self) -> StateResult<()> {
        let state_diff = self.cache.get_state_diff();

        // for (address, nonce) in state_diff.nonce_writes {
        //     let initial_nonce = self.state.get_nonce_at(address);

        //     for _ in initial_nonce..=nonce {
        //         self.state.increment_nonce(address);
        //     }
        // }

        for (address, class_hash) in state_diff.class_hash_writes {
            self.state.set_class_hash_at(address, class_hash)?;
        }

        for ((address, key), value) in state_diff.storage_writes {
            self.state.set_storage_at(address, key, value);
        }

        Ok(())
    }

    fn abort(self) {}
}

pub type ContractStorageKey = (ContractAddress, StorageKey);

#[derive(IntoIterator, Debug, Default)]
pub struct StorageView(pub HashMap<ContractStorageKey, StarkFelt>);

/// Converts a `CachedState`'s storage mapping into a `StateDiff`'s storage mapping.
impl From<StorageView> for IndexMap<ContractAddress, IndexMap<StorageKey, StarkFelt>> {
    fn from(storage_view: StorageView) -> Self {
        let mut storage_updates = Self::new();
        for ((address, key), value) in storage_view.into_iter() {
            storage_updates
                .entry(address)
                .and_modify(|map| {
                    map.insert(key, value);
                })
                .or_insert_with(|| IndexMap::from([(key, value)]));
        }

        storage_updates
    }
}

/// Caches read and write requests.

// Invariant: keys cannot be deleted from fields (only used internally by the cached state).
#[derive(Debug, Default, PartialEq)]
struct StateCache {
    // Reader's cached information; initial values, read before any write operation (per cell).
    nonce_initial_values: HashMap<ContractAddress, Nonce>,
    class_hash_initial_values: HashMap<ContractAddress, ClassHash>,
    storage_initial_values: HashMap<ContractStorageKey, StarkFelt>,

    // Writer's cached information.
    nonce_writes: HashMap<ContractAddress, Nonce>,
    class_hash_writes: HashMap<ContractAddress, ClassHash>,
    storage_writes: HashMap<ContractStorageKey, StarkFelt>,
}

impl StateCache {
    fn get_storage_at(
        &self,
        contract_address: ContractAddress,
        key: StorageKey,
    ) -> Option<&StarkFelt> {
        let contract_storage_key = (contract_address, key);
        self.storage_writes
            .get(&contract_storage_key)
            .or_else(|| self.storage_initial_values.get(&contract_storage_key))
    }

    fn get_nonce_at(&self, contract_address: ContractAddress) -> Option<&Nonce> {
        self.nonce_writes
            .get(&contract_address)
            .or_else(|| self.nonce_initial_values.get(&contract_address))
    }

    pub fn set_storage_initial_value(
        &mut self,
        contract_address: ContractAddress,
        key: StorageKey,
        value: StarkFelt,
    ) {
        let contract_storage_key = (contract_address, key);
        self.storage_initial_values.insert(contract_storage_key, value);
    }

    fn set_storage_value(
        &mut self,
        contract_address: ContractAddress,
        key: StorageKey,
        value: StarkFelt,
    ) {
        let contract_storage_key = (contract_address, key);
        self.storage_writes.insert(contract_storage_key, value);
    }

    fn set_nonce_initial_value(&mut self, contract_address: ContractAddress, nonce: Nonce) {
        self.nonce_initial_values.insert(contract_address, nonce);
    }

    fn set_nonce_value(&mut self, contract_address: ContractAddress, nonce: Nonce) {
        self.nonce_writes.insert(contract_address, nonce);
    }

    fn get_class_hash_at(&self, contract_address: ContractAddress) -> Option<&ClassHash> {
        self.class_hash_writes
            .get(&contract_address)
            .or_else(|| self.class_hash_initial_values.get(&contract_address))
    }

    fn set_class_hash_initial_value(
        &mut self,
        contract_address: ContractAddress,
        class_hash: ClassHash,
    ) {
        self.class_hash_initial_values.insert(contract_address, class_hash);
    }

    fn set_class_hash_write(&mut self, contract_address: ContractAddress, class_hash: ClassHash) {
        self.class_hash_writes.insert(contract_address, class_hash);
    }

    fn get_state_diff(&self) -> StateCache {
        let deployed_contracts =
            subtract_mappings(&self.class_hash_writes, &self.class_hash_initial_values);
        let storage_diffs = subtract_mappings(&self.storage_writes, &self.storage_initial_values);
        let nonce_diffs = subtract_mappings(&self.nonce_writes, &self.nonce_initial_values);

        StateCache {
            nonce_initial_values: HashMap::default(),
            class_hash_initial_values: HashMap::default(),
            storage_initial_values: HashMap::default(),
            nonce_writes: nonce_diffs,
            class_hash_writes: deployed_contracts,
            storage_writes: storage_diffs,
        }
    }
}
