//! Wallet - Transactions.

//
// Copyright (c) 2018 Stegos
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

use crate::change::*;
use crate::error::*;
use chrono::Utc;
use failure::Error;
use log::*;
use std::collections::HashMap;
use stegos_blockchain::DataOutput;
use stegos_blockchain::EscrowOutput;
use stegos_blockchain::Output;
use stegos_blockchain::PaymentOutput;
use stegos_blockchain::Transaction;
use stegos_config::*;
use stegos_crypto::curve1174::cpt::PublicKey;
use stegos_crypto::curve1174::cpt::SecretKey;
use stegos_crypto::curve1174::fields::Fr;
use stegos_crypto::hash::Hash;
use stegos_crypto::pbc::secure;

/// Calculate fee for data transaction.
/// Sic: this method was copy-pasted from NodeService to avoid
/// Wallet <-> NodeService dependency.
fn data_fee(size: usize, ttl: u64) -> i64 {
    assert!(size > 0);
    let units: usize = (size + (DATA_UNIT - 1)) / DATA_UNIT;
    (units as i64) * (ttl as i64) * DATA_UNIT_FEE
}

/// Create a new payment transaction.
/// Sic: used by unit tests in NodeService.
pub fn create_payment_transaction(
    sender_skey: &SecretKey,
    sender_pkey: &PublicKey,
    recipient: &PublicKey,
    unspent: &HashMap<Hash, (PaymentOutput, i64)>,
    amount: i64,
) -> Result<Transaction, Error> {
    if amount <= 0 {
        return Err(WalletError::ZeroOrNegativeAmount.into());
    }

    debug!(
        "Creating a payment transaction: recipient={}, amount={}",
        recipient, amount
    );

    //
    // Find inputs
    //

    trace!("Checking for available funds in the wallet...");
    let fee = PAYMENT_FEE;
    let fee_change = fee + PAYMENT_FEE;
    let unspent_iter = unspent.values().map(|(o, a)| (o, *a));
    let (inputs, fee, change) = find_utxo(unspent_iter, amount, fee, fee_change)?;
    let inputs: Vec<Output> = inputs
        .into_iter()
        .map(|o| Output::PaymentOutput(o.clone()))
        .collect();

    debug!(
        "Transaction preview: recipient={}, amount={}, withdrawn={}, change={}, fee={}",
        recipient,
        amount,
        amount + change + fee,
        change,
        fee
    );
    for input in &inputs {
        debug!("Use UTXO: hash={}", Hash::digest(input));
    }

    //
    // Create outputs
    //

    let timestamp = Utc::now().timestamp() as u64;
    let mut outputs: Vec<Output> = Vec::<Output>::with_capacity(2);

    // Create an output for payment
    trace!("Creating change UTXO...");
    let (output1, gamma1) = Output::new_payment(timestamp, sender_skey, recipient, amount)?;
    info!(
        "Created payment UTXO: hash={}, recipient={}, amount={}",
        Hash::digest(&output1),
        recipient,
        amount
    );
    outputs.push(output1);
    let mut gamma = gamma1;

    if change > 0 {
        // Create an output for change
        trace!("Creating change UTXO...");
        let (output2, gamma2) = Output::new_payment(timestamp, sender_skey, sender_pkey, change)?;
        info!(
            "Created change UTXO: hash={}, recipient={}, change={}",
            Hash::digest(&output2),
            sender_pkey,
            change
        );
        outputs.push(output2);
        gamma += gamma2;
    }

    trace!("Signing transaction...");
    let tx = Transaction::new(sender_skey, &inputs, &outputs, gamma, fee)?;
    let tx_hash = Hash::digest(&tx);
    info!(
        "Signed payment transaction: hash={}, recipient={}, amount={}, withdrawn={}, change={}, fee={}",
        tx_hash,
        recipient,
        amount,
        amount + change + fee,
        change,
        fee
    );

    Ok(tx)
}

