#![allow(non_snake_case)]

mod model;
mod listener;

use serde_json::json;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use listener::Subscribe;
use model::StreamResponseType;
use crate::listener::QueryMarketLiquidity;
use crate::model::{MarketLiquidityResponse, OrderBook};

const SUBSCRIPTION_URL: &str = "wss://gateway.prod.vertexprotocol.com/v1/subscribe";
const GATEWAY_URL: &str = "wss://gateway.prod.vertexprotocol.com/v1/ws";
const PRODUCT_ID: usize = 2; // BTC-USDC perp
const BOOK_DEPTH_STREAM_BUFFER_SIZE: usize = 1000000; // 1MM
const MARKET_LIQ_QUERY_DEPTH: usize = 10; // how deep to fill the order book up from snapshot (max 100)
const PING_FRAME_INTERVAL: u64 = 5; // how often to send ping frames to keep the ws connection alive (max 30)

#[tokio::main]
async fn main() {

    // listen to the book_depth stream
    let (sender, receiver) =
        mpsc::channel::<StreamResponseType>(BOOK_DEPTH_STREAM_BUFFER_SIZE);
    tokio::spawn(async move { Subscribe(sender, &book_depth(), &SUBSCRIPTION_URL).await; });

    // build + display order book
    build_orderbook(receiver).await;

}

async fn build_orderbook(mut receiver: Receiver<StreamResponseType>) {
    // From the docs: https://docs.vertexprotocol.com/developer-resources/api/subscriptions/events#book-depth
    //
    // To keep an updated local orderbook, do the following:
    // 1. Subscribe to the book_depth stream and queue up events.
    // 2. Get a market data snapshot by calling MarketLiquidity. The snapshot contains a timestamp in the response
    // 3. Apply events with max_timestamp > snapshot timestamp.
    // 4. When you receive an event where its last_max_timestamp is not equal to the last event you've received,
    //    it means some events were lost and you should repeat 1-3 again.

    let mut order_book = OrderBook::new();

    // snapshot_timestamp is used to track if we missed events
    let snapshot = query_market_liquidity().await;
    let mut snapshot_timestamp: u128 = snapshot.data.timestamp.parse().expect("expected u128");
    let mut prev_timestamp = None;

    // populate the order book
    order_book.from_snapshot(snapshot);

    while let Some(event) = receiver.recv().await {
        match event {
            StreamResponseType::BookDepth(data) => {
                let last_max_timestamp: u128 = data.last_max_timestamp.parse().expect("last max timestamp");
                let max_timestamp: u128 = data.max_timestamp.parse().expect("max timestamp");

                if last_max_timestamp <= snapshot_timestamp {
                    continue // drop msgs from before the snapshot
                }

                if prev_timestamp.is_none() || prev_timestamp == Some(last_max_timestamp) {
                    prev_timestamp = Some(max_timestamp);
                    order_book.update(data);
                    print!("{}", order_book.visualize());
                } else {
                    println!("dropped a book depth update, retrieving snapshot...");
                    // populate from the snapshot response
                    let snapshot = query_market_liquidity().await;
                    snapshot_timestamp = snapshot.data.timestamp.parse().expect("snapshot timestamp");
                    order_book.from_snapshot(snapshot);

                }
            }
            _ => {}
        }

    }

}


fn book_depth() -> String {
    json!({
        "method": "subscribe",
        "stream": {
           "type": "book_depth",
           "product_id": PRODUCT_ID
        },
        "id": 0
    })
        .to_string()
}

async fn query_market_liquidity() -> MarketLiquidityResponse {
    let market_liquidity_request = json!({
      "type": "market_liquidity",
      "product_id": PRODUCT_ID,
      "depth": MARKET_LIQ_QUERY_DEPTH
    })
    .to_string();

    QueryMarketLiquidity(&market_liquidity_request, GATEWAY_URL).await
}
