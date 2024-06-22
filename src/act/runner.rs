use log::info;
use anyhow::Result;
use ethers::{
    providers::{Middleware, Provider, Ws},
    types::*,
};

use tokio::time::{sleep, Duration};
use tokio_stream::StreamExt;
use std::{collections::HashMap, str::FromStr, sync::Arc, thread};



use tokio::sync::broadcast::Sender;

use crate::common::def_logger::*;

pub async fn run_loop(provider: Arc<Provider<Ws>>, event_sender: Sender<Event>) {
    info!("entered loop");
    let env = Env::new();

    let block = provider
        .get_block(BlockNumber::Latest)
        .await
        .unwrap()
        .unwrap();

    let mut new_block = NewBlock {
        block_number: block.number.unwrap(),
        base_fee: block.base_fee_per_gas.unwrap(),
        next_base_fee: calculate_next_block_base_fee(
            block.gas_used,
            block.gas_limit,
            block.base_fee_per_gas.unwrap(),
        ),
    };

    info!("Starting block {}", new_block.block_number);

    let mut event_receiver = event_sender.subscribe();
    let mut new_trans : HashMap<H256, NewPendingTx> = HashMap::new();


    loop {
        match event_receiver.recv().await {
            Ok(event) => {
                match event {
                    Event::Block(block) => {
                        info!("NEW block {}", block.block_number);
                        new_block = block;
                        match tran_hashes_by_new_block(&provider, new_block.block_number).await
                        {
                            Some(val) => {

                                info!("pre total: {}", new_trans.len());
                                for h in val {
                                    if new_trans.contains_key(&h) {
                                        new_trans.remove(&h);
                                    }
                                }
                                info!("post total: {}", new_trans.len());

                            }
                            None => { }
                        }

                    }
                    Event::PendingTx(mut pending_tx) => {

                        if (!new_trans.contains_key(&pending_tx.tx.hash)) {
                            if (calculate_victim_gas(&pending_tx.tx) >= new_block.base_fee) {
                                if (!tran_has_receipt(&provider, pending_tx.tx.hash).await) {
                                    if (search_in_logs(&provider, &new_block, &pending_tx).await) {
                                        new_trans.insert(pending_tx.tx.hash, pending_tx);
                                    }
                                }
                            }
                        }

                    }

                }
            },
            Err(e) => {
                info!("recv error {}", e);
            }
        }

        if new_trans.len() % 100 == 0 {
            //info!("more 100 trans recv, total: {}", new_trans.len());
        }
    }
}

pub async fn run_pending_blocks(provider: Arc<Provider<Ws>>, event_sender: Sender<Event>) {
    let stream = provider.subscribe_blocks().await.unwrap();
    let mut stream =
        stream.
            filter_map(|block|
                match block.number {
                    Some(number) => Some(NewBlock {
                        block_number: number,
                        base_fee: block.base_fee_per_gas.unwrap_or_default(),
                        next_base_fee: U256::from(calculate_next_block_base_fee(
                            block.gas_used,
                            block.gas_limit,
                            block.base_fee_per_gas.unwrap_or_default())),
                    }),
                    None => None
                }
            );

    while let Some(block) = stream.next().await {
        sleep(Duration::from_millis(200)).await;

        match event_sender.send(Event::Block(block)) {
            Ok(_) => {}
            Err(_) => {
                info!("error2");
            }
        }
    }

}
pub async fn run_pending_transactions(provider: Arc<Provider<Ws>>, event_sender: Sender<Event>) {

    let stream = provider.subscribe_pending_txs().await.unwrap();
    let mut stream = stream.transactions_unordered(256).fuse();



    while let Some(result) = stream.next().await {
        sleep(Duration::from_millis(180)).await;
        match result {
            Ok(tx) => match event_sender.send(Event::PendingTx(NewPendingTx {
                added_block: None,
                tx,
            }))
            {
                Ok(_) => {}
                Err(_) => {}
            },
            Err(e) => {

                //info!("error1 {:?}", e );
            }
        };
    }
}

