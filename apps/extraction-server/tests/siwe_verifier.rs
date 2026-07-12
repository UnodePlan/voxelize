use std::{
    net::TcpListener,
    sync::{Arc, Mutex},
    time::{Duration as StdDuration, Instant},
};

use actix_web::{dev::ServerHandle, web, App, HttpResponse, HttpServer};
use extraction_server::auth::{
    AuthConfig, SignatureVerificationError, SignatureVerifier, SiweSignatureVerifier,
};
use serde_json::{json, Value};
use signinwithethereum::Message;
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

const OFFICIAL_NONCE: &str = "bTyXgcQxn2htgkjJn";
const OFFICIAL_SIGNATURE: &str = "7fcf011b4dff0a6024ce6cea93dcd0ebc4592f43e7685e174d4d3d4a1f942ed75bbf5358841924dc8e1b261f750dfec5dbcc0aad9a3d9439f1e5c4d59d7d98a81c";
const OFFICIAL_MESSAGE: &str = "siwe.xyz wants you to sign in with your Ethereum account:\n0xAE9aA90F1a627c7a20783AF9e8747fCFEDEFAd03\n\nSign In with Ethereum Example Statement\n\nURI: https://siwe.xyz\nVersion: 1\nChain ID: 1\nNonce: bTyXgcQxn2htgkjJn\nIssued At: 2022-01-27T17:09:38.578Z\nExpiration Time: 2100-01-07T14:31:43.952Z";
const EIP6492_MAGIC_SUFFIX: [u8; 32] = [
    0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92,
    0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92, 0x64, 0x92,
];

#[actix_web::test]
async fn official_eoa_vector_is_verified_without_rpc() {
    let now = parse_time("2022-01-27T17:10:00Z");
    let mut config = AuthConfig::local("siwe.xyz", "https://siwe.xyz");
    // 官方向量的过期时间位于 2100 年，测试配置只为容纳该固定向量。
    config.nonce_ttl = Duration::days(36_500);
    let verifier = SiweSignatureVerifier::new(config);
    let message: Message = OFFICIAL_MESSAGE.parse().unwrap();
    let signature = hex::decode(OFFICIAL_SIGNATURE).unwrap();

    assert_eq!(
        verifier
            .verify(&message, &signature, OFFICIAL_NONCE, now)
            .await,
        Ok(())
    );
}

#[actix_web::test]
async fn invalid_domain_chain_expiration_and_issued_at_are_rejected() {
    let now = parse_time("2022-01-27T17:10:00Z");
    let signature = [0_u8; 65];
    let official: Message = OFFICIAL_MESSAGE.parse().unwrap();

    let mut long_lived = AuthConfig::local("example.com", "https://siwe.xyz");
    long_lived.nonce_ttl = Duration::days(36_500);
    let wrong_domain = SiweSignatureVerifier::new(long_lived);
    assert_eq!(
        wrong_domain
            .verify(&official, &signature, OFFICIAL_NONCE, now)
            .await,
        Err(SignatureVerificationError::Invalid)
    );

    let mut wrong_uri_config = AuthConfig::local("siwe.xyz", "https://wrong.example");
    wrong_uri_config.nonce_ttl = Duration::days(36_500);
    let wrong_uri = SiweSignatureVerifier::new(wrong_uri_config);
    assert_eq!(
        wrong_uri
            .verify(&official, &signature, OFFICIAL_NONCE, now)
            .await,
        Err(SignatureVerificationError::Invalid)
    );

    let mut signature_config = AuthConfig::local("siwe.xyz", "https://siwe.xyz");
    signature_config.nonce_ttl = Duration::days(36_500);
    let wrong_signature = SiweSignatureVerifier::new(signature_config);
    assert_eq!(
        wrong_signature
            .verify(&official, &[0_u8; 65], OFFICIAL_NONCE, now)
            .await,
        Err(SignatureVerificationError::Unavailable)
    );

    let config = AuthConfig::local("siwe.xyz", "https://siwe.xyz");
    let verifier = SiweSignatureVerifier::new(config);
    let wrong_chain: Message = OFFICIAL_MESSAGE
        .replace("Chain ID: 1", "Chain ID: 5")
        .parse()
        .unwrap();
    assert_eq!(
        verifier
            .verify(&wrong_chain, &signature, OFFICIAL_NONCE, now)
            .await,
        Err(SignatureVerificationError::WrongNetwork)
    );

    let missing_expiration: Message = OFFICIAL_MESSAGE
        .replace("\nExpiration Time: 2100-01-07T14:31:43.952Z", "")
        .parse()
        .unwrap();
    assert_eq!(
        verifier
            .verify(&missing_expiration, &signature, OFFICIAL_NONCE, now)
            .await,
        Err(SignatureVerificationError::Invalid)
    );

    let future_issued_at = wallet_message(
        "siwe.xyz",
        "https://siwe.xyz",
        "0x0000000000000000000000000000000000000001",
        OFFICIAL_NONCE,
        now + Duration::seconds(31),
        now + Duration::minutes(4),
        1,
    );
    assert_eq!(
        verifier
            .verify(&future_issued_at, &signature, OFFICIAL_NONCE, now)
            .await,
        Err(SignatureVerificationError::Invalid)
    );
}

