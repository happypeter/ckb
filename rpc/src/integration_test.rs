use bigint::H256;
use chain::chain::ChainProvider;
use ckb_pow::Clicker;
use core::header::{BlockNumber, Header};
use core::transaction::{OutPoint, Transaction};
use jsonrpc_core::{Error, IoHandler, Result};
use jsonrpc_http_server::ServerBuilder;
use jsonrpc_server_utils::cors::AccessControlAllowOrigin;
use jsonrpc_server_utils::hosts::DomainsValidation;
use miner::{build_block_template, BlockTemplate};
use network::NetworkService;
use pool::TransactionPool;
use std::sync::Arc;
use {BlockWithHash, CellOutputWithOutPoint, Config, TransactionWithHash};

//TODO: build_rpc_trait! do not surppot trait bounds
build_rpc_trait! {
    pub trait IntegrationTestRpc {
        // curl -d '{"id": 2, "jsonrpc": "2.0", "method":"submit_solution","params": [1]}' -H 'content-type:application/json' 'http://localhost:3030'
        #[rpc(name = "submit_pow_solution")]
        fn submit_pow_solution(&self, u64) -> Result<()>;

        // curl -d '{"id": 2, "jsonrpc": "2.0", "method":"send_transaction","params": [{"version":2, "deps":[], "inputs":[], "outputs":[]}]}' -H 'content-type:application/json' 'http://localhost:3030'
        #[rpc(name = "send_transaction")]
        fn send_transaction(&self, Transaction) -> Result<H256>;

        // curl -d '{"id": 2, "jsonrpc": "2.0", "method":"get_block","params": ["0x0f9da6db98d0acd1ae0cf7ae3ee0b2b5ad2855d93c18d27c0961f985a62a93c3"]}' -H 'content-type:application/json' 'http://localhost:3030'
        #[rpc(name = "get_block")]
        fn get_block(&self, H256) -> Result<Option<BlockWithHash>>;

        // curl -d '{"id": 2, "jsonrpc": "2.0", "method":"get_transaction","params": ["0x0f9da6db98d0acd1ae0cf7ae3ee0b2b5ad2855d93c18d27c0961f985a62a93c3"]}' -H 'content-type:application/json' 'http://localhost:3030'
        #[rpc(name = "get_transaction")]
        fn get_transaction(&self, H256) -> Result<Option<TransactionWithHash>>;

        // curl -d '{"id": 2, "jsonrpc": "2.0", "method":"get_block_hash","params": [1]}' -H 'content-type:application/json' 'http://localhost:3030'
        #[rpc(name = "get_block_hash")]
        fn get_block_hash(&self, u64) -> Result<Option<H256>>;

        // curl -d '{"id": 2, "jsonrpc": "2.0", "method":"get_tip_header","params": []}' -H 'content-type:application/json' 'http://localhost:3030'
        #[rpc(name = "get_tip_header")]
        fn get_tip_header(&self) -> Result<Header>;

        // curl -d '{"id": 2, "jsonrpc": "2.0", "method":"get_block_template","params": []}' -H 'content-type:application/json' 'http://localhost:3030'
        #[rpc(name = "get_block_template")]
        fn get_block_template(&self) -> Result<BlockTemplate>;

        // curl -d '{"id": 2, "jsonrpc": "2.0", "method":"get_cells_by_type_hash","params": ["0x1b1c832d02fdb4339f9868c8a8636c3d9dd10bd53ac7ce99595825bd6beeffb3", 1, 10]}' -H 'content-type:application/json' 'http://localhost:3030'
        #[rpc(name = "get_cells_by_type_hash")]
        fn get_cells_by_type_hash(&self, H256, u64, u64) -> Result<Vec<CellOutputWithOutPoint>>;

        #[rpc(name = "local_node_id")]
        fn local_node_id(&self) -> Result<Option<String>>;

        #[rpc(name = "add_node")]
        fn add_node(&self, String) -> Result<()>;
    }
}

