use std::{convert::TryInto, iter::FromIterator};

use ethers_core::types::U256;

use async_trait::async_trait;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    Client, ClientBuilder,
};
use serde::Deserialize;
use url::Url;

use crate::gas_oracle::{GasCategory, GasOracle, GasOracleError, GWEI_TO_WEI};

const BLOCKNATIVE_GAS_PRICE_ENDPOINT: &str = "https://api.blocknative.com/gasprices/blockprices";

/// A client over HTTP for the [BlockNative](https://www.blocknative.com/gas-estimator) gas tracker API
/// that implements the `GasOracle` trait
#[derive(Clone, Debug)]
pub struct BlockNative {
    client: Client,
    url: Url,
    gas_category: GasCategory,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlockNativeGasResponse {
    system: Option<String>,
    network: Option<String>,
    unit: Option<String>,
    max_price: Option<u64>,
    block_prices: Vec<BlockPrice>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct BlockPrice {
    #[serde(rename = "blockNumber")]
    block_number: u64,
    #[serde(rename = "estimatedTransactionCount")]
    estimated_transaction_count: u64,
    #[serde(rename = "baseFeePerGas")]
    base_fee_per_gas: f64,
    #[serde(rename = "estimatedPrices")]
    estimated_prices: Vec<EstimatedPrice>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct EstimatedPrice {
    confidence: u64,
    price: u64,
    #[serde(rename = "maxPriorityFeePerGas")]
    max_priority_fee_per_gas: f64,
    #[serde(rename = "maxFeePerGas")]
    max_fee_per_gas: f64,
}

fn gas_category_to_confidence(gas_category: GasCategory) -> u64 {
    match gas_category {
        GasCategory::SafeLow => 80,
        GasCategory::Standard => 90,
        GasCategory::Fast => 95,
        GasCategory::Fastest => 99,
    }
}

impl BlockNative {
    /// Creates a new [BlockNative](https://www.blocknative.com/gas-estimator) gas oracle
    pub fn new(api_key: &str) -> Self {
        let header_value = HeaderValue::from_str(api_key).unwrap();
        let headers = HeaderMap::from_iter([(AUTHORIZATION, header_value)]);
        let client = ClientBuilder::new().default_headers(headers).build().unwrap();
        Self {
            client,
            url: BLOCKNATIVE_GAS_PRICE_ENDPOINT.try_into().unwrap(),
            gas_category: GasCategory::Standard,
        }
    }

    /// Sets the gas price category to be used when fetching the gas price.
    #[must_use]
    pub fn category(mut self, gas_category: GasCategory) -> Self {
        self.gas_category = gas_category;
        self
    }

    pub async fn query(&self) -> Result<BlockNativeGasResponse, GasOracleError> {
        let resp = self
            .client
            .get(self.url.as_ref())
            .send()
            .await?;
        let text = resp.text().await?;
        match serde_json::from_str(&text) {
            Ok(r) => Ok(r),
            Err(e) => {
                tracing::error!("error from blocknative: {e:?} (resp: {})", text);
                Err(GasOracleError::SerdeJsonError(e.into()))
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl GasOracle for BlockNative {
    async fn fetch(&self) -> Result<U256, GasOracleError> {
        todo!()
        // let mut res = self.query().await?;
        // let confidence = gas_category_to_confidence(self.gas_category);
        // let price = res
        //     .block_prices
        //     .pop()
        //     .unwrap()
        //     .estimated_prices
        //     .into_iter()
        //     .find(|p| p.confidence == confidence)
        //     .unwrap();
        // Ok(U256::from((price.price * GWEI_TO_WEI) / 10))
    }

    async fn estimate_eip1559_fees(&self) -> Result<(U256, U256), GasOracleError> {
        let mut res = self.query().await?;
        let confidence = gas_category_to_confidence(self.gas_category);
        let block_prices = res
            .block_prices
            .pop()
            .unwrap()
            .estimated_prices
            .into_iter()
            .find(|p| p.confidence == confidence)
            .unwrap();
        let base_fee = U256::from((block_prices.max_fee_per_gas * 100.0) as u64) *
            U256::from(GWEI_TO_WEI) /
            U256::from(100);
        let prio_fee = U256::from((block_prices.max_priority_fee_per_gas * 100.0) as u64 as u64) *
            U256::from(GWEI_TO_WEI) /
            U256::from(100);
        Ok((base_fee, prio_fee))
    }
}
