//! Node - API.

//
// Copyright (c) 2019 Stegos AG
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
use super::replication::api::*;
use futures::sync::mpsc;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use stegos_blockchain::{
    ElectionInfo, EpochInfo, EscrowInfo, MacroBlock, MicroBlock, Output, Timestamp, Transaction,
    ValidatorKeyInfo,
};

use stegos_crypto::hash::{Hash, Hashable, Hasher};
use stegos_crypto::scc;
use stegos_crypto::utils::{
    deserialize_protobuf_array_from_hex, deserialize_protobuf_from_hex,
    serialize_protobuf_array_to_hex, serialize_protobuf_to_hex,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "output_type")]
#[serde(rename_all = "snake_case")]
pub enum OutputType {
    PublicPayment,
    Payment { comment: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewOutputInfo {
    #[serde(flatten)]
    pub output_type: OutputType,
    pub recipient: scc::PublicKey,
    pub amount: i64,
}
///
/// RPC requests.
///
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum NodeRequest {
    ElectionInfo {},
    EscrowInfo {},
    ReplicationInfo {},
    PopMicroBlock {},
    ChainName {},
    BroadcastTransaction {
        #[serde(serialize_with = "serialize_protobuf_to_hex")]
        #[serde(deserialize_with = "deserialize_protobuf_from_hex")]
        data: Transaction,
    },
    /// Get full output corresponding to output id.
    OutputsList {
        utxos: Vec<Hash>,
    },
    /// Create transaction From inputs, and information about outputs.
    CreateRawTransaction {
        /// Transaction inputs ids, Currently should be from same sender.
        txins: Vec<Hash>,
        txouts: Vec<NewOutputInfo>,
        secret_key: [u8; 32],
        fee: i64,

        #[serde(default)]
        #[serde(serialize_with = "serialize_protobuf_array_to_hex")]
        #[serde(deserialize_with = "deserialize_protobuf_array_from_hex")]
        unspent_list: Vec<Output>,
    },
    ValidateCertificate {
        output_hash: Hash,
        spender: scc::PublicKey,
        recipient: scc::PublicKey,
        rvalue: scc::Fr,
    },
    EnableRestaking {},
    DisableRestaking {},
    ChangeUpstream {},
    StatusInfo {},
    ValidatorsInfo {},
    SubscribeStatus {},
    MacroBlockInfo {
        epoch: u64,
    },
    MicroBlockInfo {
        epoch: u64,
        offset: u32,
    },
    SubscribeChain {
        epoch: u64,
        offset: u32,
    },
}

///
/// RPC responses.
///
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum NodeResponse {
    ElectionInfo(ElectionInfo),
    EscrowInfo(EscrowInfo),
    ReplicationInfo(ReplicationInfo),
    MicroBlockPopped,
    ChainName {
        name: String,
    },
    OutputsList {
        #[serde(serialize_with = "serialize_protobuf_array_to_hex")]
        #[serde(deserialize_with = "deserialize_protobuf_array_from_hex")]
        utxos: Vec<Output>,
    },
    CreateRawTransaction {
        txouts: Vec<Hash>,
        #[serde(serialize_with = "serialize_protobuf_to_hex")]
        #[serde(deserialize_with = "deserialize_protobuf_from_hex")]
        data: Transaction,
    },
    BroadcastTransaction {
        hash: Hash,
        status: TransactionStatus,
    },
    CertificateValid {
        epoch: u64,
        block_hash: Hash,
        is_final: bool,
        timestamp: Timestamp,
        amount: i64,
    },
    RestakingEnabled,
    RestakingDisabled,
    UpstreamChanged,
    StatusInfo(StatusInfo),
    ValidatorsInfo {
        epoch: u64,
        offset: u32,
        view_change: u32,
        validators: Vec<ValidatorKeyInfo>,
    },
    SubscribedStatus {
        #[serde(flatten)]
        status: StatusInfo,
        #[serde(skip)]
        rx: Option<mpsc::Receiver<StatusNotification>>, // Option is needed for serde.
    },
    MacroBlockInfo(ExtendedMacroBlock),
    MicroBlockInfo(MicroBlock),
    SubscribedChain {
        current_epoch: u64,
        current_offset: u32,
        #[serde(skip)]
        rx: Option<mpsc::Receiver<ChainNotification>>, // Option is needed for serde.
    },
    Error {
        error: String,
    },
}

/// Notification about synchronization status.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusInfo {
    pub is_synchronized: bool,
    pub epoch: u64,
    pub offset: u32,
    pub view_change: u32,
    pub last_block_hash: Hash,
    pub last_macro_block_hash: Hash,
    pub last_macro_block_timestamp: Timestamp,
    pub local_timestamp: Timestamp,
}

/// Status notifications.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum StatusNotification {
    StatusChanged(StatusInfo),
}

