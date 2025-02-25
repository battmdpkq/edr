use core::fmt::Debug;
use std::convert::Infallible;

use dyn_clone::DynClone;
use edr_eth::transaction;
use edr_evm::blockchain::BlockchainError;

use crate::{
    data::CallResult, debug_mine::DebugMineBlockResult, error::EstimateGasFailure, ProviderError,
};

pub trait Logger {
    type BlockchainError;

    type LoggerError: Debug;

    /// Whether the logger is enabled.
    fn is_enabled(&self) -> bool;

    /// Sets whether the logger is enabled.
    fn set_is_enabled(&mut self, is_enabled: bool);

    fn log_call(
        &mut self,
        spec_id: edr_eth::SpecId,
        transaction: &transaction::Signed,
        result: &CallResult,
    ) -> Result<(), Self::LoggerError> {
        let _spec_id = spec_id;
        let _transaction = transaction;
        let _result = result;

        Ok(())
    }

    fn log_estimate_gas_failure(
        &mut self,
        spec_id: edr_eth::SpecId,
        transaction: &transaction::Signed,
        result: &EstimateGasFailure,
    ) -> Result<(), Self::LoggerError> {
        let _spec_id = spec_id;
        let _transaction = transaction;
        let _failure = result;

        Ok(())
    }

    fn log_interval_mined(
        &mut self,
        spec_id: edr_eth::SpecId,
        result: &DebugMineBlockResult<Self::BlockchainError>,
    ) -> Result<(), Self::LoggerError> {
        let _spec_id = spec_id;
        let _result = result;

        Ok(())
    }

    fn log_mined_block(
        &mut self,
        spec_id: edr_eth::SpecId,
        results: &[DebugMineBlockResult<Self::BlockchainError>],
    ) -> Result<(), Self::LoggerError> {
        let _spec_id = spec_id;
        let _results = results;

        Ok(())
    }

    fn log_send_transaction(
        &mut self,
        spec_id: edr_eth::SpecId,
        transaction: &transaction::Signed,
        mining_results: &[DebugMineBlockResult<Self::BlockchainError>],
    ) -> Result<(), Self::LoggerError> {
        let _spec_id = spec_id;
        let _transaction = transaction;
        let _mining_results = mining_results;

        Ok(())
    }

    /// Prints the collected logs, which correspond to the method with the
    /// provided name.
    ///
    /// Adds an empty line at the end.
    fn print_method_logs(
        &mut self,
        method: &str,
        error: Option<&ProviderError<Self::LoggerError>>,
    ) -> Result<(), Self::LoggerError>;

    fn print_contract_decoding_error(&mut self, error: &str) -> Result<(), Self::LoggerError>;
}

pub trait SyncLogger: Logger + DynClone + Send + Sync {}

impl<T> SyncLogger for T where T: Logger + DynClone + Send + Sync {}

impl<BlockchainErrorT, LoggerErrorT> Clone
    for Box<dyn SyncLogger<BlockchainError = BlockchainErrorT, LoggerError = LoggerErrorT>>
{
    fn clone(&self) -> Self {
        dyn_clone::clone_box(&**self)
    }
}

/// A logger that does nothing.
#[derive(Clone, Default)]
pub struct NoopLogger;

impl Logger for NoopLogger {
    type BlockchainError = BlockchainError;

    type LoggerError = Infallible;

    fn is_enabled(&self) -> bool {
        false
    }

    fn set_is_enabled(&mut self, _is_enabled: bool) {}

    fn print_method_logs(
        &mut self,
        _method: &str,
        _error: Option<&ProviderError<Infallible>>,
    ) -> Result<(), Infallible> {
        Ok(())
    }

    fn print_contract_decoding_error(&mut self, _error: &str) -> Result<(), Self::LoggerError> {
        Ok(())
    }
}
