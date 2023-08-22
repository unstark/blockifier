use std::convert::TryFrom;

use blockifier::transaction::errors::TransactionExecutionError;
use cairo_vm::serde::deserialize_program::BuiltinName;
use num_bigint::BigUint;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use starknet_api::core::{ChainId, ClassHash, CompiledClassHash, ContractAddress};
use starknet_api::deprecated_contract_class::EntryPointType;
use starknet_api::hash::StarkFelt;
use starknet_api::transaction::EthAddress;

use crate::errors::{NativeBlockifierError, NativeBlockifierInputError, NativeBlockifierResult};

#[derive(Eq, FromPyObject, Hash, PartialEq, Clone, Copy)]
pub struct PyFelt(#[pyo3(from_py_with = "int_to_stark_felt")] pub StarkFelt);

impl IntoPy<PyObject> for PyFelt {
    fn into_py(self, py: Python<'_>) -> PyObject {
        BigUint::from_bytes_be(self.0.bytes()).into_py(py)
    }
}

impl From<u64> for PyFelt {
    fn from(value: u64) -> Self {
        Self(StarkFelt::from(value))
    }
}

impl From<u8> for PyFelt {
    fn from(value: u8) -> Self {
        Self(StarkFelt::from(value))
    }
}

impl From<ContractAddress> for PyFelt {
    fn from(address: ContractAddress) -> Self {
        Self(*address.0.key())
    }
}

impl From<EthAddress> for PyFelt {
    fn from(address: EthAddress) -> Self {
        let address_as_bytes: [u8; 20] = address.0.to_fixed_bytes();
        // Pad with 12 zeros.
        let mut bytes = [0; 32];
        bytes[12..32].copy_from_slice(&address_as_bytes);
        PyFelt(StarkFelt::new(bytes).expect("Convert Ethereum address to StarkFelt"))
    }
}

impl From<ClassHash> for PyFelt {
    fn from(class_hash: ClassHash) -> Self {
        Self(class_hash.0)
    }
}

impl From<CompiledClassHash> for PyFelt {
    fn from(compiled_class_hash: CompiledClassHash) -> Self {
        Self(compiled_class_hash.0)
    }
}

fn int_to_stark_felt(int: &PyAny) -> PyResult<StarkFelt> {
    let biguint: BigUint = int.extract()?;
    biguint_to_felt(biguint).map_err(|e| PyValueError::new_err(e.to_string()))
}

// TODO: Convert to a `TryFrom` cast and put in starknet-api (In StarkFelt).
pub fn biguint_to_felt(biguint: BigUint) -> NativeBlockifierResult<StarkFelt> {
    let biguint_hex = format!("{biguint:#x}");
    Ok(StarkFelt::try_from(&*biguint_hex)?)
}

pub fn to_py_vec<T, PyT, F>(values: Vec<T>, converter: F) -> Vec<PyT>
where
    F: FnMut(T) -> PyT,
{
    values.into_iter().map(converter).collect()
}

pub fn int_to_chain_id(int: &PyAny) -> PyResult<ChainId> {
    let biguint: BigUint = int.extract()?;
    Ok(ChainId(String::from_utf8_lossy(&biguint.to_bytes_be()).into()))
}

pub fn to_entry_point_type(value: &PyAny) -> PyResult<EntryPointType> {
    let value: u8 = value.getattr("value")?.extract()?;
    match value {
        0 => Ok(EntryPointType::External),
        1 => Ok(EntryPointType::L1Handler),
        2 => Ok(EntryPointType::Constructor),
        _ => Err(PyTypeError::new_err("Invalid EntryPointType")),
    }
}

pub fn builtin_name_from_str(s: String) -> NativeBlockifierResult<BuiltinName> {
    match s.as_str() {
        "output" => Ok(BuiltinName::output),
        "range_check" => Ok(BuiltinName::range_check),
        "pedersen" => Ok(BuiltinName::pedersen),
        "ecdsa" => Ok(BuiltinName::ecdsa),
        "keccak" => Ok(BuiltinName::keccak),
        "bitwise" => Ok(BuiltinName::bitwise),
        "ec_op" => Ok(BuiltinName::ec_op),
        "poseidon" => Ok(BuiltinName::poseidon),
        "segment_arena" => Ok(BuiltinName::segment_arena),
        _ => Err(NativeBlockifierError::NativeBlockifierInputError(
            NativeBlockifierInputError::InvalidInput,
        )),
    }
}

// TODO(Dori, 1/4/2023): If and when supported in the Python build environment, use #[cfg(test)].
#[pyfunction]
pub fn raise_error_for_testing() -> NativeBlockifierResult<()> {
    Err(TransactionExecutionError::CairoResourcesNotContainedInFeeCosts.into())
}

pub fn py_attr<T>(obj: &PyAny, attr: &str) -> NativeBlockifierResult<T>
where
    T: for<'a> FromPyObject<'a>,
    T: Clone,
{
    Ok(obj.getattr(attr)?.extract()?)
}

pub fn py_enum_name<T>(obj: &PyAny, attr: &str) -> NativeBlockifierResult<T>
where
    T: for<'a> FromPyObject<'a>,
    T: Clone,
    T: ToString,
{
    py_attr(obj.getattr(attr)?, "name")
}
