use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use adnl::common::TaggedObject;
use rand::Rng;
use ton_api::ton::rpc::ton_node::DownloadNextBlockFull;
use ton_api::ton::TLObject;

type Result<T> = ::std::result::Result<T, failure::Error>;

const PARALLEL: usize = 10; // try 200

struct Service;

#[async_trait::async_trait]
impl adnl::common::Subscriber for Service {
    async fn try_consume_query(
        &self,
        _object: TLObject,
        _peers: &adnl::common::AdnlPeers,
    ) -> Result<adnl::common::QueryResult> {
        adnl::common::QueryResult::consume_boxed(example_response())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let left_key: [u8; 32] = rand::thread_rng().gen();
    let right_key: [u8; 32] = rand::thread_rng().gen();

    let (_, config) = adnl::node::AdnlNodeConfig::from_ip_address_and_private_keys(
        "127.0.0.1:10000",
        vec![(left_key, 0)],
    )?;
    let left_node = adnl::node::AdnlNode::with_config(config).await?;
    adnl::node::AdnlNode::start(&left_node, vec![]).await?;

    let (_, config) = adnl::node::AdnlNodeConfig::from_ip_address_and_private_keys(
        "127.0.0.1:10001",
        vec![(right_key, 0)],
    )?;
    let right_node = adnl::node::AdnlNode::with_config(config).await?;
    adnl::node::AdnlNode::start(&right_node, vec![Arc::new(Service)]).await?;

    let left_key = left_node.key_by_tag(0)?.id().clone();
    let right_key = right_node.key_by_tag(0)?;

    left_node.add_peer(
        &left_key,
        right_node.ip_address(),
        &right_node.key_by_tag(0)?,
    )?;
    let peers = adnl::common::AdnlPeers::with_keys(left_key, right_key.id().clone());

    let iterations = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..PARALLEL {
        let left_node = left_node.clone();
        let query = TaggedObject {
            object: TLObject::new(example_request()),
        };
        let iterations = iterations.clone();
        let peers = peers.clone();
        handles.push(tokio::spawn(async move {
            let e = loop {
                match left_node.clone().query(&query, &peers, Some(2)).await {
                    Ok(Some(data)) => {
                        if data.downcast::<ton_api::ton::ton_node::DataFull>().is_err() {
                            break failure::err_msg("invalid response");
                        }
                        iterations.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(None) => println!("Packet lost"),
                    Err(e) => break e,
                }
            };
            println!("Error: {e:?}");
        }));
    }

    tokio::select! {
        _ = futures_util::future::join_all(handles) => {},
        _ = tokio::time::sleep(Duration::from_secs(10)) => {},
    }

    left_node.stop().await;
    right_node.stop().await;

    let request_len = ton_api::serialize_boxed(&example_request())?.len();
    let response_len = ton_api::serialize_boxed(&example_response())?.len();
    println!("REQ+RES: {}", request_len + response_len);

    let throughput = (request_len + response_len) * iterations.load(Ordering::Relaxed);
    println!("Total throughput: {} MB/s", throughput as f64 / 10485760.0);

    Ok(())
}

fn example_request() -> DownloadNextBlockFull {
    DownloadNextBlockFull {
        prev_block: Default::default(),
    }
}

fn example_response() -> ton_api::ton::ton_node::DataFull {
    ton_api::ton::ton_node::DataFull::TonNode_DataFull(ton_api::ton::ton_node::datafull::DataFull {
        id: Default::default(),
        proof: ton_api::ton::bytes(vec![1u8; 128]),
        block: ton_api::ton::bytes(vec![1u8; 128]),
        is_link: ton_api::ton::Bool::BoolFalse,
    })
}