struct RpcImpl<C> {
    pub network: Arc<NetworkService>,
    pub chain: Arc<C>,
    pub tx_pool: Arc<TransactionPool<C>>,
    pub pow: Arc<Clicker>,
}

impl<C: ChainProvider + 'static> IntegrationTestRpc for RpcImpl<C> {
    fn submit_pow_solution(&self, nonce: u64) -> Result<()> {
        self.pow.submit(nonce);
        Ok(())
    }

    fn send_transaction(&self, tx: Transaction) -> Result<H256> {
        let indexed_tx: Transaction = tx.into();
        let result = indexed_tx.hash();
        let pool_result = self.tx_pool.add_transaction(indexed_tx.clone());
        debug!(target: "rpc", "send_transaction add to pool result: {:?}", pool_result);

        // TODO PENDING new api NetworkContext#connected_peers
        // for peer_id in self.nc.connected_peers() {
        //     let data = builde_transaction(indexed_tx);
        //     self.nc.send(peer_id, 0, data.to_vec());
        // }
        Ok(result)
    }

    fn get_block(&self, hash: H256) -> Result<Option<BlockWithHash>> {
        Ok(self.chain.block(&hash).map(Into::into))
    }

    fn get_transaction(&self, hash: H256) -> Result<Option<TransactionWithHash>> {
        Ok(self.chain.get_transaction(&hash).map(Into::into))
    }

    fn get_block_hash(&self, number: BlockNumber) -> Result<Option<H256>> {
        Ok(self.chain.block_hash(number))
    }

    fn get_tip_header(&self) -> Result<Header> {
        Ok(self.chain.tip_header().read().inner().clone())
    }

    fn get_block_template(&self) -> Result<BlockTemplate> {
        Ok(build_block_template(&self.chain, &self.tx_pool, H256::from(0), 20000, 20000).unwrap())
    }

    fn get_cells_by_type_hash(
        &self,
        type_hash: H256,
        from: u64,
        to: u64,
    ) -> Result<Vec<CellOutputWithOutPoint>> {
        let mut result = Vec::new();
        for block_number in from..=to {
            if let Some(block_hash) = self.chain.block_hash(block_number) {
                let block = self
                    .chain
                    .block(&block_hash)
                    .ok_or_else(Error::internal_error)?;
                for transaction in block.commit_transactions() {
                    let transaction_meta = self
                        .chain
                        .get_transaction_meta(&transaction.hash())
                        .ok_or_else(Error::internal_error)?;
                    for (i, output) in transaction.outputs().iter().enumerate() {
                        if output.lock == type_hash && (!transaction_meta.is_spent(i)) {
                            result.push(CellOutputWithOutPoint {
                                outpoint: OutPoint::new(transaction.hash(), i as u32),
                                capacity: output.capacity,
                                lock: output.lock,
                            });
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    fn local_node_id(&self) -> Result<Option<String>> {
        Ok(self.network.external_url())
    }

    fn add_node(&self, _node_id: String) -> Result<()> {
        unimplemented!()
    }
}

pub struct RpcServer {
    pub config: Config,
}

impl RpcServer {
    pub fn start<C>(
        &self,
        network: Arc<NetworkService>,
        chain: Arc<C>,
        tx_pool: Arc<TransactionPool<C>>,
        pow: Arc<Clicker>,
    ) where
        C: ChainProvider + 'static,
    {
        let mut io = IoHandler::new();
        io.extend_with(
            RpcImpl {
                network,
                chain,
                tx_pool,
                pow,
            }.to_delegate(),
        );

        let server = ServerBuilder::new(io)
            .cors(DomainsValidation::AllowOnly(vec![
                AccessControlAllowOrigin::Null,
                AccessControlAllowOrigin::Any,
            ])).start_http(&self.config.listen_addr.parse().unwrap())
            .unwrap();

        info!(target: "rpc", "Now listening on {:?}", server.address());
        server.wait();
    }
}