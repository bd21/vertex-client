use serde::{de, Deserialize, Deserializer};
use std::collections::BTreeMap;

/// Internal

//
#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum StreamResponseType {
    BookDepth(BookDepthResponse),
    SubscriptionResponse(SubscriptionResponse)
    // ...register more stream response models here
}

/// Vertex

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SubscriptionResponse {
    pub result: Option<serde_json::Value>,
    pub id: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct BookDepthResponse {
    pub r#type: String, // `type` is a reserved keyword in Rust
    pub min_timestamp: String,
    pub max_timestamp: String,
    pub last_max_timestamp: String,
    pub product_id: u32,
    #[serde(deserialize_with = "deserialize_bid_ask")]
    pub bids: Vec<(u128, u128)>, // (bid price, quantity)
    #[serde(deserialize_with = "deserialize_bid_ask")]
    pub asks: Vec<(u128, u128)>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MarketLiquidityResponse {
    pub status: String,
    pub data: MarketLiquidityData,
    pub request_type: String,
}

#[derive(Debug, Deserialize)]
pub struct MarketLiquidityData {
    #[serde(deserialize_with = "deserialize_bid_ask")]
    pub bids: Vec<(u128, u128)>,
    #[serde(deserialize_with = "deserialize_bid_ask")]
    pub asks: Vec<(u128, u128)>,
    pub timestamp: String,
}

fn deserialize_bid_ask<'de, D>(deserializer: D) -> Result<Vec<(u128, u128)>, D::Error>
where
    D: Deserializer<'de>,
{
    // Parse as a vector of string tuples
    let vec: Vec<(String, String)> = Deserialize::deserialize(deserializer)?;

    // Convert each string tuple into a tuple of u128
    vec.into_iter()
        .map(|(price, quantity)| {
            let price = price.parse::<u128>().map_err(de::Error::custom)?;
            let quantity = quantity.parse::<u128>().map_err(de::Error::custom)?;
            Ok((price, quantity))
        })
        .collect()
}

#[derive(Debug)]
pub struct OrderBook {
    bids: BTreeMap<u128, u128>, // Price -> Quantity
    asks: BTreeMap<u128, u128>,
}

impl OrderBook {
    pub fn new() -> Self {
        OrderBook {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    pub fn from_snapshot(&mut self, snapshot: MarketLiquidityResponse) {
        self.bids.clear();
        self.asks.clear();

        for (price, quantity) in snapshot.data.bids {
            if quantity == 0 {
                self.bids.remove(&price);
            } else {
                self.bids.insert(price, quantity);
            }
        }

        for (price, quantity) in snapshot.data.asks {
            if quantity == 0 {
                self.asks.remove(&price);
            } else {
                self.asks.insert(price, quantity);
            }
        }

        self.validate_orderbook();
    }

    pub fn update(&mut self, book_depth: BookDepthResponse) {
        // Update bids
        for (price, quantity) in book_depth.bids {
            if quantity == 0 {
                self.bids.remove(&price);
            } else {
                self.bids.insert(price, quantity);
            }
        }

        // Update asks
        for (price, quantity) in book_depth.asks {
            if quantity == 0 {
                self.asks.remove(&price);
            } else {
                self.asks.insert(price, quantity);
            }
        }

        self.validate_orderbook();
    }

    fn validate_orderbook(&mut self) {
        // Check that all bids are less than asks
        if let (Some(highest_bid), Some(lowest_ask)) = (self.bids.iter().next_back(), self.asks.iter().next()) {
            assert!(
                highest_bid.0 < lowest_ask.0,
                "Bid-Ask Spread Violation: Highest bid ({}) >= Lowest ask ({})",
                highest_bid.0,
                lowest_ask.0
            );
        }

        // Check that all quantities are > 0
        for (price, quantity) in self.bids.iter().chain(self.asks.iter()) {
            assert!(
                *quantity > 0,
                "Quantity Zero Violation: Price {} has zero quantity",
                price
            );
        }

        // Check that bids > 0
        if let Some((price, _)) = self.bids.iter().next() {
            assert!(
                *price > 0,
                "Invalid Bid Price: Bid price must be greater than 0"
            );
        }

        // Check that asks < ∞ .  Price bounds might be more appropriate here.
        if let Some((price, _)) = self.asks.iter().next_back() {
            assert!(
                *price < u128::MAX,
                "Invalid Ask Price: Ask price must be less than infinity (u128::MAX)"
            );
        }
    }
    pub fn visualize(&self) -> String {
        let mut output = String::new();
        output.push_str("\x1B[2J\x1B[H"); // Clear screen and reset cursor to top-left

        // Calculate the market price (midpoint)
        let best_bid = self.bids.iter().next_back(); // Highest bid
        let best_ask = self.asks.iter().next();     // Lowest ask
        let market_price = match (best_bid, best_ask) {
            (Some((bid_price, _)), Some((ask_price, _))) => {
                let bid_scaled = *bid_price as f64 / 1_000_000_000_000_000_000.0;
                let ask_scaled = *ask_price as f64 / 1_000_000_000_000_000_000.0;
                Some((bid_scaled + ask_scaled) / 2.0)
            }
            _ => None,
        };

        // Display the market price
        output.push_str("Order Book\n");
        output.push_str("=================\n");
        match market_price {
            Some(price) => output.push_str(&format!("Market Price: {:.2}\n\n", price)),
            None => output.push_str("Market Price: N/A\n\n"),
        }

        // Add headers for asks and bids
        output.push_str(format!("{:<30} {:>30}\n", "Asks (Price -> Quantity)", "Bids (Price -> Quantity)").as_str());
        output.push_str(format!("{:=<60}\n", "").as_str()); // Separator

        let mut asks_iter = self.asks.iter();
        let mut bids_iter = self.bids.iter().rev();

        loop {
            let ask = asks_iter.next();
            let bid = bids_iter.next();

            match (ask, bid) {
                (Some((ask_price, ask_quantity)), Some((bid_price, bid_quantity))) => {
                    let ask_price_scaled = *ask_price / 1_000_000_000_000_000_000; // Convert to dollars
                    let ask_quantity_scaled = *ask_quantity as f64 / 1e18;         // Convert to units

                    let bid_price_scaled = *bid_price / 1_000_000_000_000_000_000; // Convert to dollars
                    let bid_quantity_scaled = *bid_quantity as f64 / 1e18;         // Convert to units

                    output.push_str(&format!(
                        "{:<15.2} -> {:<15.10} {:>15.2} -> {:>15.10}\n",
                        ask_price_scaled, ask_quantity_scaled, bid_price_scaled, bid_quantity_scaled
                    ));
                }
                (Some((ask_price, ask_quantity)), None) => {
                    let ask_price_scaled = *ask_price / 1_000_000_000_000_000_000; // Convert to dollars
                    let ask_quantity_scaled = *ask_quantity as f64 / 1e18;         // Convert to units

                    output.push_str(&format!(
                        "{:<15.2} -> {:<15.10} {:>30}\n",
                        ask_price_scaled, ask_quantity_scaled, ""
                    ));
                }
                (None, Some((bid_price, bid_quantity))) => {
                    let bid_price_scaled = *bid_price / 1_000_000_000_000_000_000; // Convert to dollars
                    let bid_quantity_scaled = *bid_quantity as f64 / 1e18;         // Convert to units

                    output.push_str(&format!(
                        "{:<30} {:>15.2} -> {:>15.10}\n",
                        "", bid_price_scaled, bid_quantity_scaled
                    ));
                }
                (None, None) => break,
            }
        }

        output
    }




}