#[actix_web::test]
async fn eip1271_contract_signature_uses_rpc_magic_value() {
    let rpc = MockRpc::start(MockRpcMode::Eip1271).await;
    let now = parse_time("2026-07-13T02:00:00Z");
    let verifier = rpc_verifier(&rpc.url, StdDuration::from_secs(1));
    let message = wallet_message(
        "game.example",
        "https://game.example",
        "0x0000000000000000000000000000000000000127",
        "contractNonce1271",
        now,
        now + Duration::minutes(4),
        1,
    );

    assert_eq!(
        verifier
            .verify(&message, &[0_u8; 65], "contractNonce1271", now)
            .await,
        Ok(())
    );
    assert_rpc_calls(&rpc, &["eth_chainId", "eth_call"]);
    rpc.stop().await;
}

#[actix_web::test]
async fn invalid_signature_is_rejected_after_contract_fallback() {
    let rpc = MockRpc::start(MockRpcMode::Invalid).await;
    let now = parse_time("2026-07-13T02:00:00Z");
    let verifier = rpc_verifier(&rpc.url, StdDuration::from_secs(1));
    let message = wallet_message(
        "game.example",
        "https://game.example",
        "0x0000000000000000000000000000000000000127",
        "invalidSignature1271",
        now,
        now + Duration::minutes(4),
        1,
    );

    assert_eq!(
        verifier
            .verify(&message, &[0_u8; 65], "invalidSignature1271", now)
            .await,
        Err(SignatureVerificationError::Invalid)
    );
    assert_rpc_calls(&rpc, &["eth_chainId", "eth_call"]);
    rpc.stop().await;
}

#[actix_web::test]
async fn eip6492_counterfactual_signature_uses_universal_validator() {
    let rpc = MockRpc::start(MockRpcMode::Eip6492).await;
    let now = parse_time("2026-07-13T02:00:00Z");
    let verifier = rpc_verifier(&rpc.url, StdDuration::from_secs(1));
    let message = wallet_message(
        "game.example",
        "https://game.example",
        "0x0000000000000000000000000000000000006492",
        "contractNonce6492",
        now,
        now + Duration::minutes(4),
        1,
    );
    let mut signature = vec![0xab];
    signature.extend_from_slice(&EIP6492_MAGIC_SUFFIX);

    assert_eq!(
        verifier
            .verify(&message, &signature, "contractNonce6492", now)
            .await,
        Ok(())
    );
    assert_rpc_calls(&rpc, &["eth_chainId", "eth_call"]);
    rpc.stop().await;
}

