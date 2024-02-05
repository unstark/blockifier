use std::collections::HashMap;

use cairo_vm::vm::runners::cairo_runner::ExecutionResources as VmExecutionResources;
use serde::Deserialize;

use crate::execution::deprecated_syscalls::hint_processor::SyscallCounter;
use crate::execution::deprecated_syscalls::DeprecatedSyscallSelector;
use crate::execution::errors::PostExecutionError;
use crate::fee::os_resources::OS_RESOURCES;
use crate::transaction::errors::TransactionExecutionError;
use crate::transaction::transaction_types::TransactionType;

#[cfg(test)]
#[path = "os_usage_test.rs"]
pub mod test;

#[derive(Debug, Deserialize)]
pub struct ResourcesParams {
    pub constant: VmExecutionResources,
    pub calldata_factor: VmExecutionResources,
}

#[derive(Debug, Deserialize)]
pub struct OsResources {
    // Mapping from every syscall to its execution resources in the OS (e.g., amount of Cairo
    // steps).
    execute_syscalls: HashMap<DeprecatedSyscallSelector, VmExecutionResources>,
    // Mapping from every transaction to its extra execution resources in the OS,
    // i.e., resources that don't count during the execution itself.
    execute_txs_inner: HashMap<TransactionType, ResourcesParams>,
}

impl OsResources {
    fn resources_params_for_tx_type(&self, tx_type: &TransactionType) -> &ResourcesParams {
        self.execute_txs_inner
            .get(tx_type)
            .unwrap_or_else(|| panic!("should contain transaction type '{tx_type:?}'."))
    }

    pub fn resources_for_tx_type(
        &self,
        tx_type: &TransactionType,
        calldata_length: usize,
    ) -> VmExecutionResources {
        let resources_vector = self.resources_params_for_tx_type(tx_type);
        &resources_vector.constant + &(&(resources_vector.calldata_factor) * calldata_length)
    }
}

// Calculates the additional resources needed for the OS to run the given transaction;
// i.e., the resources of the Starknet OS function `execute_transactions_inner`.
// Also adds the resources needed for the fee transfer execution, performed in the end·
// of every transaction.
pub fn get_additional_os_tx_resources(
    tx_type: TransactionType,
    calldata_length: usize,
) -> Result<VmExecutionResources, TransactionExecutionError> {
    Ok(OS_RESOURCES.resources_for_tx_type(&tx_type, calldata_length))
}

/// Calculates the additional resources needed for the OS to run the given syscalls;
/// i.e., the resources of the Starknet OS function `execute_syscalls`.
pub fn get_additional_os_syscall_resources(
    syscall_counter: &SyscallCounter,
) -> Result<VmExecutionResources, TransactionExecutionError> {
    let mut os_additional_vm_resources = VmExecutionResources::default();
    for (syscall_selector, count) in syscall_counter {
        let syscall_resources =
            OS_RESOURCES.execute_syscalls.get(syscall_selector).unwrap_or_else(|| {
                panic!("OS resources of syscall '{syscall_selector:?}' are unknown.")
            });
        os_additional_vm_resources += &(syscall_resources * *count);
    }

    Ok(os_additional_vm_resources)
}

/// Calculates the additional resources needed for the OS to run the given syscalls;
/// i.e., the resources of the Starknet OS function `execute_syscalls`.
pub fn get_additional_os_syscall_resources_copy(
    syscall_counter: &SyscallCounter,
) -> Result<VmExecutionResources, PostExecutionError> {
    let mut os_additional_syscall_resources = VmExecutionResources::default();
    for (syscall_selector, count) in syscall_counter {
        let syscall_resources =
            OS_RESOURCES.execute_syscalls.get(syscall_selector).unwrap_or_else(|| {
                panic!("OS resources of syscall '{syscall_selector:?}' are unknown.")
            });
        os_additional_syscall_resources += &(syscall_resources * *count);
    }

    Ok(os_additional_syscall_resources)
}
