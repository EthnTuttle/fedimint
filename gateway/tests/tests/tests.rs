mod fixtures;

use std::future::Future;
use std::net::SocketAddr;

use anyhow::Result;
use fixtures::{fixtures, Fixtures};
use ln_gateway::rpc::rpc_client::{Error, Response};
use ln_gateway::{
    config::GatewayConfig,
    rpc::{
        rpc_client::RpcClient, BalancePayload, DepositAddressPayload, DepositPayload,
        RegisterFedPayload, WithdrawPayload,
    },
};
use mint_client::api::WsFederationConnect;
use mint_client::FederationId;
use url::Url;

#[tokio::test(flavor = "multi_thread")]
async fn test_gateway_authentication() -> Result<()> {
    let gw_password = "password".to_string();
    let gw_port = portpicker::pick_unused_port().expect("Failed to pick port");
    let gw_bind_address = SocketAddr::from(([127, 0, 0, 1], gw_port));
    let gw_announce_address =
        Url::parse(&format!("http://{}", gw_bind_address)).expect("Invalid gateway address");
    let federation_id = FederationId("test_fed".into());

    let cfg = GatewayConfig {
        password: gw_password.clone(),
        default_federation: federation_id.clone(),
        bind_address: gw_bind_address,
        announce_address: gw_announce_address.clone(),
    };

    let Fixtures {
        bitcoin,
        gateway,
        mut task_group,
    } = fixtures(cfg).await?;

    // Run gateway in an isolate thread, so we dont block the test thread
    task_group
        .spawn("Run Gateway", move |_| async move {
            if gateway.run().await.is_err() {}
        })
        .await;

    // Create an RPC client
    let client = RpcClient::new(gw_announce_address);
    let client_ref = &client;

    // Test gateway authentication on `register_federation` function
    // *  `register_federation` with correct password succeeds
    // *  `register_federation` with incorrect password fails
    let payload = RegisterFedPayload {
        connect: serde_json::to_string(&WsFederationConnect { members: vec![] })?,
    };
    test_auth(&gw_password, move |pw| {
        client_ref.register_federation(pw, payload.clone())
    })
    .await?;

    // Test gateway authentication on `get_info` function
    // *  `get_info` with correct password succeeds
    // *  `get_info` with incorrect password fails
    test_auth(&gw_password, |pw| client_ref.get_info(pw)).await?;

    // Test gateway authentication on `get_balance` function
    // *  `get_balance` with correct password succeeds
    // *  `get_balance` with incorrect password fails
    let payload = BalancePayload {
        federation_id: federation_id.clone(),
    };
    test_auth(&gw_password, move |pw| {
        client_ref.get_balance(pw, payload.clone())
    })
    .await?;

    // Test gateway authentication on `get_deposit_address` function
    // *  `get_deposit_address` with correct password succeeds
    // *  `get_deposit_address` with incorrect password fails
    let payload = DepositAddressPayload {
        federation_id: federation_id.clone(),
    };
    test_auth(&gw_password, move |pw| {
        client_ref.get_deposit_address(pw, payload.clone())
    })
    .await?;

    // Test gateway authentication on `deposit` function
    // *  `deposit` with correct password succeeds
    // *  `deposit` with incorrect password fails
    let (proof, tx) = bitcoin.send_and_mine_block(
        &bitcoin.get_new_address(),
        bitcoin::Amount::from_btc(1.0).unwrap(),
    );
    let payload = DepositPayload {
        federation_id: federation_id.clone(),
        txout_proof: proof,
        transaction: tx,
    };
    test_auth(&gw_password, move |pw| {
        client_ref.deposit(pw, payload.clone())
    })
    .await?;

    // Test gateway authentication on `withdraw` function
    // *  `withdraw` with correct password succeeds
    // *  `withdraw` with incorrect password fails
    let payload = WithdrawPayload {
        federation_id,
        amount: bitcoin::Amount::from_sat(100),
        address: bitcoin.get_new_address(),
    };
    test_auth(&gw_password, |pw| client_ref.withdraw(pw, payload.clone())).await?;

    task_group.shutdown_join_all().await
}

async fn test_auth<Fut>(gw_password: &str, func: impl Fn(String) -> Fut) -> Result<()>
where
    Fut: Future<Output = Result<Response, Error>>,
{
    assert_eq!(
        // use random password here
        func("foobar123456789".to_string()).await?.status(),
        401
    );
    assert_ne!(func(gw_password.to_string()).await?.status(), 401);

    Ok(())
}