/// Create a new data transaction.
/// Sic: used by unit tests in NodeService.
pub fn create_data_transaction(
    sender_skey: &SecretKey,
    sender_pkey: &PublicKey,
    recipient: &PublicKey,
    unspent: &HashMap<Hash, (PaymentOutput, i64)>,
    ttl: u64,
    data: Vec<u8>,
) -> Result<Transaction, Error> {
    debug!(
        "Creating a data transaction: recipient={}, ttl={}",
        recipient, ttl
    );

    //
    // Find inputs
    //

    trace!("Checking for available funds in the wallet...");

    let fee = data_fee(data.len(), ttl);
    let fee_change = fee + PAYMENT_FEE;
    let unspent_iter = unspent.values().map(|(o, a)| (o, *a));
    let (inputs, fee, change) = find_utxo(unspent_iter, 0, fee, fee_change)?;
    let inputs: Vec<Output> = inputs
        .into_iter()
        .map(|o| Output::PaymentOutput(o.clone()))
        .collect();

    debug!(
        "Transaction preview: recipient={}, ttl={}, withdrawn={}, change={}, fee={}",
        recipient,
        ttl,
        change + fee,
        change,
        fee
    );
    for input in &inputs {
        debug!("Use UTXO: hash={}", Hash::digest(input));
    }

    //
    // Create outputs
    //

    let timestamp = Utc::now().timestamp() as u64;
    let mut outputs: Vec<Output> = Vec::<Output>::with_capacity(2);

    // Create an output for payment
    trace!("Creating data UTXO...");
    let (output1, gamma1) = Output::new_data(timestamp, sender_skey, recipient, ttl, &data)?;
    info!(
        "Created data UTXO: hash={}, recipient={}, ttl={}",
        Hash::digest(&output1),
        recipient,
        ttl
    );
    outputs.push(output1);
    let mut gamma = gamma1;

    if change > 0 {
        // Create an output for change
        trace!("Creating change UTXO...");
        let (output2, gamma2) = Output::new_payment(timestamp, sender_skey, sender_pkey, change)?;
        info!(
            "Created change UTXO: hash={}, recipient={}, change={}",
            Hash::digest(&output2),
            recipient,
            change
        );
        outputs.push(output2);
        gamma += gamma2;
    }

    trace!("Signing transaction...");
    let tx = Transaction::new(sender_skey, &inputs, &outputs, gamma, fee)?;
    let tx_hash = Hash::digest(&tx);
    info!(
        "Signed data transaction: hash={}, recipient={}, ttl={}, spent={}, change={}, fee={}",
        tx_hash,
        recipient,
        ttl,
        change + fee,
        change,
        fee
    );

    Ok(tx)
}

/// Create a new transaction to prune data.
pub(crate) fn create_data_pruning_transaction(
    sender_skey: &SecretKey,
    output: DataOutput,
) -> Result<Transaction, Error> {
    let output_hash = Hash::digest(&output);

    debug!(
        "Creating a data pruning transaction: data_utxo={}",
        output_hash
    );

    let inputs = [Output::DataOutput(output)];
    let outputs = [];
    let adjustment = Fr::zero();
    let fee: i64 = 0;

    trace!("Signing transaction...");
    let tx = Transaction::new(sender_skey, &inputs, &outputs, adjustment, fee)?;
    let tx_hash = Hash::digest(&tx);
    info!(
        "Signed data pruning transaction: hash={}, data_utxo={}, fee={}",
        tx_hash, output_hash, fee
    );

    Ok(tx)
}