#[actix_web::test]
async fn rpc_timeout_fails_closed() {
    let rpc = MockRpc::start(MockRpcMode::Slow).await;
    let now = parse_time("2026-07-13T02:00:00Z");
    let verifier = rpc_verifier(&rpc.url, StdDuration::from_millis(40));
    let message = wallet_message(
        "game.example",
        "https://game.example",
        "0x0000000000000000000000000000000000000127",
        "timeoutNonce1271",
        now,
        now + Duration::minutes(4),
        1,
    );

    assert_eq!(
        verifier
            .verify(&message, &[0_u8; 65], "timeoutNonce1271", now)
            .await,
        Err(SignatureVerificationError::Unavailable)
    );
    assert_rpc_calls(&rpc, &["eth_chainId", "eth_call"]);
    rpc.stop().await;
}

#[actix_web::test]
async fn eip1271_rpc_failures_after_chain_check_are_unavailable() {
    for mode in [
        MockRpcMode::HttpFailure,
        MockRpcMode::MalformedResponse,
        MockRpcMode::JsonRpcFailure,
    ] {
        let rpc = MockRpc::start(mode).await;
        let now = parse_time("2026-07-13T02:00:00Z");
        let verifier = rpc_verifier(&rpc.url, StdDuration::from_secs(1));
        let message = wallet_message(
            "game.example",
            "https://game.example",
            "0x0000000000000000000000000000000000000127",
            "rpcFailureNonce1271",
            now,
            now + Duration::minutes(4),
            1,
        );

        assert_eq!(
            verifier
                .verify(&message, &[0_u8; 65], "rpcFailureNonce1271", now)
                .await,
            Err(SignatureVerificationError::Unavailable)
        );
        assert_rpc_calls(&rpc, &["eth_chainId", "eth_call"]);
        rpc.stop().await;
    }
}

#[actix_web::test]
async fn rpc_concurrency_overflow_is_rejected_without_a_wait_queue() {
    let rpc = MockRpc::start(MockRpcMode::Slow).await;
    let now = parse_time("2026-07-13T02:00:00Z");
    let mut config = AuthConfig::local("game.example", "https://game.example");
    config.rpc_url = Some(rpc.url.clone());
    config.rpc_timeout = StdDuration::from_millis(100);
    config.rpc_concurrency = 1;
    let verifier = Arc::new(SiweSignatureVerifier::new(config));
    let message = wallet_message(
        "game.example",
        "https://game.example",
        "0x0000000000000000000000000000000000000127",
        "boundedQueueNonce1271",
        now,
        now + Duration::minutes(4),
        1,
    );

    let started_at = Instant::now();
    let attempts = (0..3)
        .map(|_| {
            let verifier = verifier.clone();
            let message = message.clone();
            actix_web::rt::spawn(async move {
                verifier
                    .verify(&message, &[0_u8; 65], "boundedQueueNonce1271", now)
                    .await
            })
        })
        .collect::<Vec<_>>();
    for attempt in attempts {
        assert_eq!(
            attempt.await.expect("RPC 验证任务不应崩溃"),
            Err(SignatureVerificationError::Unavailable)
        );
    }

    assert!(
        started_at.elapsed() < StdDuration::from_millis(220),
        "RPC 拥塞请求未及时收敛: {:?}",
        started_at.elapsed()
    );
    {
        let calls = rpc.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|method| method.as_str() == "eth_call")
                .count(),
            1,
            "并发上限之外的请求不应进入 RPC"
        );
    }
    rpc.stop().await;
}

fn parse_time(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}

fn wallet_message(
    domain: &str,
    uri: &str,
    address: &str,
    nonce: &str,
    issued_at: OffsetDateTime,
    expiration: OffsetDateTime,
    chain_id: u64,
) -> Message {
    format!(
        "{domain} wants you to sign in with your Ethereum account:\n{address}\n\nSign in to Voxel Extraction.\n\nURI: {uri}\nVersion: 1\nChain ID: {chain_id}\nNonce: {nonce}\nIssued At: {}\nExpiration Time: {}",
        issued_at.format(&Rfc3339).unwrap(),
        expiration.format(&Rfc3339).unwrap()
    )
    .parse()
    .unwrap()
}

