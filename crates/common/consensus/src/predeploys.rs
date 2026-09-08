//! Addresses of Base pre-deploys.
//!
//! This module contains predeploy contract addresses and system addresses for Base.
//! See the complete set of predeploys at <https://specs.base.org/protocol/execution/evm/predeploys#predeploys>

use alloy_primitives::{Address, address, hex};

/// Container for all predeploy contract addresses
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Predeploys;

impl Predeploys {
    /// List of all predeploys.
    pub const ALL: [Address; 25] = [
        Self::LEGACY_MESSAGE_PASSER,
        Self::DEPLOYER_WHITELIST,
        Self::LEGACY_ERC20_ETH,
        Self::WETH9,
        Self::L2_CROSS_DOMAIN_MESSENGER,
        Self::L2_STANDARD_BRIDGE,
        Self::SEQUENCER_FEE_VAULT,
        Self::BASE_MINTABLE_ERC20_FACTORY,
        Self::L1_BLOCK_NUMBER,
        Self::GAS_PRICE_ORACLE,
        Self::GOVERNANCE_TOKEN,
        Self::L1_BLOCK_INFO,
        Self::L2_TO_L1_MESSAGE_PASSER,
        Self::L2_ERC721_BRIDGE,
        Self::BASE_MINTABLE_ERC721_FACTORY,
        Self::PROXY_ADMIN,
        Self::BASE_FEE_VAULT,
        Self::L1_FEE_VAULT,
        Self::SCHEMA_REGISTRY,
        Self::EAS,
        Self::BEACON_BLOCK_ROOT,
        Self::OPERATOR_FEE_VAULT,
        Self::CROSS_L2_INBOX,
        Self::L2_TO_L2_XDM,
        Self::BASE_TIME,
    ];

    /// The `LegacyMessagePasser` contract stores commitments to withdrawal transactions before the
    /// Bedrock upgrade.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#legacymessagepasser>
    pub const LEGACY_MESSAGE_PASSER: Address =
        address!("0x4200000000000000000000000000000000000000");

    /// The `DeployerWhitelist` was used to provide additional safety during initial phases of
    /// Base.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#deployerwhitelist>
    pub const DEPLOYER_WHITELIST: Address = address!("0x4200000000000000000000000000000000000002");

    /// The `LegacyERC20ETH` predeploy represented all ether in the system before the Bedrock upgrade.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#legacyerc20eth>
    pub const LEGACY_ERC20_ETH: Address = address!("0xDeadDeAddeAddEAddeadDEaDDEAdDeaDDeAD0000");

    /// The WETH9 predeploy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#weth9>
    pub const WETH9: Address = address!("0x4200000000000000000000000000000000000006");

    /// Higher level API for sending cross domain messages.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#l2crossdomainmessenger>
    pub const L2_CROSS_DOMAIN_MESSENGER: Address =
        address!("0x4200000000000000000000000000000000000007");

    /// The L2 cross-domain messenger proxy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#l2standardbridge>
    pub const L2_STANDARD_BRIDGE: Address = address!("0x4200000000000000000000000000000000000010");

    /// The sequencer fee vault proxy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#sequencerfeevault>
    pub const SEQUENCER_FEE_VAULT: Address = address!("0x4200000000000000000000000000000000000011");

    /// The mintable ERC20 factory proxy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#optimismmintableerc20factory>
    pub const BASE_MINTABLE_ERC20_FACTORY: Address =
        address!("0x4200000000000000000000000000000000000012");

    /// Returns the last known L1 block number (legacy system).
    /// <https://specs.base.org/protocol/execution/evm/predeploys#l1blocknumber>
    pub const L1_BLOCK_NUMBER: Address = address!("0x4200000000000000000000000000000000000013");

    /// The gas price oracle proxy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#gaspriceoracle>
    pub const GAS_PRICE_ORACLE: Address = address!("0x420000000000000000000000000000000000000F");

    /// The governance token proxy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#governancetoken>
    pub const GOVERNANCE_TOKEN: Address = address!("0x4200000000000000000000000000000000000042");

    /// The L1 block information proxy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#l1block>
    pub const L1_BLOCK_INFO: Address = address!("0x4200000000000000000000000000000000000015");

    /// The L2 contract `L2ToL1MessagePasser`, stores commitments to withdrawal transactions.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#l2tol1messagepasser>
    pub const L2_TO_L1_MESSAGE_PASSER: Address =
        address!("0x4200000000000000000000000000000000000016");

    /// The L2 ERC721 bridge proxy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys>
    pub const L2_ERC721_BRIDGE: Address = address!("0x4200000000000000000000000000000000000014");

    /// The mintable ERC721 proxy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#optimismmintableerc721factory>
    pub const BASE_MINTABLE_ERC721_FACTORY: Address =
        address!("0x4200000000000000000000000000000000000017");

    /// The L2 proxy admin address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#proxyadmin>
    pub const PROXY_ADMIN: Address = address!("0x4200000000000000000000000000000000000018");

    /// The base fee vault address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#basefeevault>
    pub const BASE_FEE_VAULT: Address = address!("0x4200000000000000000000000000000000000019");

    /// The L1 fee vault address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#l1feevault>
    pub const L1_FEE_VAULT: Address = address!("0x420000000000000000000000000000000000001a");

    /// The schema registry proxy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#schemaregistry>
    pub const SCHEMA_REGISTRY: Address = address!("0x4200000000000000000000000000000000000020");

    /// The EAS proxy address.
    /// <https://specs.base.org/protocol/execution/evm/predeploys#eas>
    pub const EAS: Address = address!("0x4200000000000000000000000000000000000021");