impl From<StatusInfo> for StatusNotification {
    fn from(status: StatusInfo) -> StatusNotification {
        StatusNotification::StatusChanged(status)
    }
}

/// Blockchain notifications.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ChainNotification {
    MicroBlockPrepared(MicroBlock),
    MicroBlockReverted(RevertedMicroBlock),
    MacroBlockCommitted(ExtendedMacroBlock),
}

/// A macro block with extra information.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtendedMacroBlock {
    /// Committed macro block.
    #[serde(flatten)]
    pub block: MacroBlock,
    /// Collected information about epoch.
    #[serde(flatten)]
    pub epoch_info: EpochInfo,
}

impl ExtendedMacroBlock {
    pub fn inputs(&self) -> impl Iterator<Item = &Hash> {
        self.block.inputs.iter()
    }
    pub fn outputs(&self) -> impl Iterator<Item = &Output> {
        self.block.outputs.iter()
    }
}

/// Information about reverted micro block.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevertedMicroBlock {
    #[serde(flatten)]
    pub block: MicroBlock,
    #[serde(skip)] // internal API for wallet
    pub recovered_inputs: HashMap<Hash, Output>,
    #[serde(skip)] // internal API for wallet
    pub pruned_outputs: Vec<Hash>,
}

impl RevertedMicroBlock {
    pub fn pruned_outputs(&self) -> impl Iterator<Item = &Hash> {
        self.pruned_outputs.iter()
    }
    pub fn recovered_inputs(&self) -> impl Iterator<Item = &Output> {
        self.recovered_inputs.values()
    }
}

/// PA micro block with extra information.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtendedMicroBlock {
    #[serde(flatten)]
    pub block: MicroBlock,
}

impl From<ExtendedMacroBlock> for ChainNotification {
    fn from(block: ExtendedMacroBlock) -> ChainNotification {
        ChainNotification::MacroBlockCommitted(block)
    }
}

impl From<MicroBlock> for ChainNotification {
    fn from(block: MicroBlock) -> ChainNotification {
        ChainNotification::MicroBlockPrepared(block)
    }
}

impl From<RevertedMicroBlock> for ChainNotification {
    fn from(block: RevertedMicroBlock) -> ChainNotification {
        ChainNotification::MicroBlockReverted(block)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "status")]
#[serde(rename_all = "snake_case")]
pub enum TransactionStatus {
    Created {},
    Accepted {},
    Rejected {
        error: String,
    },
    /// Transaction was included in microblock.
    Prepared {
        epoch: u64,
        offset: u32,
    },
    /// Transaction was reverted back to mempool.
    Rollback {
        epoch: u64,
        offset: u32,
    },
    /// Transaction was committed to macro block.
    Committed {
        epoch: u64,
    },
    /// Transaction was rejected, because other conflicted
    Conflicted {
        epoch: u64,
        offset: Option<u32>,
    },
}

impl Hashable for TransactionStatus {
    fn hash(&self, hasher: &mut Hasher) {
        match self {
            TransactionStatus::Created {} => "Created".hash(hasher),
            TransactionStatus::Accepted {} => "Accepted".hash(hasher),
            TransactionStatus::Rejected { error } => {
                "Rejected".hash(hasher);
                error.hash(hasher)
            }
            TransactionStatus::Prepared { epoch, offset } => {
                "Prepare".hash(hasher);
                epoch.hash(hasher);
                offset.hash(hasher);
            }
            TransactionStatus::Rollback { epoch, offset } => {
                "Rollback".hash(hasher);
                epoch.hash(hasher);
                offset.hash(hasher);
            }
            TransactionStatus::Committed { epoch } => {
                "Committed".hash(hasher);
                epoch.hash(hasher);
            }
            TransactionStatus::Conflicted { epoch, offset } => {
                "Conflicted".hash(hasher);

                epoch.hash(hasher);
                if let Some(offset) = offset {
                    "some".hash(hasher);
                    offset.hash(hasher);
                } else {
                    "none".hash(hasher);
                }
            }
        }
    }
}
