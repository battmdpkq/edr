use std::{convert::Infallible, num::NonZeroU64, time::SystemTime};

use anyhow::anyhow;
#[allow(deprecated)]
// This is test code, it's ok to use `DangerousSecretKeyStr`
use edr_eth::{
    block::{miner_reward, BlobGas, BlockOptions},
    receipt::BlockReceipt,
    signature::{secret_key_from_str, DangerousSecretKeyStr},
    spec::chain_hardfork_activations,
    transaction::EthTransactionRequest,
    trie::KECCAK_NULL_RLP,
    withdrawal::Withdrawal,
    Address, Bytes, HashMap, PreEip1898BlockSpec, SpecId, B256, U256,
};
use edr_evm::{
    alloy_primitives::U160,
    blockchain::{Blockchain as _, ForkedBlockchain},
    chain_spec::L1ChainSpec,
    state::IrregularState,
    Block, BlockBuilder, CfgEnv, CfgEnvWithHandlerCfg, DebugContext, ExecutionResultWithContext,
    IntoRemoteBlock, RandomHashGenerator,
};
use edr_rpc_eth::client::EthRpcClient;

use super::{
    AccountConfig, Arc, Debug, MethodInvocation, Provider, ProviderConfig, ProviderData,
    ProviderError, ProviderRequest, TimeSinceEpoch,
};
use crate::{config::MiningConfig, requests::hardhat::rpc_types::ForkConfig};

pub const TEST_SECRET_KEY: &str =
    "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

// Address 0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826
pub const TEST_SECRET_KEY_SIGN_TYPED_DATA_V4: &str =
    "0xc85ef7d79691fe79573b1a7064c19c1a9819ebdbd1faaab1a8ec92344438aaf4";

pub const FORK_BLOCK_NUMBER: u64 = 18_725_000;

/// Constructs a test config with a single account with 1 ether
pub fn create_test_config() -> ProviderConfig {
    create_test_config_with_fork(None)
}

pub fn one_ether() -> U256 {
    U256::from(10).pow(U256::from(18))
}

pub fn create_test_config_with_fork(fork: Option<ForkConfig>) -> ProviderConfig {
    ProviderConfig {
        accounts: vec![
            AccountConfig {
                // This is test code, it's ok to use `DangerousSecretKeyStr`
                // Can't use `edr_test_utils` as a dependency here.
                #[allow(deprecated)]
                secret_key: secret_key_from_str(DangerousSecretKeyStr(TEST_SECRET_KEY))
                    .expect("should construct secret key from string"),
                balance: one_ether(),
            },
            AccountConfig {
                // This is test code, it's ok to use `DangerousSecretKeyStr`
                // Can't use `edr_test_utils` as a dependency here.
                #[allow(deprecated)]
                secret_key: secret_key_from_str(DangerousSecretKeyStr(
                    TEST_SECRET_KEY_SIGN_TYPED_DATA_V4,
                ))
                .expect("should construct secret key from string"),
                balance: one_ether(),
            },
        ],
        allow_blocks_with_same_timestamp: false,
        allow_unlimited_contract_size: false,
        bail_on_call_failure: false,
        bail_on_transaction_failure: false,
        // SAFETY: literal is non-zero
        block_gas_limit: unsafe { NonZeroU64::new_unchecked(30_000_000) },
        chain_id: 123,
        chains: HashMap::new(),
        coinbase: Address::from(U160::from(1)),
        enable_rip_7212: false,
        fork,
        genesis_accounts: HashMap::new(),
        hardfork: SpecId::LATEST,
        initial_base_fee_per_gas: Some(U256::from(1000000000)),
        initial_blob_gas: Some(BlobGas {
            gas_used: 0,
            excess_gas: 0,
        }),
        initial_date: Some(SystemTime::now()),
        initial_parent_beacon_block_root: Some(KECCAK_NULL_RLP),
        min_gas_price: U256::ZERO,
        mining: MiningConfig::default(),
        network_id: 123,
        cache_dir: edr_defaults::CACHE_DIR.into(),
    }
}

/// Retrieves the pending base fee per gas from the provider data.
pub fn pending_base_fee(
    data: &mut ProviderData<Infallible>,
) -> Result<U256, ProviderError<Infallible>> {
    let block = data.mine_pending_block()?.block;

    let base_fee = block
        .header()
        .base_fee_per_gas
        .unwrap_or_else(|| U256::from(1));

    Ok(base_fee)
}

/// Deploys a contract with the provided code. Returns the address of the
/// contract.
pub fn deploy_contract<LoggerErrorT, TimerT>(
    provider: &Provider<LoggerErrorT, TimerT>,
    caller: Address,
    code: Bytes,
) -> anyhow::Result<Address>
where
    LoggerErrorT: Debug + Send + Sync + 'static,
    TimerT: Clone + TimeSinceEpoch,
{
    let deploy_transaction = EthTransactionRequest {
        from: caller,
        data: Some(code),
        ..EthTransactionRequest::default()
    };

    let result = provider.handle_request(ProviderRequest::Single(
        MethodInvocation::SendTransaction(deploy_transaction),
    ))?;

    let transaction_hash: B256 = serde_json::from_value(result.result)?;

    let result = provider.handle_request(ProviderRequest::Single(
        MethodInvocation::GetTransactionReceipt(transaction_hash),
    ))?;

    let receipt: BlockReceipt = serde_json::from_value(result.result)?;
    let contract_address = receipt.contract_address.expect("Call must create contract");

    Ok(contract_address)
}