    /// Provides access to L1 beacon block roots (EIP-4788).
    /// <https://specs.base.org/protocol/execution/evm/predeploys#beacon-block-root>
    pub const BEACON_BLOCK_ROOT: Address = address!("0x000F3df6D732807Ef1319fB7B8bB8522d0Beac02");

    /// The Operator Fee Vault proxy address.
    pub const OPERATOR_FEE_VAULT: Address = address!("0x420000000000000000000000000000000000001B");

    /// The `CrossL2Inbox` proxy address.
    pub const CROSS_L2_INBOX: Address = address!("0x4200000000000000000000000000000000000022");

    /// The `L2ToL2CrossDomainMessenger` proxy address.
    pub const L2_TO_L2_XDM: Address = address!("0x4200000000000000000000000000000000000023");

    /// The `BaseTime` predeploy address.
    pub const BASE_TIME: Address = address!("0x4200000000000000000000000000000000000030");
}

/// The canonical deterministic-deployment CREATE2 factory (the "Arachnid proxy"),
/// formalized as a required predeploy by [EIP-7997].
///
/// The factory has fixed runtime bytecode at a fixed address on every chain. A call
/// treats the first 32 bytes of input as the CREATE2 salt and the remaining bytes as
/// init code, forwards call value, and returns the 20-byte deployed address.
///
/// EIP-7997 allows the account to be established either by its keyless creation
/// transaction or by inclusion in the genesis state with a nonce of 1. On existing
/// Base networks it is already present via the keyless transaction (Base mainnet
/// carries [`Self::RUNTIME_CODE`] with a nonzero nonce), so no fork-boundary action
/// is taken: per EIP-7997, "client software MUST NOT check for the existence of the
/// contract at the fork boundary". New chains include it in their genesis allocation
/// using [`Self::ADDRESS`], [`Self::RUNTIME_CODE`], and [`Self::GENESIS_NONCE`].
///
/// This is deliberately kept separate from [`Predeploys`], which are the proxied
/// `0x42..` OP system contracts; the factory is a plain, non-proxied account.
///
/// [EIP-7997]: https://eips.ethereum.org/EIPS/eip-7997
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeterministicDeploymentProxy;

impl DeterministicDeploymentProxy {
    /// The fixed factory address, equal to the CREATE address of the keyless
    /// deployment signer at nonce 0.
    pub const ADDRESS: Address = address!("0x4e59b44847b379578588920cA78FbF26c0B4956C");

    /// The keyless signer whose nonce-0 CREATE establishes [`Self::ADDRESS`].
    pub const DEPLOYER: Address = address!("0x3fAB184622Dc19b6109349B94811493BF2a45362");

    /// The fixed runtime bytecode required by EIP-7997.
    pub const RUNTIME_CODE: &'static [u8] = &hex!(
        "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe03601600081602082378035828234f58015156039578182fd5b8082525050506014600cf3"
    );

    /// The account nonce required when the factory is included in a chain's genesis
    /// allocation.
    pub const GENESIS_NONCE: u64 = 1;
}

/// Container for system addresses that are not predeploy contracts.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SystemAddresses;

impl SystemAddresses {
    /// The depositor address of the L1 attributes transaction (`L1Block` contract depositor).
    /// <https://specs.base.org/protocol/bridging/deposits#l1-attributes-deposited-transaction>
    pub const DEPOSITOR_ACCOUNT: Address = address!("0xDeaDDEaDDeAdDeAdDEAdDEaddeAddEAdDEAd0001");
}

/// Container for system deployer addresses used during protocol upgrades.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Deployers;

impl Deployers {
    /// Ecotone L1 Block deployer address.
    pub const ECOTONE_L1_BLOCK: Address = address!("4210000000000000000000000000000000000000");

    /// Ecotone Gas Price Oracle deployer address.
    pub const ECOTONE_GAS_PRICE_ORACLE: Address =
        address!("4210000000000000000000000000000000000001");

    /// Fjord Gas Price Oracle deployer address.
    pub const FJORD_GAS_PRICE_ORACLE: Address =
        address!("4210000000000000000000000000000000000002");

    /// Isthmus L1 Block deployer address.
    pub const ISTHMUS_L1_BLOCK: Address = address!("4210000000000000000000000000000000000003");

    /// Isthmus Gas Price Oracle deployer address.
    pub const ISTHMUS_GAS_PRICE_ORACLE: Address =
        address!("4210000000000000000000000000000000000004");

    /// Isthmus Operator Fee Vault deployer address.
    pub const ISTHMUS_OPERATOR_FEE_VAULT: Address =
        address!("4210000000000000000000000000000000000005");

    /// Jovian L1 Block deployer address.
    pub const JOVIAN_L1_BLOCK: Address = address!("4210000000000000000000000000000000000006");

    /// Jovian Gas Price Oracle deployer address.
    pub const JOVIAN_GAS_PRICE_ORACLE: Address =
        address!("4210000000000000000000000000000000000007");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_address_is_create_of_keyless_deployer() {
        // Per EIP-7997 the factory lives at the CREATE address of the keyless
        // deployment signer at nonce 0. Deriving it independently guards the
        // address constant against typos.
        assert_eq!(
            DeterministicDeploymentProxy::DEPLOYER.create(0),
            DeterministicDeploymentProxy::ADDRESS,
        );
    }

    #[test]
    fn factory_runtime_code_matches_eip7997() {
        // Golden value fixed by EIP-7997 and verified live on Base mainnet via
        // `eth_getCode(0x4e59…56C)`. A single wrong byte silently breaks CREATE2
        // determinism, so pin the exact bytecode.
        assert_eq!(
            hex::encode(DeterministicDeploymentProxy::RUNTIME_CODE),
            "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe03601600081602082378035828234f58015156039578182fd5b8082525050506014600cf3",
        );
        assert_eq!(DeterministicDeploymentProxy::GENESIS_NONCE, 1);
    }
}
