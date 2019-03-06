//! Blockchain definition.

//
// Copyright (c) 2018 Stegos AG
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use crate::block::*;
use crate::config::*;
use crate::error::*;
use crate::escrow::*;
use crate::merkle::*;
use crate::metrics;
use crate::output::*;
use crate::storage::ListDb;
use chrono::Utc;
use failure::Error;
use log::*;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::time::Instant;
use stegos_crypto::bulletproofs::fee_a;
use stegos_crypto::curve1174::cpt::Pt;
use stegos_crypto::curve1174::ecpt::ECp;
use stegos_crypto::curve1174::fields::Fr;
use stegos_crypto::curve1174::G;
use stegos_crypto::hash::*;
use stegos_crypto::pbc::secure;
use tokio_timer::clock;

type BlockId = usize;

/// A help to find UTXO in this blockchain.
struct OutputKey {
    /// The short block identifier.
    pub block_id: BlockId,
    /// Merkle Tree path inside block.
    pub path: MerklePath,
}

/// The Blockchain.
pub struct Blockchain {
    /// Database.
    database: ListDb,

    // General state.
    /// Blockchain blocks stored in-memory.
    /// Position in this vector is short BlockId = usize.
    blocks: Vec<Block>,
    /// Block by hash mapping.
    block_by_hash: HashMap<Hash, BlockId>,
    /// Unspent outputs by hash.
    output_by_hash: HashMap<Hash, OutputKey>,

    //TODO: most of this fields is just duplication of keyblock. Save keyblock rather then copy of fields.
    //
    // Last blockchain info:
    // 1) Stake
    // 2) consensus group
    // 3) last block time
    // 4) epoch
    //
    /// Escrow
    pub escrow: Escrow,
    /// Snapshot of selected leader from the latest key block.
    pub leader: secure::PublicKey,
    /// Snapshot of selected facilitator from the latest key block.
    pub facilitator: secure::PublicKey,
    /// Snapshot of validators with stakes from the latest key block.
    pub validators: BTreeMap<secure::PublicKey, i64>,
    /// A timestamp when the last sealed block was received.
    pub last_block_timestamp: Instant,
    /// A monotonically increasing value that represents the epoch of the blockchain,
    /// starting from genesis block (=0).
    pub epoch: u64,
    /// Last height where epoch was changed.
    pub last_epoch_change: usize,

    // Monetary info
    /// The total sum of money created.
    created: ECp,
    /// The total sum of money burned.
    burned: ECp,
    /// The total sum of gamma adjustments.
    gamma: Fr,
    /// The total sum of monetary adjustments.
    monetary_adjustment: i64,
}

impl Blockchain {
    //----------------------------------------------------------------------------------------------
    // Public API
    //----------------------------------------------------------------------------------------------

    pub fn new(config: &StorageConfig) -> Blockchain {
        let database = ListDb::new(&config.database_path);
        Self::with_db(database)
    }

    pub fn testing() -> Blockchain {
        let database = ListDb::testing();
        Self::with_db(database)
    }

    fn with_db(database: ListDb) -> Blockchain {
        let blocks = Vec::new();
        let block_by_hash = HashMap::<Hash, BlockId>::new();
        let output_by_hash = HashMap::<Hash, OutputKey>::new();
        let epoch: u64 = 0;
        let escrow = Escrow::new();
        let leader: secure::PublicKey = secure::G2::generator().into(); // some fake key
        let facilitator: secure::PublicKey = secure::G2::generator().into(); // some fake key
        let validators = BTreeMap::<secure::PublicKey, i64>::new();
        let last_block_timestamp = clock::now();

        let created = ECp::inf();
        let burned = ECp::inf();
        let gamma = Fr::zero();
        let monetary_adjustment: i64 = 0;
        let last_epoch_change = 0;
        let mut blockchain = Blockchain {
            database,
            blocks,
            block_by_hash,
            output_by_hash,
            escrow,
            leader,
            facilitator,
            validators,
            epoch,
            last_epoch_change,
            last_block_timestamp,
            created,
            burned,
            gamma,
            monetary_adjustment,
        };
        let current_timestamp = Utc::now().timestamp() as u64;
        blockchain.load_blockchain(current_timestamp);
        blockchain
    }

