use std::str::FromStr;
use ethers_core::types::transaction::eip712::{EIP712Domain, Eip712};
use ethers::prelude::{LocalWallet, U256};
use ethers::types::H256;
use std::time::{SystemTime, UNIX_EPOCH};
use ethers::addressbook::Address;
use ethers::prelude::rand::thread_rng;
use ethers_core::utils::keccak256;
use ethers_signers::Signer;
use futures_util::{SinkExt, StreamExt};
use hex::encode;
use serde_json::json;
use tokio::select;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::{
    connect_async_with_config, tungstenite::extensions::DeflateConfig,
    tungstenite::protocol::WebSocketConfig, tungstenite::Message,
};
use vertex_sdk::eip712_structs::StreamAuthentication;
use crate::model::{MarketLiquidityResponse, StreamResponseType};
use crate::PING_FRAME_INTERVAL;

// Subscribe to a websocket stream
pub async fn Subscribe(
    sender: Sender<StreamResponseType>,
    message: &str,
    url: &str,
) {
    loop {
        let connection = connect_async_with_config(
            url,
            Some(WebSocketConfig {
                compression: Some(DeflateConfig::default()),
                ..WebSocketConfig::default()
            }),
        )
            .await;

        if let Err(e) = connection {
            println!("Failed to connect: {}", e);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            continue;
        }

        let (mut ws, _) = connection.unwrap();

        if let Err(e) = ws.send(Message::Text(message.into())).await {
            println!("Failed to send message: {}", e);
            break;
        }

        let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(PING_FRAME_INTERVAL));
        loop {
            select! {
                _ = ping_interval.tick() => {
                    if let Err(e) = ws.send(Message::Ping(vec![])).await {
                        println!("Failed to send ping: {}. Reconnecting...", e);
                        break;
                    }
                }
                message = ws.next() => {
                    match message {
                        Some(Ok(msg)) => {
                            if msg.is_text() {
                                match msg.into_text() {
                                    Ok(text) => {
                                        match serde_json::from_str::<StreamResponseType>(&text) {
                                            Ok(resp) => {
                                                if sender.send(resp).await.is_err() {
                                                    println!("Receiver dropped");
                                                    break;
                                                }
                                            }
                                            Err(e) => {
                                                println!("Unable to parse message: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        println!("Failed to convert message to text: {}", e);
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            println!("WebSocket error: {}. Reconnecting...", e);
                            break;
                        }
                        None => {
                            println!("WebSocket closed. Reconnecting...");
                            break;
                        }
                    }
                }
            }
        }
    }
}


// TODO improvement - keep the client live so the connection doesn't have to be reestablished every query
pub async fn QueryMarketLiquidity(
    message: &str,
    url: &str,
) -> MarketLiquidityResponse {
    loop {
        let connection = connect_async_with_config(
            url,
            Some(WebSocketConfig {
                compression: Some(DeflateConfig::default()),
                ..WebSocketConfig::default()
            }),
        )
            .await;

        let (mut ws, _) = match connection {
            Ok(conn) => conn,
            Err(e) => {
                println!("Failed to connect: {}", e);
                continue;
            }
        };

        if let Err(e) = ws.send(Message::Text(message.into())).await {
            println!("Failed to send message: {}.  Retrying...", e);
            continue;
        }

        match ws.next().await {
            Some(Ok(msg)) => {
                if msg.is_text() {
                    match msg.into_text() {
                        Ok(text) => {
                            match serde_json::from_str::<MarketLiquidityResponse>(&text) {
                                Ok(resp) => return resp,
                                Err(e) => {
                                    println!("Failed to parse response: {}.  Retrying...", e);
                                }
                            }
                        }
                        Err(e) => {
                            println!("Failed to convert message to text: {}.  Retrying...", e);
                        }
                    }
                } else {
                    println!("Non-text message received");
                }
            }
            Some(Err(e)) => {
                println!("Error receiving message: {}.  Retrying...", e);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            None => {
                println!("Connection closed by the server.  Retrying...");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }

        println!("Retrying...");
    }
}



fn _get_expiration() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis()
        + (1 * 1000) // 1s
}


fn _authenticate_message(sender: &str, expiration: u64, signature: &str) -> String {
    json!({
      "method": "authenticate",
      "id": 0,
      "tx": {
        "sender": format!("0x{}", sender),
        "expiration": expiration.to_string()
      },
      "signature": signature.to_string()
    })
        .to_string()
}


/// Unused but kept here for future work on authenticated streams
fn _generate_wallet() -> (String, Address) {
    let wallet = LocalWallet::new(&mut thread_rng());
    let private_key = encode(wallet.signer().to_bytes()); // hex
    let address = wallet.address();

    (format!("0x{}", private_key), address)
}


async fn _authenticate(url: String) {
    let connection = connect_async_with_config(
        url,
        Some(WebSocketConfig {
            compression: Some(DeflateConfig::default()),
            ..WebSocketConfig::default()
        }),
    )
        .await;

    if let Err(e) = connection {
        println!("failed to connect: {}", e);
        return;
    }

    let (private_key, address) = _generate_wallet();
    let address_hex = format!("{:#x}", address);
    let subaccount = "64656661756c740000000000"; // "default"

    println!("Generated Private Key: {}", private_key);
    println!("Generated Address: {}", address_hex);
    println!("Subaccount: {}", subaccount);

    // Concatenate the address and subaccount
    let sender_hex = format!("{}{}", &address_hex[2..], subaccount); // Remove "0x" from address

    // Decode the concatenated hex string into bytes
    let sender_bytes = hex::decode(sender_hex.clone()).expect("Invalid concatenated hex string");

    // Convert the decoded bytes into a fixed-size array [u8; 32]
    let sender_bz: [u8; 32] = sender_bytes
        .try_into()
        .expect("Sender must be exactly 32 bytes");


    let expiration = _get_expiration() as u64;
    let stream_auth = StreamAuthentication {
        sender: sender_bz,
        expiration,
    };

    let signature = _generate_eip712_signature(stream_auth, private_key.as_str());
    let (mut ws, _) = connection.unwrap();
    let sub = _authenticate_message(
        sender_hex.as_str(),
        expiration,
        signature.as_str(),
    );
    println!("{:?}", sub);
    if let Err(e) = ws.send(Message::Text(sub.into())).await {
        println!("Failed to send message: {}", e);
        return;
    }

    loop {
        select! {
            message = ws.next() => {
                match message {
                    Some(Ok(msg)) => {
                        if msg.is_text() {
                            let text = msg.into_text().unwrap();
                            println!("{text}");
                        }
                    }
                    _ => {
                        println!("WebSocket closed");
                        break;
                    }
                }
            }
        }
    }
}

fn _generate_eip712_signature(stream_authentication: StreamAuthentication, private_key: &str) -> String {
    let domain = EIP712Domain {
        name: Some("Vertex".to_string()),
        version: Some("0.0.1".to_string()),
        chain_id: Some(U256::from(42161)),
        verifying_contract: Some(Address::from_str("0xbbEE07B3e8121227AfCFe1E2B82772246226128e")
            .expect("Invalid address")),
        salt: None,
    };

    let domain_separator = domain.separator();
    let struct_hash = stream_authentication.struct_hash().unwrap();
    let digest_input = [&[0x19, 0x01], &domain_separator[..], &struct_hash[..]].concat();
    let digest_hash = H256::from(keccak256(digest_input));

    // sign hash
    let wallet: LocalWallet = private_key.parse().expect("Invalid private key");
    let signature = wallet.sign_hash(digest_hash).expect("Failed to sign hash");
    format!("0x{}", signature)
}