fn rpc_verifier(rpc_url: &str, timeout: StdDuration) -> SiweSignatureVerifier {
    let mut config = AuthConfig::local("game.example", "https://game.example");
    config.rpc_url = Some(rpc_url.to_owned());
    config.rpc_timeout = timeout;
    SiweSignatureVerifier::new(config)
}

fn assert_rpc_calls(rpc: &MockRpc, expected: &[&str]) {
    let calls = rpc.calls.lock().unwrap();
    for method in expected {
        assert!(
            calls.iter().any(|call| call == method),
            "缺少 RPC 调用 {method}"
        );
    }
}

#[derive(Clone, Copy)]
enum MockRpcMode {
    Eip1271,
    Eip6492,
    Invalid,
    Slow,
    HttpFailure,
    MalformedResponse,
    JsonRpcFailure,
}

struct MockRpc {
    url: String,
    calls: Arc<Mutex<Vec<String>>>,
    handle: ServerHandle,
}

impl MockRpc {
    async fn start(mode: MockRpcMode) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let state = MockRpcState {
            mode,
            calls: calls.clone(),
        };
        let server = HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(state.clone()))
                .route("/", web::post().to(mock_rpc_handler))
        })
        .listen(listener)
        .unwrap()
        .run();
        let handle = server.handle();
        let _server_task = actix_web::rt::spawn(server);

        Self {
            url: format!("http://{address}"),
            calls,
            handle,
        }
    }

    async fn stop(self) {
        self.handle.stop(false).await;
    }
}

#[derive(Clone)]
struct MockRpcState {
    mode: MockRpcMode,
    calls: Arc<Mutex<Vec<String>>>,
}

async fn mock_rpc_handler(
    state: web::Data<MockRpcState>,
    request: web::Json<Value>,
) -> HttpResponse {
    let method = request["method"].as_str().unwrap_or_default().to_owned();
    state.calls.lock().unwrap().push(method.clone());

    if method == "eth_call" && matches!(state.mode, MockRpcMode::Slow) {
        actix_web::rt::time::sleep(StdDuration::from_millis(200)).await;
    }

    if method == "eth_call" {
        match state.mode {
            MockRpcMode::HttpFailure => return HttpResponse::ServiceUnavailable().finish(),
            MockRpcMode::MalformedResponse => {
                return HttpResponse::Ok()
                    .content_type("application/json")
                    .body("{");
            }
            MockRpcMode::JsonRpcFailure => {
                return HttpResponse::Ok().json(json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").cloned().unwrap_or(Value::Null),
                    "error": {"code": -32603, "message": "Internal error"}
                }));
            }
            _ => {}
        }
    }

    let result = match method.as_str() {
        "eth_chainId" => "0x1",
        "eth_call" => match state.mode {
            MockRpcMode::Eip1271 => "0x1626ba7e",
            MockRpcMode::Eip6492 => "0x01",
            MockRpcMode::Invalid => "0x00000000",
            MockRpcMode::Slow => "0x1626ba7e",
            MockRpcMode::HttpFailure
            | MockRpcMode::MalformedResponse
            | MockRpcMode::JsonRpcFailure => unreachable!("故障模式已提前返回"),
        },
        _ => {
            return HttpResponse::Ok().json(json!({
                "jsonrpc": "2.0",
                "id": request.get("id").cloned().unwrap_or(Value::Null),
                "error": {"code": -32601, "message": "Method not found"}
            }));
        }
    };

    HttpResponse::Ok().json(json!({
        "jsonrpc": "2.0",
        "id": request.get("id").cloned().unwrap_or(Value::Null),
        "result": result
    }))
}