    fn load_blockchain(&mut self, current_timestamp: u64) {
        let mut blocks = self.database.iter();

        let block = blocks.next();
        let block = if let Some(block) = block {
            block
        } else {
            debug!("Creating a new blockchain.");
            return;
        };

        info!("Loading blockchain from database.");

        self.handle_block(block, current_timestamp);
        for block in blocks {
            self.handle_block(block, current_timestamp);
        }
    }

    fn handle_block(&mut self, block: Block, current_timestamp: u64) {
        debug!(
            "load saved block hash = {:?}, block = {:?}",
            Hash::digest(&block),
            block
        );
        match block {
            Block::MonetaryBlock(block) => {
                let _ = self
                    .register_monetary_block(block, current_timestamp)
                    .expect("error processing saved monetary block.");
            }
            Block::KeyBlock(block) => {
                self.register_key_block(block)
                    .expect("error processing saved key block.");
            }
        }
    }

    /// Returns count of blocks in current epoch.
    pub fn blocks_in_epoch(&self) -> usize {
        self.blocks.len() - self.last_epoch_change
    }

    /// Returns an iterator over UTXO hashes.
    pub fn unspent(&self) -> Vec<Hash> {
        // TODO: return iterator instead.
        self.output_by_hash.keys().cloned().collect()
    }

    /// Find UTXO by its hash.
    pub fn output_by_hash(&self, output_hash: &Hash) -> Option<&Output> {
        if let Some(OutputKey { block_id, path }) = self.output_by_hash.get(output_hash) {
            let block = &self.blocks[*block_id];
            if let Block::MonetaryBlock(MonetaryBlock { header: _, body }) = block {
                if let Some(output) = body.outputs.lookup(path) {
                    return Some(&output);
                } else {
                    return None;
                }
            } else {
                unreachable!(); // Non-monetary block
            }
        }
        return None;
    }

    /// Resolve UTXOs by its hashes.
    pub fn outputs_by_hashes(
        &self,
        output_hashes: &[Hash],
    ) -> Result<Vec<Output>, BlockchainError> {
        // Find appropriate UTXO in the database.
        let mut outputs = Vec::<Output>::new();
        for output_hash in output_hashes {
            let input = match self.output_by_hash(output_hash) {
                Some(o) => o.clone(),
                None => return Err(BlockchainError::MissingUTXO(output_hash.clone()).into()),
            };
            outputs.push(input);
        }

        Ok(outputs)
    }

    /// Find block by its hash
    pub fn block_by_hash(&self, block_hash: &Hash) -> Option<&Block> {
        if let Some(block_id) = self.block_by_hash.get(block_hash) {
            return Some(&self.blocks[*block_id]);
        }
        return None;
    }

    /// Returns blocks history starting from block_hash, limited by count.
    pub fn blocks_range(&self, starting_hash: &Hash, count: u64) -> Option<Vec<Block>> {
        if let Some(&block_id) = self.block_by_hash.get(starting_hash) {
            let block_id = block_id + 1;
            return Some(
                self.database
                    .iter_starting(block_id as u64)
                    .take(count as usize)
                    .collect(),
            );
        }
        return None;
    }

    /// Return all blocks.
    pub fn blocks(&self) -> &[Block] {
        self.blocks.as_slice()
    }

    /// Return the last block.
    pub fn last_block(&self) -> &Block {
        assert!(self.blocks.len() > 0);
        self.blocks.last().unwrap()
    }

    /// Return the current blockchain height.
    pub fn height(&self) -> usize {
        self.blocks().len()
    }

    //----------------------------------------------------------------------------------------------

    /// Force consensus group changes.
    pub fn change_group(
        &mut self,
        leader: secure::PublicKey,
        facilitator: secure::PublicKey,
        validators: BTreeMap<secure::PublicKey, i64>,
    ) {
        self.leader = leader;
        self.facilitator = facilitator;
        self.validators = validators;
    }

