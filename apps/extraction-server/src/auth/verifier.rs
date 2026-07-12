use std::sync::Arc;

use alloy::{
    primitives::{Address, Bytes, FixedBytes},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    sol,
    sol_types::SolCall,
};
use async_trait::async_trait;
use signinwithethereum::{Message, VerificationError, VerificationOpts};
use time::OffsetDateTime;
use tokio::sync::Semaphore;

use super::{AuthConfig, MAINNET_CHAIN_ID};

const EIP1271_MAGIC_VALUE: [u8; 4] = [0x16, 0x26, 0xba, 0x7e];
const EIP6492_MAGIC_SUFFIX: [u8; 32] = [
    0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92,
    0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92,
];

sol! {
    function isValidSignature(bytes32 hash, bytes signature) external view returns (bytes4);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureVerificationError {
    Invalid,
    WrongNetwork,
    Unavailable,
}

#[async_trait]
pub trait SignatureVerifier: Send + Sync {
    async fn verify(
        &self,
        message: &Message,
        signature: &[u8],
        expected_nonce: &str,
        now: OffsetDateTime,
    ) -> Result<(), SignatureVerificationError>;
}

#[derive(Clone)]
pub struct SiweSignatureVerifier {
    config: AuthConfig,
    rpc_limit: Arc<Semaphore>,
}

impl SiweSignatureVerifier {
    pub fn new(config: AuthConfig) -> Self {
        Self {
            rpc_limit: Arc::new(Semaphore::new(config.rpc_concurrency.max(1))),
            config,
        }
    }

    fn validate_fields(
        &self,
        message: &Message,
        expected_nonce: &str,
        now: OffsetDateTime,
    ) -> Result<(), SignatureVerificationError> {
        if message.chain_id != MAINNET_CHAIN_ID as u64 {
            return Err(SignatureVerificationError::WrongNetwork);
        }
        if message.domain.as_str() != self.config.domain
            || message.uri.as_str() != self.config.uri
            || message.nonce != expected_nonce
        {
            return Err(SignatureVerificationError::Invalid);
        }

        let expected_scheme = self
            .config
            .uri
            .split_once(':')
            .map(|(scheme, _)| scheme)
            .ok_or(SignatureVerificationError::Invalid)?;
        let effective_scheme = message.scheme.as_deref().unwrap_or(expected_scheme);
        if effective_scheme != expected_scheme {
            return Err(SignatureVerificationError::Invalid);
        }

        let issued_at = *message.issued_at.as_ref();
        let expiration = message
            .expiration_time
            .as_ref()
            .map(|value| *value.as_ref())
            .ok_or(SignatureVerificationError::Invalid)?;
        if issued_at > now + self.config.clock_skew
            || issued_at < now - self.config.max_message_age
            || expiration <= issued_at
            || expiration > issued_at + self.config.nonce_ttl
            || !message.valid_at(&now)
        {
            return Err(SignatureVerificationError::Invalid);
        }
        Ok(())
    }

    fn verification_options(
        &self,
        nonce: &str,
        now: OffsetDateTime,
    ) -> Result<VerificationOpts, SignatureVerificationError> {
        Ok(VerificationOpts {
            domain: Some(
                self.config
                    .domain
                    .parse()
                    .map_err(|_| SignatureVerificationError::Unavailable)?,
            ),
            nonce: Some(nonce.to_owned()),
            timestamp: Some(now),
            uri: Some(
                self.config
                    .uri
                    .parse()
                    .map_err(|_| SignatureVerificationError::Unavailable)?,
            ),
            chain_id: Some(MAINNET_CHAIN_ID as u64),
            scheme: None,
            rpc_url: self.config.rpc_url.clone(),
        })
    }

    async fn verify_eip1271(
        &self,
        message: &Message,
        signature: &[u8],
        rpc_url: &str,
    ) -> Result<(), SignatureVerificationError> {
        let rpc_url = rpc_url
            .parse()
            .map_err(|_| SignatureVerificationError::Unavailable)?;
        let provider = ProviderBuilder::new().connect_http(rpc_url);
        let rpc_chain_id = provider
            .get_chain_id()
            .await
            .map_err(|_| SignatureVerificationError::Unavailable)?;
        if rpc_chain_id != MAINNET_CHAIN_ID as u64 {
            return Err(SignatureVerificationError::Unavailable);
        }

        let message_hash = message
            .eip191_hash()
            .map_err(|_| SignatureVerificationError::Invalid)?;
        let call = isValidSignatureCall {
            hash: FixedBytes::from(message_hash),
            signature: Bytes::copy_from_slice(signature),
        };
        let transaction = TransactionRequest::default()
            .to(Address::from(message.address))
            .input(Bytes::from(call.abi_encode()).into());
        let result = provider
            .call(transaction)
            .await
            .map_err(|_| SignatureVerificationError::Unavailable)?;

        if result.len() >= EIP1271_MAGIC_VALUE.len()
            && result[..EIP1271_MAGIC_VALUE.len()] == EIP1271_MAGIC_VALUE
        {
            Ok(())
        } else {
            Err(SignatureVerificationError::Invalid)
        }
    }
}

#[async_trait]
impl SignatureVerifier for SiweSignatureVerifier {
    async fn verify(
        &self,
        message: &Message,
        signature: &[u8],
        expected_nonce: &str,
        now: OffsetDateTime,
    ) -> Result<(), SignatureVerificationError> {
        self.validate_fields(message, expected_nonce, now)?;

        if let Ok(signature) = <&[u8; 65]>::try_from(signature) {
            if message.verify_eip191(signature).is_ok() {
                return Ok(());
            }
        }

        let Some(rpc_url) = self.config.rpc_url.as_deref() else {
            return Err(SignatureVerificationError::Unavailable);
        };
        // 拥塞时立即失败，由 HTTP 层返回可重试错误，避免为匿名请求积累等待任务。
        let _permit = self
            .rpc_limit
            .try_acquire()
            .map_err(|_| SignatureVerificationError::Unavailable)?;
        let rpc_verification = async {
            if is_eip6492_signature(signature) {
                let options = self.verification_options(expected_nonce, now)?;
                message
                    .verify(signature, &options)
                    .await
                    .map_err(map_verification_error)
            } else {
                // 上游会吞掉 EIP-1271 eth_call 故障；此处直接调用以保留故障分类。
                self.verify_eip1271(message, signature, rpc_url).await
            }
        };

        tokio::time::timeout(self.config.rpc_timeout, rpc_verification)
            .await
            .map_err(|_| SignatureVerificationError::Unavailable)?
    }
}

fn is_eip6492_signature(signature: &[u8]) -> bool {
    signature.len() > EIP6492_MAGIC_SUFFIX.len()
        && signature[signature.len() - EIP6492_MAGIC_SUFFIX.len()..] == EIP6492_MAGIC_SUFFIX
}

fn map_verification_error(error: VerificationError) -> SignatureVerificationError {
    match error {
        VerificationError::ChainIdMismatch => SignatureVerificationError::WrongNetwork,
        VerificationError::ContractCall(_)
        | VerificationError::RpcChainIdMismatch { .. }
        | VerificationError::RpcRequired => SignatureVerificationError::Unavailable,
        _ => SignatureVerificationError::Invalid,
    }
}