/// Create a new staking transaction.
pub fn create_staking_transaction(
    sender_skey: &SecretKey,
    sender_pkey: &PublicKey,
    validator_pkey: &secure::PublicKey,
    unspent: &HashMap<Hash, (PaymentOutput, i64)>,
    amount: i64,
) -> Result<Transaction, Error> {
    if amount <= ESCROW_FEE + PAYMENT_FEE {
        return Err(WalletError::ZeroOrNegativeAmount.into());
    }

    debug!(
        "Creating a staking transaction: validator={}, amount={}",
        validator_pkey, amount
    );

    //
    // Find inputs
    //

    trace!("Checking for available funds in the wallet...");
    let fee = ESCROW_FEE;
    let fee_change = fee + PAYMENT_FEE;
    let unspent_iter = unspent.values().map(|(o, a)| (o, *a));
    let (inputs, fee, change) = find_utxo(unspent_iter, amount, fee, fee_change)?;
    let inputs: Vec<Output> = inputs
        .into_iter()
        .map(|o| Output::PaymentOutput(o.clone()))
        .collect();

    debug!(
        "Transaction preview: recipient={}, validator={}, stake={}, withdrawn={}, change={}, fee={}",
        sender_pkey,
        validator_pkey,
        amount,
        amount + change + fee,
        change,
        fee
    );
    for input in &inputs {
        debug!("Use UTXO: hash={}", Hash::digest(input));
    }

    //
    // Create outputs
    //

    let timestamp = Utc::now().timestamp() as u64;
    let mut outputs: Vec<Output> = Vec::<Output>::with_capacity(2);

    // Create an output for escrow
    trace!("Creating escrow UTXO...");
    let output1 = Output::new_escrow(timestamp, sender_skey, sender_pkey, validator_pkey, amount)?;
    info!(
        "Created escrow UTXO: hash={}, recipient={}, validator={}, amount={}",
        Hash::digest(&output1),
        sender_pkey,
        validator_pkey,
        amount
    );
    outputs.push(output1);
    let mut gamma = Fr::zero();

    if change > 0 {
        // Create an output for change
        trace!("Creating change UTXO...");
        let (output2, gamma2) = Output::new_payment(timestamp, sender_skey, sender_pkey, change)?;
        info!(
            "Created change UTXO: hash={}, recipient={}, change={}",
            Hash::digest(&output2),
            sender_pkey,
            change
        );
        outputs.push(output2);
        gamma += gamma2;
    }

    trace!("Signing transaction...");
    let tx = Transaction::new(sender_skey, &inputs, &outputs, gamma, fee)?;
    let tx_hash = Hash::digest(&tx);
    info!(
        "Signed stake transaction: hash={}, validator={}, stake={}, withdrawn={}, change={}, fee={}",
        tx_hash,
        validator_pkey,
        amount,
        amount + change + fee,
        change,
        fee
    );

    Ok(tx)
}

/// Create a new unstaking transaction.
pub fn create_unstaking_transaction(
    sender_skey: &SecretKey,
    sender_pkey: &PublicKey,
    validator_pkey: &secure::PublicKey,
    unspent: &HashMap<Hash, EscrowOutput>,
    amount: i64,
) -> Result<Transaction, Error> {
    if amount <= 0 {
        return Err(WalletError::ZeroOrNegativeAmount.into());
    }

    debug!(
        "Creating a unstaking transaction: recipient={}, validator={}, amount={}",
        sender_pkey, validator_pkey, amount
    );

    //
    // Find inputs
    //

    trace!("Checking for staked money in the wallet...");
    let fee = PAYMENT_FEE;
    let fee_change = fee + ESCROW_FEE;
    let unspent_iter = unspent.values().map(|o| (o, o.amount));
    let (inputs, fee, change) = find_utxo(unspent_iter, amount, fee, fee_change)?;
    let inputs: Vec<Output> = inputs
        .into_iter()
        .map(|o| Output::EscrowOutput(o.clone()))
        .collect();

    debug!(
        "Transaction preview: recipient={}, validator={}, unstake={}, stake={}, fee={}",
        sender_pkey, validator_pkey, amount, change, fee
    );
    for input in &inputs {
        debug!("Use escrow UTXO: hash={}", Hash::digest(input));
    }

    //
    // Create outputs
    //

    let timestamp = Utc::now().timestamp() as u64;
    let mut outputs: Vec<Output> = Vec::<Output>::with_capacity(2);

    // Create an output for payment
    trace!("Creating payment UTXO...");
    let (output1, gamma1) = Output::new_payment(timestamp, sender_skey, sender_pkey, amount)?;
    info!(
        "Created payment UTXO: hash={}, recipient={}, amount={}",
        Hash::digest(&output1),
        sender_pkey,
        amount
    );
    outputs.push(output1);
    let gamma = gamma1;

    if change > 0 {
        // Create an output for escrow
        trace!("Creating escrow UTXO...");
        let output2 =
            Output::new_escrow(timestamp, sender_skey, sender_pkey, validator_pkey, change)?;
        info!(
            "Created escrow UTXO: hash={}, validator={}, amount={}",
            Hash::digest(&output2),
            validator_pkey,
            change
        );
        outputs.push(output2);
    }

    trace!("Signing transaction...");
    let tx = Transaction::new(sender_skey, &inputs, &outputs, gamma, fee)?;
    let tx_hash = Hash::digest(&tx);
    info!(
        "Signed unstake transaction: hash={}, validator={}, unstake={}, stake={}, fee={}",
        tx_hash, validator_pkey, amount, change, fee
    );

    Ok(tx)
}