    pub fn register_key_block(&mut self, block: KeyBlock) -> Result<(), Error> {
        let block_id = self.blocks.len();
        self.database
            .insert(block_id as u64, Block::KeyBlock(block.clone()))?;

        // Check previous hash.
        if let Some(previous_block) = self.blocks.last() {
            let previous_hash = Hash::digest(previous_block);
            if previous_hash != block.header.base.previous {
                return Err(BlockchainError::PreviousHashMismatch(
                    previous_hash,
                    block.header.base.previous,
                )
                .into());
            }
        }

        // Check new hash.
        let this_hash = Hash::digest(&block);
        if let Some(_) = self.block_by_hash.get(&this_hash) {
            return Err(BlockchainError::BlockHashCollision(this_hash).into());
        }

        // -----------------------------------------------------------------------------------------
        // Alright, starting transaction.
        // -----------------------------------------------------------------------------------------

        info!(
            "Registered key block: height={}, hash={}",
            self.blocks.len() + 1,
            this_hash
        );

        if let Some(_) = self.block_by_hash.insert(this_hash.clone(), block_id) {
            panic!("Block hash collision");
        }

        assert_eq!(self.epoch + 1, block.header.base.epoch);
        self.epoch = self.epoch + 1;
        self.last_epoch_change = self.blocks().len();

        self.leader = block.header.leader.clone();
        self.facilitator = block.header.facilitator.clone();
        self.validators = self.escrow.multiget(&block.header.validators);

        self.last_block_timestamp = clock::now();
        self.blocks.push(Block::KeyBlock(block));
        debug!("Validators: {:?}", &self.validators);
        for (key, stake) in self.validators.iter() {
            let key_str = key.to_string();
            metrics::VALIDATOR_STAKE_GAUGEVEC
                .with_label_values(&[key_str.as_str()])
                .set(*stake);
        }

        metrics::HEIGHT.inc();
        metrics::EPOCH.inc();
        Ok(())
    }