pub async fn tran_has_receipt(provider: &Arc<Provider<Ws>>, hash: H256) -> bool {
    match provider.get_transaction_receipt(hash).await {
        Ok(o) => {
            match o {
                Some(v) => true,
                None => false
            }
        }
        Err(e) =>  false
    }
}
pub async fn tran_hashes_by_new_block(provider: &Arc<Provider<Ws>>, number: U64) -> Option<Vec<H256>> {

    match provider.get_block_with_txs(number).await {
        Ok(b) => {
            match b {
                Some(v) => Some(v.transactions.into_iter().map(|i|i.hash).collect::<Vec<H256>>()) ,
                None => None ?
            }
        }
        Err(e) =>  None ?
    }


}

pub async fn debug_trace_call(
    provider: &Arc<Provider<Ws>>,
    new_block: &NewBlock,
    pending_tx: &NewPendingTx,
) -> Result<Option<CallFrame>> {
    let mut opts = GethDebugTracingCallOptions::default();
    let mut call_config = CallConfig::default();
    call_config.with_log = Some(true);

    opts.tracing_options.tracer = Some(GethDebugTracerType::BuiltInTracer(
        GethDebugBuiltInTracerType::CallTracer,
    ));
    opts.tracing_options.tracer_config = Some(GethDebugTracerConfig::BuiltInTracer(
        GethDebugBuiltInTracerConfig::CallTracer(call_config),
    ));

    let block_number = new_block.block_number;
    let mut tx = pending_tx.tx.clone();
    let nonce = provider
        .get_transaction_count(tx.from, Some(block_number.into()))
        .await
        .unwrap_or_default();
    tx.nonce = nonce;

    let trace = provider
        .debug_trace_call(&tx, Some(block_number.into()), opts)
        .await;

    match trace {
        Ok(trace) => match trace {
            GethTrace::Known(call_tracer) => match call_tracer {
                GethTraceFrame::CallTracer(frame) => Ok(Some(frame)),
                _ => Ok(None),
            },
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

pub fn extract_logs(call_frame: &CallFrame, logs: &mut Vec<CallLogFrame>) {
    if let Some(ref logs_vec) = call_frame.logs {
        logs.extend(logs_vec.iter().cloned());
    }

    if let Some(ref calls_vec) = call_frame.calls {
        for call in calls_vec {
            extract_logs(call, logs);
        }
    }
}

pub fn calculate_victim_gas(tran : &Transaction) -> U256 {

    match tran.transaction_type {
        Some(tx_type) => {
            if (tx_type == U64::zero()) {
                tran.gas_price.unwrap_or_default()
            } else if (tx_type == U64::from(2)) {
                tran.max_fee_per_gas.unwrap_or_default()
            } else {
                U256::zero()
            }
         }
        None => { U256::zero() }
    }

}

pub async fn search_in_logs(provider: &Arc<Provider<Ws>>,
                      new_block: &NewBlock,
                      pending_tx: &NewPendingTx) -> bool {
    let frame = debug_trace_call(&provider, &new_block, &pending_tx).await;
    if (frame.is_ok())
    {
        let mut callFrame = frame.unwrap();
        if (callFrame.is_some()) {
            let frm = callFrame.unwrap();
            let mut logs = Vec::new();
            extract_logs(&frm, &mut logs);

            for a in &logs {
                match &a.topics {
                    Some(v) => {
                        if (v.len() > 1) {

                            let selector = &format!("{:?}", v[0])[0..10];
                            let is_v2_swap = selector == "0xd78ad95f" || selector == "0xc42079f9";
                            //info!("comparing: {:?} : {:?}", pending_tx.tx.hash, logs);

                            if (is_v2_swap) {
                                info!("tran: {:?} : {:?}", pending_tx.tx.hash, logs);
                                return true;
                            }

                        }
                    }
                    None => {}
                }
            }

        }
    }
    return false;
}

