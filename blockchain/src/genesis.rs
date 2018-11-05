//! Genesis Block.

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

use block::*;
use chrono::prelude::{TimeZone, Utc};
use input::*;
use merkle::MerklePath;
use output::*;
use payload::EncryptedPayload;
use stegos_crypto::bulletproofs;
use stegos_crypto::hash::Hash;
use stegos_crypto::pbc::fast::Zr;
use stegos_crypto::pbc::secure::*;

/// Genesis block for tests and development purposes.
pub fn genesis_dev() -> (Block, Vec<MerklePath>) {
    let version: u64 = 1;
    let amount: i64 = 1_000_000;
    let epoch: u64 = 1;
    let previous = Hash::digest(&"dev".to_string());
    let (skey, pkey, sig) = make_deterministic_keys(b"dev");
    let delta: Zr = Zr::random();
    let timestamp = Utc.ymd(2018, 11, 01).and_hms(0, 0, 0).timestamp() as u64;

    let leader = pkey;

    // Genesis is self-signed.
    let witnesses = [leader.clone()];

    // Recipient is ourselves.
    let recipient = leader.clone();

    // Genesis block doesn't have inputs.
    let inputs: [Input; 0] = [];

    // Genesis block have one hard-coded output.
    let (proof, _gamma) = bulletproofs::make_range_proof(amount);
    // TODO: replace with real EncryptedPayload
    let payload = EncryptedPayload::garbage();
    let output = Output::new(recipient, proof, payload);
    let outputs = [output];

    let (block, paths) = Block::sign(
        &skey, version, epoch, previous, timestamp, leader, delta, &witnesses, &inputs, &outputs,
    );

    // Fool-proof checks.
    static PREVIOUS_HEX: &str = "daeed6308874de11ec5ba896aff636aee60821b397f88164be3eae5cf6d276d8";
    static SKEY_HEX: &str = "daeed6308874de11ec5ba896aff636aee60821b397f88164be3eae5cf6d276d8";
    static PKEY_HEX: &str = "bd2f2d45a439eafb3523216a652344883b3930f634a8c6e72eda55ff1f8670f9c90f139d9f9a486c6def760d2ff4e74d1f468c848c9774e63cdd0d46917eefe401";
    static SIG_HEX: &str = "901fc4370d2b19da0c4663ab4394d3e3cb74fdca7fc0ed7c8a8f0db3d2bff7d200";
    // static DELTA_HEX: &str = "3987487567fa7d862b5890ba4b288efc486298ba";
    // static HASH_HEX: &str = "3334d1466924068a65de9be925059ab9ee8866f62db9432d502edd7252b483ea";
    assert_eq!(
        previous,
        Hash::from_hash_facsimile_str(PREVIOUS_HEX).expect("hex")
    );
    assert_eq!(skey, SecretKey::from_str(SKEY_HEX).expect("hex"));
    assert_eq!(pkey, PublicKey::from_str(PKEY_HEX).expect("hex"));
    assert_eq!(sig, Signature::from_str(SIG_HEX).expect("hex"));
    // assert_eq!(delta, Zr::from_str(DELTA_HEX).expect("hex"));
    // assert_eq!(block.header.hash, Hash::from_str(HASH_HEX).expect("hex"));

    (block, paths)
}