    pub fn register_monetary_block(
        &mut self,
        mut block: MonetaryBlock,
        current_timestamp: u64,
    ) -> Result<(Vec<Output>, Vec<Output>), Error> {
        let block_id = self.blocks.len();
        self.database
            .insert(block_id as u64, Block::MonetaryBlock(block.clone()))?;

        // Check previous hash.
        if let Some(previous_block) = self.blocks.last() {
            let previous_hash = Hash::digest(previous_block);
            if previous_hash != block.header.base.previous {
                return Err(BlockchainError::PreviousHashMismatch(
                    previous_hash,
                    block.header.base.previous,
                )
                .into());
            }
        }

        // Check new hash.
        let this_hash = Hash::digest(&block);
        if let Some(_) = self.block_by_hash.get(&this_hash) {
            return Err(BlockchainError::BlockHashCollision(this_hash).into());
        }

        let mut burned = ECp::inf();
        let mut created = ECp::inf();

        // Check all inputs.
        for output_hash in &block.body.inputs {
            if let Some(OutputKey { block_id, path }) = self.output_by_hash.get(output_hash) {
                assert!(*block_id < self.blocks.len());
                let block = &self.blocks[*block_id];
                if let Block::MonetaryBlock(MonetaryBlock { header: _, body }) = block {
                    if let Some(output) = body.outputs.lookup(&path) {
                        // Check that hash is the same.
                        assert_eq!(Hash::digest(output), *output_hash);

                        // Calculate balance.
                        match output.as_ref() {
                            Output::PaymentOutput(o) => {
                                burned += Pt::decompress(o.proof.vcmt)?;
                            }
                            Output::StakeOutput(o) => {
                                burned += fee_a(o.amount);
                            }
                        }
                    } else {
                        // Internal database inconsistency - missing UTXO in block.
                        unreachable!();
                    }
                } else {
                    // Internal database inconsistency - invalid block type.
                    unreachable!();
                }
            } else {
                // Cannot find UTXO referred by block.
                return Err(BlockchainError::MissingUTXO(*output_hash).into());
            }
        }

        // Check all outputs.
        let mut outputs_pathes: Vec<(Hash, MerklePath)> = Vec::new();
        for (output, path) in block.body.outputs.leafs() {
            // Check that hash is unique.
            let output_hash = Hash::digest(output.as_ref());
            if let Some(_) = self.output_by_hash.get(&output_hash) {
                return Err(BlockchainError::OutputHashCollision(output_hash).into());
            }
            outputs_pathes.push((output_hash, path));

            // Calculate balance.
            match output.as_ref() {
                Output::PaymentOutput(o) => {
                    created += Pt::decompress(o.proof.vcmt)?;
                }
                Output::StakeOutput(o) => {
                    created += fee_a(o.amount);
                }
            }
        }

        // Check the block monetary balance.
        if fee_a(block.header.monetary_adjustment) + burned - created != block.header.gamma * (*G) {
            return Err(BlockchainError::InvalidBlockBalance.into());
        }

        // Check the global monetary balance.
        let created: ECp = self.created + created;
        let burned: ECp = self.burned + burned;
        let gamma: Fr = self.gamma + block.header.gamma;
        let monetary_adjustment: i64 = self.monetary_adjustment + block.header.monetary_adjustment;
        if fee_a(monetary_adjustment) + burned - created != gamma * (*G) {
            panic!("Invalid global monetary balance");
        }

        // -----------------------------------------------------------------------------------------
        // Alright, starting transaction.
        // -----------------------------------------------------------------------------------------
        info!(
            "Registered monetary block: height={}, hash={}, inputs={}, outputs={}",
            self.blocks.len() + 1,
            this_hash,
            block.body.inputs.len(),
            outputs_pathes.len()
        );

        let mut pruned: Vec<Output> = Vec::with_capacity(block.body.inputs.len());

        // Remove spent outputs.
        for output_hash in &block.body.inputs {
            info!("Pruned UXTO: hash={}", output_hash);
            // Remove from the set of unspent outputs.
            if let Some(OutputKey { block_id, path }) = self.output_by_hash.remove(output_hash) {
                let block = &mut self.blocks[block_id];
                if let Block::MonetaryBlock(MonetaryBlock { header: _, body }) = block {
                    // Remove from the block.
                    if let Some(output) = body.outputs.prune(&path) {
                        pruned.push(*output);
                    } else {
                        unreachable!();
                    }
                } else {
                    unreachable!();
                }
            } else {
                unreachable!();
            }
        }

        // Prune inputs.
        block.body.inputs.clear();

        // Register created unspent outputs.
        for (hash, path) in outputs_pathes {
            info!("Registered UXTO: hash={}", &hash);

            // Create the new unspent output
            let output_key = OutputKey { block_id, path };
            if let Some(_) = self.output_by_hash.insert(hash, output_key) {
                unreachable!();
            }
        }

        // Register block
        if let Some(_) = self.block_by_hash.insert(this_hash.clone(), block_id) {
            unreachable!();
        }

        let outputs: Vec<Output> = block
            .body
            .outputs
            .leafs()
            .drain(..)
            .map(|(o, _path)| *o.clone())
            .collect();

        // remove pruned outputs
        for input in &pruned {
            if let Output::StakeOutput(o) = input {
                let output_hash = Hash::digest(o);
                self.escrow
                    .unstake(o.validator, output_hash, current_timestamp);
            }
        }

        // register new outputs
        for output in &outputs {
            if let Output::StakeOutput(o) = output {
                let output_hash = Hash::digest(o);
                let block_timestamp = block.header.base.timestamp;
                let bonding_timestamp = block_timestamp + crate::escrow::BONDING_TIME;
                self.escrow
                    .stake(o.validator, output_hash, bonding_timestamp, o.amount);
            }
        }

        // Save the global balance.
        self.created = created;
        self.burned = burned;
        self.gamma = gamma;
        self.monetary_adjustment = monetary_adjustment;
        self.last_block_timestamp = clock::now();
        self.blocks.push(Block::MonetaryBlock(block));
        metrics::HEIGHT.inc();
        metrics::UTXO_LEN.set(self.output_by_hash.len() as i64);
        Ok((pruned, outputs))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use chrono::prelude::Utc;

    use crate::genesis::genesis;
    use std::collections::BTreeSet;
    use stegos_crypto::curve1174::cpt::*;
    use stegos_keychain::KeyChain;

    fn unspent(blockchain: &Blockchain, skey: &SecretKey) -> (Output, Fr, i64) {
        for input_hash in blockchain.unspent() {
            let input = blockchain.output_by_hash(&input_hash).unwrap();
            match input {
                Output::PaymentOutput(o) => {
                    let payload = o.decrypt_payload(skey).expect("keys are valid");
                    return (input.clone(), payload.gamma, payload.amount);
                }
                _ => {}
            }
        }
        unreachable!();
    }

    fn iterate(
        blockchain: &mut Blockchain,
        skey: &SecretKey,
        pkey: &PublicKey,
    ) -> Result<(), Error> {
        let version = 1;
        let timestamp = Utc::now().timestamp() as u64;
        let (epoch, previous) = {
            let last = blockchain.last_block();
            let base_header = last.base_header();
            let epoch = base_header.epoch + 1;
            let previous = Hash::digest(last);
            (epoch, previous)
        };

        let base = BaseBlockHeader::new(version, previous, epoch, timestamp);

        let (input, input_gamma, amount) = unspent(blockchain, skey);
        let input_hashes = [Hash::digest(&input)];
        let inputs = [input];

        let (output, output_gamma) =
            Output::new_payment(timestamp, skey, pkey, amount).expect("tests have valid keys");
        let outputs = [output];

        let gamma = input_gamma - output_gamma;
        let block = MonetaryBlock::new(base, gamma, 0, &input_hashes, &outputs);
        block.validate(&inputs).expect("block is valid");
        blockchain.register_monetary_block(block, timestamp)?;

        Ok(())
    }

    #[test]
    fn basic() {
        use simple_logger;
        simple_logger::init_with_level(log::Level::Debug).unwrap_or_default();
        let keychains = [
            KeyChain::new_mem(),
            KeyChain::new_mem(),
            KeyChain::new_mem(),
        ];

        let current_timestamp = Utc::now().timestamp() as u64;
        let blocks = genesis(&keychains, 1000, 1_000_000, current_timestamp);
        let mut blockchain = Blockchain::testing();
        for block in blocks {
            match block {
                Block::KeyBlock(block) => blockchain.register_key_block(block).unwrap(),
                Block::MonetaryBlock(block) => {
                    blockchain
                        .register_monetary_block(block, current_timestamp)
                        .unwrap();
                }
            }
        }

        let skey = &keychains[0].wallet_skey;
        let pkey = &keychains[0].wallet_pkey;
        assert!(blockchain.blocks().len() > 0);
        for _ in 0..3 {
            iterate(&mut blockchain, skey, pkey).unwrap();
        }
    }

    #[test]
    fn block_range_limit() {
        use simple_logger;
        simple_logger::init_with_level(log::Level::Debug).unwrap_or_default();
        let keychains = [
            KeyChain::new_mem(),
            KeyChain::new_mem(),
            KeyChain::new_mem(),
        ];

        let current_timestamp = Utc::now().timestamp() as u64;
        let blocks = genesis(&keychains, 1000, 1_000_000, current_timestamp);
        let mut blockchain = Blockchain::testing();
        for block in blocks {
            match block {
                Block::KeyBlock(block) => blockchain.register_key_block(block).unwrap(),
                Block::MonetaryBlock(block) => {
                    blockchain
                        .register_monetary_block(block, current_timestamp)
                        .unwrap();
                }
            }
        }

        let last_block = blockchain.last_block();
        let start = Hash::digest(last_block);
        // len of genesis
        assert!(blockchain.blocks().len() > 0);
        let version: u64 = 1;
        for epoch in 2..12 {
            let new_block = {
                let previous = Hash::digest(&blockchain.last_block());
                let base = BaseBlockHeader::new(version, previous, epoch, 0);

                let validators: BTreeSet<secure::PublicKey> =
                    keychains.iter().map(|p| p.network_pkey.clone()).collect();
                let leader = keychains[0].network_pkey.clone();
                let facilitator = keychains[0].network_pkey.clone();

                KeyBlock::new(base, leader, facilitator, validators)
            };
            blockchain.register_key_block(new_block).unwrap();
        }

        assert_eq!(blockchain.blocks_range(&start, 1).unwrap().len(), 1);

        assert_eq!(blockchain.blocks_range(&start, 4).unwrap().len(), 4);
        // limit
        assert_eq!(blockchain.blocks_range(&start, 20).unwrap().len(), 10);
    }
}
