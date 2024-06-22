

use anyhow::Result;
use ethers::providers::{Provider, Ws};
use log::info;
use std::sync::Arc;
use itertools::Itertools;
use tokio::sync::broadcast::{self, Sender};
use tokio::task::JoinSet;
use def::common::def_logger::*;
use def::act::runner::*;


#[tokio::main]
async fn main() -> Result<()> {




    dotenv::dotenv().ok();
    setup_logger().unwrap();

    info!("Starting def");

    let env = Env::new();

    let ws = Ws::connect(env.wss_url).await.unwrap();

    let provider = Arc::new(Provider::new(ws));

    let (event_sender, _): (Sender<Event>, _) = broadcast::channel(512);

    let mut set = JoinSet::new();

    set.spawn(run_pending_blocks(
        provider.clone(),
        event_sender.clone(),
    ));

    set.spawn(run_pending_transactions(
        provider.clone(),
        event_sender.clone(),
    ));

    set.spawn(run_loop(
        provider.clone(),
        event_sender.clone(),
    ));

    while let Some(res) = set.join_next().await {
        info!("{:?}", res);
    }


    Ok(())

}
