use anyhow::Result;

use ethers::prelude::*;
use ethers::{
    self,
    types::{
        transaction::eip2930::{AccessList, AccessListItem},
        U256,
    },
};
use ethers::prelude::rand::Rng;
use fern::colors::{Color, ColoredLevelConfig};
use log::LevelFilter;


pub static PROJECT_NAME: &str = "def";

#[derive(Debug, Clone)]
pub struct Env {
    pub https_url: String,
    pub wss_url: String,
    pub bot_address: String,
    pub private_key: String,
    pub identity_key: String,
    pub telegram_token: String,
    pub telegram_chat_id: String,
    pub use_alert: bool,
    pub debug: bool,
}

impl Env {
    pub fn new() -> Self {
        Env {
            https_url: get_env("HTTPS_URL"),
            wss_url: get_env("WSS_URL"),
            bot_address: get_env("BOT_ADDRESS"),
            private_key: get_env("PRIVATE_KEY"),
            identity_key: get_env("IDENTITY_KEY"),
            telegram_token: get_env("TELEGRAM_TOKEN"),
            telegram_chat_id: get_env("TELEGRAM_CHAT_ID"),
            use_alert: get_env("USE_ALERT").parse::<bool>().unwrap(),
            debug: get_env("DEBUG").parse::<bool>().unwrap(),
        }
    }
}

pub fn get_env(key: &str) -> String {
    std::env::var(key).unwrap_or(String::from(""))
}


#[derive(Default, Debug, Clone)]
pub struct NewBlock {
    pub block_number: U64,
    pub base_fee: U256,
    pub next_base_fee: U256,
}

#[derive(Debug, Clone)]
pub struct NewPendingTx {
    pub added_block: Option<U64>,
    pub tx: Transaction,
}

impl Default for NewPendingTx {
    fn default() -> Self {
        Self {
            added_block: None,
            tx: Transaction::default(),
        }
    }
}
#[derive(Debug, Clone)]
pub enum Event {
    Block(NewBlock),
    PendingTx(NewPendingTx),
}



pub fn setup_logger() -> Result<()> {

    let colors = ColoredLevelConfig {
        trace: Color::Cyan,
        debug: Color::Magenta,
        info: Color::Green,
        warn: Color::Red,
        error: Color::BrightRed,
        ..ColoredLevelConfig::new()
    };

    fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{}[{}] {}",
                chrono::Local::now().format("[%H:%M:%S]"),
                colors.color(record.level()),
                message
            ))
        })
        .chain(std::io::stdout())
        .level(log::LevelFilter::Error)
        .level_for(PROJECT_NAME, LevelFilter::Info)
        .apply()?;

    Ok(())
}

pub fn calculate_next_block_base_fee(
    gas_used: U256,
    gas_limit: U256,
    base_fee_per_gas: U256,
) -> U256 {
    let gas_used = gas_used;

    let mut target_gas_used = gas_limit / 2;
    target_gas_used = if target_gas_used == U256::zero() {
        U256::one()
    } else {
        target_gas_used
    };

    let new_base_fee = {
        if gas_used > target_gas_used {
            base_fee_per_gas
                + ((base_fee_per_gas * (gas_used - target_gas_used)) / target_gas_used)
                / U256::from(8u64)
        } else {
            base_fee_per_gas
                - ((base_fee_per_gas * (target_gas_used - gas_used)) / target_gas_used)
                / U256::from(8u64)
        }
    };

    let seed = rand::thread_rng().gen_range(0..9);
    new_base_fee + seed
}