/// Runs a full remote block, asserting that the mined block matches the remote
/// block.
pub async fn run_full_block(url: String, block_number: u64, chain_id: u64) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Handle::current();
    let default_config = create_test_config_with_fork(Some(ForkConfig {
        json_rpc_url: url.clone(),
        block_number: Some(block_number - 1),
        http_headers: None,
    }));

    let replay_block = {
        let rpc_client =
            EthRpcClient::<L1ChainSpec>::new(&url, default_config.cache_dir.clone(), None)?;

        let block = rpc_client
            .get_block_by_number_with_transaction_data(PreEip1898BlockSpec::Number(block_number))
            .await?;

        block.into_remote_block(Arc::new(rpc_client), runtime.clone())?
    };

    let rpc_client =
        EthRpcClient::<L1ChainSpec>::new(&url, default_config.cache_dir.clone(), None)?;
    let mut irregular_state = IrregularState::default();
    let state_root_generator = Arc::new(parking_lot::Mutex::new(RandomHashGenerator::with_seed(
        edr_defaults::STATE_ROOT_HASH_SEED,
    )));
    let hardfork_activation_overrides = HashMap::new();

    let hardfork_activations =
        chain_hardfork_activations(chain_id).ok_or(anyhow!("Unsupported chain id"))?;

    let spec_id = hardfork_activations
        .hardfork_at_block_number(block_number)
        .ok_or(anyhow!("Unsupported block number"))?;

    let blockchain = ForkedBlockchain::new(
        runtime.clone(),
        Some(chain_id),
        spec_id,
        Arc::new(rpc_client),
        Some(block_number - 1),
        &mut irregular_state,
        state_root_generator,
        &hardfork_activation_overrides,
    )
    .await?;

    let mut cfg = CfgEnv::default();
    cfg.chain_id = chain_id;
    cfg.disable_eip3607 = true;

    let cfg = CfgEnvWithHandlerCfg::new_with_spec_id(cfg, spec_id);

    let parent = blockchain.last_block()?;
    let replay_header = replay_block.header();

    let mut builder = BlockBuilder::new(
        cfg,
        &parent,
        BlockOptions {
            beneficiary: Some(replay_header.beneficiary),
            gas_limit: Some(replay_header.gas_limit),
            extra_data: Some(replay_header.extra_data.clone()),
            mix_hash: Some(replay_header.mix_hash),
            nonce: Some(replay_header.nonce),
            parent_beacon_block_root: replay_header.parent_beacon_block_root,
            state_root: Some(replay_header.state_root),
            timestamp: Some(replay_header.timestamp),
            withdrawals: replay_block.withdrawals().map(<[Withdrawal]>::to_vec),
            ..BlockOptions::default()
        },
        None,
    )?;

    let mut state =
        blockchain.state_at_block_number(block_number - 1, irregular_state.state_overrides())?;

    for transaction in replay_block.transactions() {
        let debug_context: Option<DebugContext<'_, L1ChainSpec, _, (), _>> = None;
        let ExecutionResultWithContext {
            result,
            evm_context: _,
        } = builder.add_transaction(&blockchain, &mut state, transaction.clone(), debug_context);

        result?;
    }

    let rewards = vec![(
        replay_header.beneficiary,
        miner_reward(spec_id).unwrap_or(U256::ZERO),
    )];
    let mined_block = builder.finalize(&mut state, rewards)?;

    let mined_header = mined_block.block.header();
    for (expected, actual) in replay_block
        .transaction_receipts()?
        .into_iter()
        .zip(mined_block.block.transaction_receipts().iter())
    {
        debug_assert_eq!(
            expected.block_number,
            actual.block_number,
            "{:?}",
            replay_block.transactions()[expected.transaction_index as usize]
        );
        debug_assert_eq!(
            expected.transaction_hash,
            actual.transaction_hash,
            "{:?}",
            replay_block.transactions()[expected.transaction_index as usize]
        );
        debug_assert_eq!(
            expected.transaction_index,
            actual.transaction_index,
            "{:?}",
            replay_block.transactions()[expected.transaction_index as usize]
        );
        debug_assert_eq!(
            expected.from,
            actual.from,
            "{:?}",
            replay_block.transactions()[expected.transaction_index as usize]
        );
        debug_assert_eq!(
            expected.to,
            actual.to,
            "{:?}",
            replay_block.transactions()[expected.transaction_index as usize]
        );
        debug_assert_eq!(
            expected.contract_address,
            actual.contract_address,
            "{:?}",
            replay_block.transactions()[expected.transaction_index as usize]
        );
        debug_assert_eq!(
            expected.gas_used,
            actual.gas_used,
            "{:?}",
            replay_block.transactions()[expected.transaction_index as usize]
        );
        debug_assert_eq!(
            expected.effective_gas_price,
            actual.effective_gas_price,
            "{:?}",
            replay_block.transactions()[expected.transaction_index as usize]
        );
        debug_assert_eq!(
            expected.cumulative_gas_used,
            actual.cumulative_gas_used,
            "{:?}",
            replay_block.transactions()[expected.transaction_index as usize]
        );
        if expected.logs_bloom != actual.logs_bloom {
            for (expected, actual) in expected.logs.iter().zip(actual.logs.iter()) {
                debug_assert_eq!(
                    expected.inner.address,
                    actual.inner.address,
                    "{:?}",
                    replay_block.transactions()[expected.transaction_index as usize]
                );
                debug_assert_eq!(
                    expected.inner.topics(),
                    actual.inner.topics(),
                    "{:?}",
                    replay_block.transactions()[expected.transaction_index as usize]
                );
                debug_assert_eq!(
                    expected.inner.data.data,
                    actual.inner.data.data,
                    "{:?}",
                    replay_block.transactions()[expected.transaction_index as usize]
                );
            }
        }
        debug_assert_eq!(
            expected.data,
            actual.data,
            "{:?}",
            replay_block.transactions()[expected.transaction_index as usize]
        );
    }

    assert_eq!(mined_header, replay_header);

    Ok(())
}
