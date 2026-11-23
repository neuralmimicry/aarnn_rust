// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Lock-step neuron-range shard groups over TCP.
//!
//! A shard group splits every population of one network into contiguous
//! cell ranges (`crate::runner::shard_cell_range`), one per process. Shard 0
//! is the *coordinator*: it lives with the embodiment front end (the IPC
//! owner that encodes body frames and decodes motor replies) and drives the
//! group; shards `1..count` are *followers* in other nodes. Per step:
//!
//! 1. the coordinator sends `STEP` (tick, dt, reward, encoded sensory frame,
//!    output state) to every follower, and every shard runs the same step;
//! 2. at each population's spike commit, followers send their cells
//!    (`SLICE`: spikes, membrane voltages, output currents) and the
//!    coordinator returns the merged population (`MERGED`);
//! 3. followers report `DONE`; only then does the coordinator's step return,
//!    so a motor reply is sent after every shard has finished the step.
//!
//! The coordinator sends its own snapshot in `INIT`, so every shard starts
//! from the same state. Frames are `[u32 len][u8 kind][payload]`, little
//! endian, over `TCP_NODELAY` streams (star topology).

use crate::config::{IzhikevichParams, LIFParams, NetworkConfig, STDPParams};
use crate::runner::{
    NeuronShard, NeuronShardExchange, Runner, ShardPopulation, ShardRowLayout, ShardStepHeader,
    ShardTraffic, shard_cell_range,
};
use crate::sim::{Learning, NeuronModel};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// Protocol tag carried by `HELLO`.
pub const PROTOCOL_MAGIC: u32 = 0x4e53_4831; // "NSH1"
const OUTPUT_POPULATION: u32 = u32::MAX;

const HELLO: u8 = 1;
const INIT: u8 = 2;
const READY: u8 = 3;
const STEP: u8 = 4;
const SLICE: u8 = 5;
const MERGED: u8 = 6;
const DONE: u8 = 7;
const BYE: u8 = 8;
const GATHER: u8 = 9;
const GATHERED: u8 = 10;

/// Environment contract used by the launchers.
pub const ENV_SHARDS: &str = "NM_BODY_SHARDS";
pub const ENV_ADDR: &str = "NM_BODY_SHARD_ADDR";
pub const ENV_BRAIN: &str = "NM_BODY_SHARD_BRAIN";
pub const ENV_TIMEOUT_MS: &str = "NM_BODY_SHARD_TIMEOUT_MS";

/// Shard count requested by `NM_BODY_SHARDS` (> 1 enables a group).
pub fn configured_shard_count() -> Option<usize> {
    std::env::var(ENV_SHARDS)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 1)
}

/// Coordinator listen / follower connect address (`NM_BODY_SHARD_ADDR`).
pub fn configured_addr() -> Option<String> {
    std::env::var(ENV_ADDR)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// Whether the IPC owner of `brain_id` coordinates the configured group.
pub fn coordinates_brain(brain_id: &str) -> bool {
    configured_shard_count().is_some()
        && configured_addr().is_some()
        && std::env::var(ENV_BRAIN)
            .map(|b| b.trim() == brain_id)
            .unwrap_or(true)
}

pub fn configured_timeout() -> Duration {
    Duration::from_millis(
        std::env::var(ENV_TIMEOUT_MS)
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(120_000),
    )
}

// ----------------------------------------------------------------- wire ---

fn send(stream: &mut TcpStream, kind: u8, payload: &[u8], buf: &mut Vec<u8>) -> io::Result<()> {
    buf.clear();
    buf.extend_from_slice(&(payload.len() as u32 + 1).to_le_bytes());
    buf.push(kind);
    buf.extend_from_slice(payload);
    stream.write_all(buf)
}

fn recv(stream: &mut TcpStream, payload: &mut Vec<u8>) -> io::Result<u8> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len == 0 || len > 1 << 30 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad frame length",
        ));
    }
    let mut kind = [0u8; 1];
    stream.read_exact(&mut kind)?;
    payload.resize(len - 1, 0);
    stream.read_exact(payload)?;
    Ok(kind[0])
}

fn expect(kind: u8, want: u8) -> io::Result<()> {
    if kind == want {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("shard protocol: expected message {want}, got {kind}"),
        ))
    }
}

#[derive(Default)]
struct Enc(Vec<u8>);

impl Enc {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f32(&mut self, v: f32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f64(&mut self, v: f64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
        self
    }
    fn i8s(&mut self, v: &[i8]) -> &mut Self {
        self.u32(v.len() as u32);
        self.0.extend(v.iter().map(|x| *x as u8));
        self
    }
    fn f64s(&mut self, v: &[f64]) -> &mut Self {
        self.u32(v.len() as u32);
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
        self
    }
}

struct Dec<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Dec<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, at: 0 }
    }
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len());
        let Some(end) = end else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "short shard frame",
            ));
        };
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn bytes(&mut self) -> io::Result<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    /// Copy an `i8` array into `dst[offset..]`.
    fn i8s_into(&mut self, dst: &mut [i8], offset: usize) -> io::Result<usize> {
        let src = self.bytes()?;
        let end = offset
            .checked_add(src.len())
            .filter(|e| *e <= dst.len())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "spike slice width"))?;
        for (d, s) in dst[offset..end].iter_mut().zip(src) {
            *d = *s as i8;
        }
        Ok(src.len())
    }
    fn i8s(&mut self) -> io::Result<Vec<i8>> {
        Ok(self.bytes()?.iter().map(|b| *b as i8).collect())
    }
    /// Copy an `f64` array into `dst[offset..]`.
    fn f64s_into(&mut self, dst: &mut [f64], offset: usize) -> io::Result<()> {
        let n = self.u32()? as usize;
        let src = self.take(n * 8)?;
        let end = offset
            .checked_add(n)
            .filter(|e| *e <= dst.len())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "field slice width"))?;
        for (d, c) in dst[offset..end].iter_mut().zip(src.chunks_exact(8)) {
            *d = f64::from_le_bytes(c.try_into().unwrap());
        }
        Ok(())
    }
}

fn population_code(p: ShardPopulation) -> u32 {
    match p {
        ShardPopulation::Hidden(l) => l as u32,
        ShardPopulation::Output => OUTPUT_POPULATION,
    }
}

fn protocol_error(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

// ---------------------------------------------------------- parameters ---

/// Runner construction parameters that are not part of a snapshot.
#[derive(serde::Serialize, serde::Deserialize)]
struct RunnerParams {
    lif: LIFParams,
    stdp: STDPParams,
    model: String,
    izh: Option<IzhikevichParams>,
    learning: String,
    feedback_enabled: bool,
}

impl RunnerParams {
    fn of(r: &Runner) -> Self {
        Self {
            lif: r.lif,
            stdp: r.stdp,
            model: r.neuron_model.to_str().to_owned(),
            izh: match r.neuron_model {
                NeuronModel::Izh(p) => Some(p),
                _ => None,
            },
            learning: r.learning.to_str().to_owned(),
            feedback_enabled: r.feedback_enabled,
        }
    }

    fn build(&self, snapshot: &str) -> anyhow::Result<Runner> {
        let model = match (NeuronModel::from_str(&self.model), self.izh) {
            (Some(NeuronModel::Izh(_)), Some(p)) => NeuronModel::Izh(p),
            (Some(m), _) => m,
            (None, _) => anyhow::bail!("unknown neuron model {}", self.model),
        };
        let learning = Learning::from_str(&self.learning)
            .ok_or_else(|| anyhow::anyhow!("unknown learning rule {}", self.learning))?;
        let mut r = Runner::new(
            self.lif,
            self.stdp,
            NetworkConfig::default(),
            model,
            learning,
        );
        r.import_network_json(snapshot)?;
        r.lif = self.lif;
        r.stdp = self.stdp;
        r.feedback_enabled = self.feedback_enabled;
        Ok(r)
    }
}

// ---------------------------------------------------- sparse population ---

/// Encode a hidden population's share: the cells that fired (index, value)
/// and the voltages of `exports` (cells whose voltage another shard reads:
/// graded cells referenced by its rows, and gap-coupled cells).
fn encode_sparse(
    enc: &mut Enc,
    code: u32,
    spikes: &[i8],
    cells: std::ops::Range<usize>,
    v: &[f64],
    exports: &[u32],
) {
    enc.u32(code);
    let fired = spikes[cells.clone()].iter().filter(|s| **s != 0).count();
    enc.u32(fired as u32);
    for i in cells {
        if spikes[i] != 0 {
            enc.u32(i as u32).u8(spikes[i] as u8);
        }
    }
    enc.u32(exports.len() as u32);
    for &i in exports {
        enc.u32(i).f64(v[i as usize]);
    }
}

/// Apply [`encode_sparse`] entries; returns (fired, voltages) pairs read.
fn apply_sparse(
    d: &mut Dec<'_>,
    spikes: &mut [i8],
    v: &mut [f64],
    mut on_entry: impl FnMut(u32, Option<i8>, Option<f64>),
) -> io::Result<()> {
    let n = d.u32()? as usize;
    for _ in 0..n {
        let i = d.u32()?;
        let s = d.u8()? as i8;
        *spikes
            .get_mut(i as usize)
            .ok_or_else(|| protocol_error("spike index out of range"))? = s;
        on_entry(i, Some(s), None);
    }
    let n = d.u32()? as usize;
    for _ in 0..n {
        let i = d.u32()?;
        let x = d.f64()?;
        *v.get_mut(i as usize)
            .ok_or_else(|| protocol_error("voltage index out of range"))? = x;
        on_entry(i, None, Some(x));
    }
    Ok(())
}

/// Clear every entry of `spikes` outside `keep` (before a sparse merge).
fn clear_outside(spikes: &mut [i8], keep: &std::ops::Range<usize>) {
    for (i, s) in spikes.iter_mut().enumerate() {
        if !keep.contains(&i) {
            *s = 0;
        }
    }
}

// ---------------------------------------------------------- coordinator ---

/// Shard 0's transport: drives the followers and merges their cells.
pub struct CoordinatorExchange {
    followers: Vec<TcpStream>,
    /// Per hidden layer, this shard's cells whose voltage others read.
    exports: Vec<Vec<u32>>,
    error: Option<String>,
    frame: Vec<u8>,
    payload: Vec<u8>,
    enc: Enc,
    merged_spikes: Vec<(u32, i8)>,
    merged_v: Vec<(u32, f64)>,
    traffic: ShardTraffic,
}

impl CoordinatorExchange {
    fn fail(&mut self, e: io::Error) {
        if self.error.is_none() {
            let message = format!("neuron shard group lost: {e}");
            crate::nm_err!("[neuron-shard] {message}");
            self.error = Some(message);
        }
    }

    fn broadcast(&mut self, kind: u8) -> io::Result<()> {
        for f in &mut self.followers {
            send(f, kind, &self.enc.0, &mut self.frame)?;
            self.traffic.bytes_sent += self.frame.len() as u64;
        }
        Ok(())
    }

    fn recv_from(&mut self, i: usize, want: u8) -> io::Result<()> {
        expect(recv(&mut self.followers[i], &mut self.payload)?, want)?;
        self.traffic.bytes_received += self.payload.len() as u64 + 5;
        Ok(())
    }

    fn try_exchange(
        &mut self,
        population: ShardPopulation,
        owned: std::ops::Range<usize>,
        spikes: &mut [i8],
        fields: &mut [&mut [f64]],
    ) -> io::Result<()> {
        let code = population_code(population);
        match population {
            ShardPopulation::Hidden(layer) => {
                let [v] = fields else {
                    return Err(protocol_error("hidden merge carries one field"));
                };
                clear_outside(spikes, &owned);
                self.merged_spikes.clear();
                self.merged_v.clear();
                for i in 0..self.followers.len() {
                    self.recv_from(i, SLICE)?;
                    let payload = std::mem::take(&mut self.payload);
                    let mut d = Dec::new(&payload);
                    if d.u32()? != code {
                        return Err(protocol_error("shard slice for another population"));
                    }
                    let (ms, mv) = (&mut self.merged_spikes, &mut self.merged_v);
                    apply_sparse(&mut d, spikes, v, |i, s, x| {
                        if let Some(s) = s {
                            ms.push((i, s));
                        }
                        if let Some(x) = x {
                            mv.push((i, x));
                        }
                    })?;
                    self.payload = payload;
                }
                let exports = self.exports.get(layer).map(Vec::as_slice).unwrap_or(&[]);
                self.enc.0.clear();
                self.enc.u32(code);
                let own_fired = spikes[owned.clone()].iter().filter(|s| **s != 0).count();
                self.enc.u32((own_fired + self.merged_spikes.len()) as u32);
                for i in owned {
                    if spikes[i] != 0 {
                        self.enc.u32(i as u32).u8(spikes[i] as u8);
                    }
                }
                for &(i, s) in &self.merged_spikes {
                    self.enc.u32(i).u8(s as u8);
                }
                self.enc.u32((exports.len() + self.merged_v.len()) as u32);
                for &i in exports {
                    self.enc.u32(i).f64(v[i as usize]);
                }
                for &(i, x) in &self.merged_v {
                    self.enc.u32(i).f64(x);
                }
            }
            ShardPopulation::Output => {
                for i in 0..self.followers.len() {
                    self.recv_from(i, SLICE)?;
                    let mut d = Dec::new(&self.payload);
                    if d.u32()? != code {
                        return Err(protocol_error("shard slice for another population"));
                    }
                    let start = d.u32()? as usize;
                    d.i8s_into(spikes, start)?;
                    if d.u32()? as usize != fields.len() {
                        return Err(protocol_error("shard slice field count"));
                    }
                    for field in fields.iter_mut() {
                        d.f64s_into(field, start)?;
                    }
                }
                self.enc.0.clear();
                self.enc.u32(code).i8s(spikes).u32(fields.len() as u32);
                for field in fields.iter() {
                    self.enc.f64s(field);
                }
            }
        }
        self.broadcast(MERGED)
    }

    fn try_gather(&mut self, v_h: &mut [&mut [f64]], v_o: &mut [f64]) -> io::Result<()> {
        self.enc.0.clear();
        self.broadcast(GATHER)?;
        for i in 0..self.followers.len() {
            self.recv_from(i, GATHERED)?;
            let mut d = Dec::new(&self.payload);
            let layers = d.u32()? as usize;
            if layers != v_h.len() {
                return Err(protocol_error("gathered layer count"));
            }
            for v in v_h.iter_mut() {
                let start = d.u32()? as usize;
                d.f64s_into(v, start)?;
            }
            let start = d.u32()? as usize;
            d.f64s_into(v_o, start)?;
        }
        Ok(())
    }
}

impl NeuronShardExchange for CoordinatorExchange {
    fn begin_step(&mut self, h: &ShardStepHeader<'_>) {
        if self.error.is_some() {
            return;
        }
        self.enc.0.clear();
        self.enc
            .u64(h.t as u64)
            .f64(h.dt)
            .f32(h.reward)
            .u8(u8::from(h.sensory.is_some()))
            .i8s(h.sensory.unwrap_or(&[]))
            .i8s(h.last_spk_o);
        if let Err(e) = self.broadcast(STEP) {
            self.fail(e);
        }
    }

    fn exchange(
        &mut self,
        population: ShardPopulation,
        owned: std::ops::Range<usize>,
        spikes: &mut [i8],
        fields: &mut [&mut [f64]],
    ) {
        if self.error.is_none()
            && let Err(e) = self.try_exchange(population, owned, spikes, fields)
        {
            self.fail(e);
        }
    }

    fn end_step(&mut self, t: usize) {
        if self.error.is_some() {
            return;
        }
        for i in 0..self.followers.len() {
            let result = self.recv_from(i, DONE).and_then(|()| {
                let done = Dec::new(&self.payload).u64()?;
                if done != t as u64 {
                    return Err(protocol_error(format!(
                        "shard {} finished tick {done}, expected {t}",
                        i + 1
                    )));
                }
                Ok(())
            });
            if let Err(e) = result {
                self.fail(e);
                return;
            }
        }
        self.traffic.steps += 1;
    }

    fn error(&self) -> Option<String> {
        self.error.clone()
    }

    fn gather_state(&mut self, v_h: &mut [&mut [f64]], v_o: &mut [f64]) {
        if self.error.is_none()
            && let Err(e) = self.try_gather(v_h, v_o)
        {
            self.fail(e);
        }
    }

    fn traffic(&self) -> ShardTraffic {
        self.traffic
    }
}

impl Drop for CoordinatorExchange {
    fn drop(&mut self) {
        for f in &mut self.followers {
            let _ = send(f, BYE, &[], &mut self.frame);
        }
    }
}

/// How a coordinator hands the network to its followers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShardStorage {
    /// Each shard stores only its own cells' incoming weight rows when the
    /// network is eligible (`Runner::partition_storage_blocker`), else
    /// every shard stores the whole network.
    #[default]
    PartitionedWhenEligible,
    /// Every shard stores the whole network.
    Replicated,
}

/// Cells of `owner`'s range in hidden `layer` whose voltage another shard
/// reads: gap-coupled cells (the implicit electrical solve spans the whole
/// layer) and graded cells with a synapse onto another shard's rows.
fn exported_cells(r: &Runner, count: usize, owner: usize, layer: usize) -> Vec<u32> {
    let n = r.layer_size(layer);
    let own = shard_cell_range(owner, count, n);
    let gap = r.gap_coupled_cells(layer);
    let graded = r.graded_release_h(layer);
    let (_, out_l) = r.get_io_layers();
    let layers = r.net.num_hidden_layers;
    let outputs = r.net.num_output_neurons;
    let output_all = r.net.aarnn_output_gain_floor_enabled;
    // Whether presynaptic `i` has a synapse onto a row of another shard.
    let foreign_rows = |m: &ndarray::Array2<f64>, i: usize, rows: usize| {
        (0..count).filter(|s| *s != owner).any(|s| {
            shard_cell_range(s, count, rows)
                .any(|row| row < m.nrows() && i < m.ncols() && m[(row, i)] != 0.0)
        })
    };
    // The AARNN mean-field/spatial gap-junction term couples each cell to
    // the membranes of the whole layer.
    let field_coupled = matches!(r.neuron_model, crate::sim::NeuronModel::Aarnn)
        && r.net.aarnn_gap_junction_strength > 0.0
        && n > 1;
    own.filter(|&i| {
        if field_coupled || gap.get(i).copied().unwrap_or(false) {
            return true;
        }
        if graded.get(i).is_none_or(|g| g.is_none()) {
            return false;
        }
        let rec = r.w_hh_rec.get(layer).is_some_and(|m| foreign_rows(m, i, n));
        let fwd = layer + 1 < layers
            && r.w_hh_fwd
                .get(layer)
                .is_some_and(|m| foreign_rows(m, i, r.layer_size(layer + 1)));
        let bwd = layer >= 1
            && r.w_hh_bwd
                .get(layer - 1)
                .is_some_and(|m| foreign_rows(m, i, r.layer_size(layer - 1)));
        let out = layer == out_l
            && if output_all {
                count > 1 && (0..outputs).any(|k| r.w_out[(k, i)] != 0.0)
            } else {
                foreign_rows(&r.w_out, i, outputs)
            };
        rec || fwd || bwd || out
    })
    .map(|i| i as u32)
    .collect()
}

fn encode_matrix(enc: &mut Enc, m: &ndarray::Array2<f64>) {
    enc.u32(m.nrows() as u32).u32(m.ncols() as u32);
    match m.as_slice() {
        Some(data) => {
            enc.f64s(data);
        }
        None => {
            let data: Vec<f64> = m.iter().copied().collect();
            enc.f64s(&data);
        }
    }
}

fn decode_matrix(d: &mut Dec<'_>) -> io::Result<ndarray::Array2<f64>> {
    let rows = d.u32()? as usize;
    let cols = d.u32()? as usize;
    let mut data = vec![0.0; rows * cols];
    d.f64s_into(&mut data, 0)?;
    ndarray::Array2::from_shape_vec((rows, cols), data)
        .map_err(|e| protocol_error(format!("matrix shape: {e}")))
}

fn rows_of(m: &ndarray::Array2<f64>, r: &std::ops::Range<usize>) -> ndarray::Array2<f64> {
    m.slice(ndarray::s![
        r.start.min(m.nrows())..r.end.min(m.nrows()),
        ..
    ])
    .to_owned()
}

/// The runner's snapshot with shape-only weight matrices and no presence
/// counters: every shard imports it and receives its weight rows in binary,
/// so all shards start from one exactly shared state.
pub fn skeleton_snapshot_json(runner: &Runner) -> io::Result<String> {
    let mut snap = runner.snapshot();
    snap.w_in.data = Vec::new();
    snap.w_out.data = Vec::new();
    for m in snap
        .w_hh_rec
        .iter_mut()
        .chain(snap.w_hh_fwd.iter_mut())
        .chain(snap.w_hh_bwd.iter_mut())
    {
        m.data = Vec::new();
    }
    snap.p_in = None;
    snap.p_fwd = None;
    snap.p_bwd = None;
    snap.p_rec = None;
    snap.p_out = None;
    serde_json::to_string(&snap).map_err(|e| protocol_error(format!("encode skeleton: {e}")))
}

/// Re-import `runner` from its own skeleton and reinstall its full weights
/// bit-exactly: the state a shard group starts from (JSON does not
/// round-trip every `f64`), used for single-node references.
pub fn canonicalize_runner(runner: &mut Runner) -> io::Result<()> {
    let skeleton = skeleton_snapshot_json(runner)?;
    let params = RunnerParams::of(runner);
    let (w_in, w_rec, w_fwd, w_bwd, w_out) = (
        runner.w_in.clone(),
        runner.w_hh_rec.clone(),
        runner.w_hh_fwd.clone(),
        runner.w_hh_bwd.clone(),
        runner.w_out.clone(),
    );
    let mut fresh = params
        .build(&skeleton)
        .map_err(|e| protocol_error(format!("import skeleton: {e}")))?;
    fresh.w_in = w_in;
    fresh.w_hh_rec = w_rec;
    fresh.w_hh_fwd = w_fwd;
    fresh.w_hh_bwd = w_bwd;
    fresh.w_out = w_out;
    *runner = fresh;
    Ok(())
}

/// Listening endpoint of a coordinator; followers may connect before the
/// group is attached.
pub struct ShardGroupListener {
    listener: TcpListener,
}

/// What `ShardGroupListener::attach` handed out.
#[derive(Debug, Clone, Default)]
pub struct ShardGroupReport {
    /// `None`: partitioned storage; otherwise why every shard is a replica.
    pub replicated_because: Option<String>,
    /// INIT bytes sent to each follower.
    pub init_bytes: Vec<usize>,
}

impl ShardGroupListener {
    pub fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr)?,
        })
    }

    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }

    /// Accept `count - 1` followers, hand each its shard index and the
    /// network, and make `runner` shard 0 of the group.
    pub fn attach(&self, runner: &mut Runner, count: usize, timeout: Duration) -> io::Result<()> {
        self.attach_with(runner, count, timeout, ShardStorage::default())
            .map(|_| ())
    }

    /// [`Self::attach`] with an explicit storage policy. Under partitioned
    /// storage each follower receives a shape-only skeleton snapshot plus
    /// its own incoming weight rows, and the coordinator then keeps only
    /// its own rows too.
    pub fn attach_with(
        &self,
        runner: &mut Runner,
        count: usize,
        timeout: Duration,
        storage: ShardStorage,
    ) -> io::Result<ShardGroupReport> {
        if count < 2 {
            return Err(protocol_error("a shard group needs at least two shards"));
        }
        let deadline = Instant::now() + timeout;
        self.listener.set_nonblocking(true)?;
        let mut followers = Vec::with_capacity(count - 1);
        let mut payload = Vec::new();
        while followers.len() < count - 1 {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_nodelay(true)?;
                    stream.set_read_timeout(Some(timeout))?;
                    expect(recv(&mut stream, &mut payload)?, HELLO)?;
                    if Dec::new(&payload).u32()? != PROTOCOL_MAGIC {
                        return Err(protocol_error("follower speaks another protocol"));
                    }
                    followers.push(stream);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!(
                                "{} of {} neuron-shard followers connected",
                                followers.len(),
                                count - 1
                            ),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => return Err(e),
            }
        }

        // Shard membership decides eligibility and rows; the transport is
        // installed once every follower is initialised.
        runner.layer_range = None;
        runner.neuron_shard = Some(NeuronShard {
            index: 0,
            count,
            exchange: Box::new(NullExchange),
        });
        let replicated_because = match storage {
            ShardStorage::Replicated => Some("replicated storage requested".to_owned()),
            ShardStorage::PartitionedWhenEligible => {
                runner.partition_storage_blocker().map(str::to_owned)
            }
        };
        let layers = runner.net.num_hidden_layers;
        let exports: Vec<Vec<Vec<u32>>> = (0..count)
            .map(|s| {
                (0..layers)
                    .map(|l| exported_cells(runner, count, s, l))
                    .collect()
            })
            .collect();
        let params = serde_json::to_vec(&RunnerParams::of(runner))
            .map_err(|e| protocol_error(format!("encode runner params: {e}")))?;
        let snapshot = if replicated_because.is_some() {
            runner
                .export_network_json()
                .map_err(|e| protocol_error(format!("export snapshot: {e}")))?
        } else {
            // Shape-only weights: each shard receives its rows in binary.
            skeleton_snapshot_json(runner)?
        };
        let (in_l, _) = runner.get_io_layers();
        let mut frame = Vec::new();
        let mut init_bytes = Vec::with_capacity(count - 1);
        for (i, f) in followers.iter_mut().enumerate() {
            let index = i + 1;
            let mut enc = Enc::default();
            enc.u32(index as u32)
                .u32(count as u32)
                .bytes(&params)
                .bytes(snapshot.as_bytes())
                .u8(u8::from(replicated_because.is_none()));
            if replicated_because.is_none() {
                let rows: Vec<_> = (0..layers)
                    .map(|l| shard_cell_range(index, count, runner.layer_size(l)))
                    .collect();
                let out_rows = if runner.net.aarnn_output_gain_floor_enabled {
                    0..runner.net.num_output_neurons
                } else {
                    shard_cell_range(index, count, runner.net.num_output_neurons)
                };
                for r in rows.iter().chain(std::iter::once(&out_rows)) {
                    enc.u32(r.start as u32).u32(r.end as u32);
                }
                let in_rows = rows.get(in_l).cloned().unwrap_or(0..runner.w_in.nrows());
                encode_matrix(&mut enc, &rows_of(&runner.w_in, &in_rows));
                for (l, m) in runner.w_hh_rec.iter().enumerate() {
                    encode_matrix(&mut enc, &rows_of(m, &rows[l]));
                }
                for (l, m) in runner.w_hh_fwd.iter().enumerate() {
                    encode_matrix(&mut enc, &rows_of(m, &rows[l + 1]));
                }
                for (l, m) in runner.w_hh_bwd.iter().enumerate() {
                    encode_matrix(&mut enc, &rows_of(m, &rows[l]));
                }
                encode_matrix(&mut enc, &rows_of(&runner.w_out, &out_rows));
            }
            enc.u32(layers as u32);
            for cells in &exports[index] {
                enc.u32(cells.len() as u32);
                for c in cells {
                    enc.u32(*c);
                }
            }
            send(f, INIT, &enc.0, &mut frame)?;
            init_bytes.push(frame.len());
        }
        for f in &mut followers {
            expect(recv(f, &mut payload)?, READY)?;
            // Steady state is paced by the body; a stalled shard is a fault.
            f.set_read_timeout(Some(timeout))?;
        }
        // Start shard 0 from exactly the followers' state: the same snapshot
        // import, and under partitioned storage its own rows bit-exactly.
        let own_rows = if replicated_because.is_none() {
            let rows: Vec<_> = (0..layers)
                .map(|l| shard_cell_range(0, count, runner.layer_size(l)))
                .collect();
            let layout = runner
                .shard_row_layout()
                .ok_or_else(|| protocol_error("no layout"))?;
            let in_rows = rows.get(in_l).cloned().unwrap_or(0..runner.w_in.nrows());
            Some((
                layout,
                rows_of(&runner.w_in, &in_rows),
                runner
                    .w_hh_rec
                    .iter()
                    .enumerate()
                    .map(|(l, m)| rows_of(m, &rows[l]))
                    .collect::<Vec<_>>(),
                runner
                    .w_hh_fwd
                    .iter()
                    .enumerate()
                    .map(|(l, m)| rows_of(m, &rows[l + 1]))
                    .collect::<Vec<_>>(),
                runner
                    .w_hh_bwd
                    .iter()
                    .enumerate()
                    .map(|(l, m)| rows_of(m, &rows[l]))
                    .collect::<Vec<_>>(),
            ))
        } else {
            None
        };
        let w_out_rows = runner
            .shard_row_layout()
            .map(|l| rows_of(&runner.w_out, &l.output));
        let params_now = RunnerParams::of(runner);
        runner
            .import_network_json(&snapshot)
            .map_err(|e| protocol_error(format!("re-import shard 0: {e}")))?;
        runner.lif = params_now.lif;
        runner.stdp = params_now.stdp;
        runner.feedback_enabled = params_now.feedback_enabled;
        runner.layer_range = None;
        if let (Some((layout, w_in, w_rec, w_fwd, w_bwd)), Some(w_out)) = (own_rows, w_out_rows) {
            runner
                .install_shard_rows(layout, w_in, w_rec, w_fwd, w_bwd, w_out)
                .map_err(|e| protocol_error(format!("partition coordinator storage: {e}")))?;
        }
        runner.neuron_shard = Some(NeuronShard {
            index: 0,
            count,
            exchange: Box::new(CoordinatorExchange {
                followers,
                exports: exports.into_iter().next().unwrap_or_default(),
                error: None,
                frame,
                payload,
                enc: Enc::default(),
                merged_spikes: Vec::new(),
                merged_v: Vec::new(),
                traffic: ShardTraffic::default(),
            }),
        });
        Ok(ShardGroupReport {
            replicated_because,
            init_bytes,
        })
    }
}

/// Placeholder transport while a group is being formed.
struct NullExchange;

impl NeuronShardExchange for NullExchange {
    fn begin_step(&mut self, _header: &ShardStepHeader<'_>) {}
    fn exchange(
        &mut self,
        _population: ShardPopulation,
        _owned: std::ops::Range<usize>,
        _spikes: &mut [i8],
        _fields: &mut [&mut [f64]],
    ) {
    }
    fn end_step(&mut self, _t: usize) {}
    fn error(&self) -> Option<String> {
        Some("shard group is not formed".to_owned())
    }
}

// ------------------------------------------------------------ follower ---

/// Transport of shards `1..count`.
pub struct FollowerExchange {
    stream: TcpStream,
    exports: Vec<Vec<u32>>,
    error: Option<String>,
    frame: Vec<u8>,
    payload: Vec<u8>,
    enc: Enc,
    traffic: ShardTraffic,
}

impl FollowerExchange {
    fn fail(&mut self, e: io::Error) {
        if self.error.is_none() {
            self.error = Some(format!("neuron shard coordinator lost: {e}"));
        }
    }

    fn send(&mut self, kind: u8) -> io::Result<()> {
        send(&mut self.stream, kind, &self.enc.0, &mut self.frame)?;
        self.traffic.bytes_sent += self.frame.len() as u64;
        Ok(())
    }

    fn try_exchange(
        &mut self,
        population: ShardPopulation,
        owned: std::ops::Range<usize>,
        spikes: &mut [i8],
        fields: &mut [&mut [f64]],
    ) -> io::Result<()> {
        let code = population_code(population);
        let owned = owned.start.min(spikes.len())..owned.end.min(spikes.len());
        self.enc.0.clear();
        match population {
            ShardPopulation::Hidden(layer) => {
                let [v] = fields else {
                    return Err(protocol_error("hidden merge carries one field"));
                };
                let exports = self.exports.get(layer).map(Vec::as_slice).unwrap_or(&[]);
                encode_sparse(&mut self.enc, code, spikes, owned.clone(), v, exports);
                self.send(SLICE)?;
                expect(recv(&mut self.stream, &mut self.payload)?, MERGED)?;
                self.traffic.bytes_received += self.payload.len() as u64 + 5;
                let mut d = Dec::new(&self.payload);
                if d.u32()? != code {
                    return Err(protocol_error("merged population mismatch"));
                }
                clear_outside(spikes, &owned);
                apply_sparse(&mut d, spikes, v, |_, _, _| {})?;
            }
            ShardPopulation::Output => {
                self.enc
                    .u32(code)
                    .u32(owned.start as u32)
                    .i8s(&spikes[owned.clone()])
                    .u32(fields.len() as u32);
                for field in fields.iter() {
                    self.enc.f64s(&field[owned.clone()]);
                }
                self.send(SLICE)?;
                expect(recv(&mut self.stream, &mut self.payload)?, MERGED)?;
                self.traffic.bytes_received += self.payload.len() as u64 + 5;
                let mut d = Dec::new(&self.payload);
                if d.u32()? != code {
                    return Err(protocol_error("merged population mismatch"));
                }
                if d.i8s_into(spikes, 0)? != spikes.len() {
                    return Err(protocol_error("merged spike width"));
                }
                if d.u32()? as usize != fields.len() {
                    return Err(protocol_error("merged field count"));
                }
                for field in fields.iter_mut() {
                    d.f64s_into(field, 0)?;
                }
            }
        }
        Ok(())
    }
}

impl NeuronShardExchange for FollowerExchange {
    fn begin_step(&mut self, _header: &ShardStepHeader<'_>) {}

    fn exchange(
        &mut self,
        population: ShardPopulation,
        owned: std::ops::Range<usize>,
        spikes: &mut [i8],
        fields: &mut [&mut [f64]],
    ) {
        if self.error.is_none()
            && let Err(e) = self.try_exchange(population, owned, spikes, fields)
        {
            self.fail(e);
        }
    }

    fn end_step(&mut self, t: usize) {
        if self.error.is_some() {
            return;
        }
        self.enc.0.clear();
        self.enc.u64(t as u64);
        if let Err(e) = self.send(DONE) {
            self.fail(e);
        }
        self.traffic.steps += 1;
    }

    fn error(&self) -> Option<String> {
        self.error.clone()
    }

    fn traffic(&self) -> ShardTraffic {
        self.traffic
    }
}

fn connect_with_retry(addr: &str, timeout: Duration) -> io::Result<TcpStream> {
    let deadline = Instant::now() + timeout;
    loop {
        match TcpStream::connect(addr) {
            Ok(s) => return Ok(s),
            Err(e) if Instant::now() < deadline => {
                let _ = e;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Build a follower's runner from an `INIT` payload.
fn follower_runner(d: &mut Dec<'_>, stream: &TcpStream) -> io::Result<Runner> {
    let index = d.u32()? as usize;
    let count = d.u32()? as usize;
    let params: RunnerParams = serde_json::from_slice(d.bytes()?)
        .map_err(|e| protocol_error(format!("runner params: {e}")))?;
    let snapshot =
        std::str::from_utf8(d.bytes()?).map_err(|e| protocol_error(format!("snapshot: {e}")))?;
    let mut r = params
        .build(snapshot)
        .map_err(|e| protocol_error(format!("import snapshot: {e}")))?;
    r.layer_range = None;
    r.neuron_shard = Some(NeuronShard {
        index,
        count,
        exchange: Box::new(NullExchange),
    });
    if d.u8()? != 0 {
        let layers = r.net.num_hidden_layers;
        let mut ranges = Vec::with_capacity(layers + 1);
        for _ in 0..=layers {
            let start = d.u32()? as usize;
            let end = d.u32()? as usize;
            ranges.push(start..end);
        }
        let output = ranges.pop().unwrap_or(0..0);
        let w_in = decode_matrix(d)?;
        let w_rec = (0..r.w_hh_rec.len())
            .map(|_| decode_matrix(d))
            .collect::<io::Result<Vec<_>>>()?;
        let w_fwd = (0..r.w_hh_fwd.len())
            .map(|_| decode_matrix(d))
            .collect::<io::Result<Vec<_>>>()?;
        let w_bwd = (0..r.w_hh_bwd.len())
            .map(|_| decode_matrix(d))
            .collect::<io::Result<Vec<_>>>()?;
        let w_out = decode_matrix(d)?;
        let layout = ShardRowLayout {
            hidden: ranges,
            output,
        };
        if Some(&layout) != r.shard_row_layout().as_ref() {
            return Err(protocol_error("coordinator and follower disagree on rows"));
        }
        r.install_shard_rows(layout, w_in, w_rec, w_fwd, w_bwd, w_out)
            .map_err(protocol_error)?;
    }
    let layers = d.u32()? as usize;
    let mut exports = Vec::with_capacity(layers);
    for _ in 0..layers {
        let n = d.u32()? as usize;
        exports.push((0..n).map(|_| d.u32()).collect::<io::Result<Vec<_>>>()?);
    }
    r.neuron_shard = Some(NeuronShard {
        index,
        count,
        exchange: Box::new(FollowerExchange {
            stream: stream.try_clone()?,
            exports,
            error: None,
            frame: Vec::new(),
            payload: Vec::new(),
            enc: Enc::default(),
            traffic: ShardTraffic::default(),
        }),
    });
    crate::nm_log!(
        "[neuron-shard] follower {index}/{count} ready: {} hidden layer(s), {} outputs, {} weight bytes ({})",
        r.net.num_hidden_layers,
        r.net.num_output_neurons,
        r.resident_weight_bytes(),
        if r.shard_rows.is_some() {
            "own rows"
        } else {
            "replica"
        }
    );
    Ok(r)
}

/// Serve as a follower shard of the coordinator at `addr` until it closes
/// the group. Each `INIT` (re)builds the runner from the coordinator's
/// skeleton and this shard's weight rows; each `STEP` runs one lock-step
/// transition.
pub fn run_follower(addr: &str, connect_timeout: Duration) -> io::Result<()> {
    let mut stream = connect_with_retry(addr, connect_timeout)?;
    stream.set_nodelay(true)?;
    let mut frame = Vec::new();
    let mut hello = Enc::default();
    hello.u32(PROTOCOL_MAGIC);
    send(&mut stream, HELLO, &hello.0, &mut frame)?;
    let mut runner: Option<Runner> = None;
    let mut payload = Vec::new();
    let mut sensory = Vec::new();
    loop {
        let kind = match recv(&mut stream, &mut payload) {
            Ok(kind) => kind,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };
        match kind {
            INIT => {
                // Release the previous runner before building the next.
                runner = None;
                let mut d = Dec::new(&payload);
                runner = Some(follower_runner(&mut d, &stream)?);
                send(&mut stream, READY, &[], &mut frame)?;
            }
            STEP => {
                let r = runner
                    .as_mut()
                    .ok_or_else(|| protocol_error("STEP before INIT"))?;
                let mut d = Dec::new(&payload);
                let t = d.u64()? as usize;
                let dt = d.f64()?;
                let reward = d.f32()?;
                let has_sensory = d.u8()? != 0;
                sensory.clear();
                sensory.extend(d.i8s()?);
                let last_spk_o = d.i8s()?;
                if t != r.t {
                    return Err(protocol_error(format!(
                        "coordinator at tick {t}, follower at {}",
                        r.t
                    )));
                }
                r.set_dt(dt);
                r.external_reward = reward;
                if let Some(dst) = r.last_spk_o.as_slice_mut()
                    && dst.len() == last_spk_o.len()
                {
                    dst.copy_from_slice(&last_spk_o);
                }
                r.step(has_sensory.then_some(sensory.as_slice()));
                if let Some(e) = r.neuron_shard_error() {
                    return Err(protocol_error(e));
                }
            }
            GATHER => {
                let r = runner
                    .as_ref()
                    .ok_or_else(|| protocol_error("GATHER before INIT"))?;
                let shard = r.neuron_shard.as_ref().expect("initialised shard");
                let mut enc = Enc::default();
                enc.u32(r.v_h.len() as u32);
                for v in &r.v_h {
                    let own = shard_cell_range(shard.index, shard.count, v.len());
                    let data: Vec<f64> =
                        v.iter().skip(own.start).take(own.len()).copied().collect();
                    enc.u32(own.start as u32).f64s(&data);
                }
                let own = r
                    .shard_row_layout()
                    .map(|l| l.output)
                    .unwrap_or(0..r.v_o.len());
                let data: Vec<f64> = r
                    .v_o
                    .iter()
                    .skip(own.start)
                    .take(own.len())
                    .copied()
                    .collect();
                enc.u32(own.start as u32).f64s(&data);
                send(&mut stream, GATHERED, &enc.0, &mut frame)?;
            }
            BYE => return Ok(()),
            other => return Err(protocol_error(format!("unexpected message {other}"))),
        }
    }
}

/// Run a follower for the configured group in a background thread
/// (`NM_BODY_SHARDS` > 1 and `NM_BODY_SHARD_ADDR`). Returns `false` when no
/// group is configured.
pub fn spawn_configured_follower(node_id: &str) -> bool {
    let (Some(count), Some(addr)) = (configured_shard_count(), configured_addr()) else {
        return false;
    };
    let node_id = node_id.to_owned();
    let timeout = configured_timeout();
    std::thread::Builder::new()
        .name("nm-neuron-shard".into())
        .spawn(move || {
            // A coordinator re-forms the group (new INIT) after topology or
            // configuration changes; rejoin until it is gone.
            loop {
                crate::nm_log!(
                    "[neuron-shard] node {node_id} joining a {count}-shard group at {addr}"
                );
                match run_follower(&addr, timeout) {
                    Ok(()) => crate::nm_log!("[neuron-shard] node {node_id}: group closed"),
                    Err(e) => {
                        crate::nm_err!("[neuron-shard] node {node_id} follower stopped: {e}");
                        break;
                    }
                }
            }
        })
        .is_ok()
}

// ------------------------------------------------------------ scaling ---

/// A frozen one-layer network of `cells` spiking cells with sparse random
/// recurrent, input and output synapses (deterministic for `seed`).
pub fn scaling_network(cells: usize, density: f64, seed: u64) -> Runner {
    use crate::config::ClumpingDesign;
    let mut net = NetworkConfig::default();
    net.clumping_design = ClumpingDesign::None;
    net.brain_regions = Vec::new();
    net.num_sensory_neurons = 64;
    net.num_hidden_layers = 1;
    net.num_hidden_per_layer_initial = cells;
    net.num_output_neurons = 32;
    net.growth_enabled = false;
    net.morpho_growth_enabled = false;
    net.use_morphology = false;
    net.use_aarnn_delays = false;
    net.thalamic_gating_enabled = false;
    net.aarnn_synaptic_energy_randomness = 0.0;
    net.perceptual_loop_enabled = false;
    net.aarnn_resonance_gain = 0.0;
    net.aarnn_ambient_energy_level = 0.0;
    net.theta_rhythm_enabled = false;
    net.aarnn_gap_junction_strength = 0.0;
    net.volume_transmission_enabled = false;
    net.aarnn_output_gain_floor_enabled = false;
    net.aarnn_layer_depth = 3;
    net.p_hidden = 0.0;
    net.p_in = 0.0;
    net.p_out = 0.0;
    fastrand::seed(seed);
    let mut r = Runner::new(
        LIFParams::default(),
        STDPParams::default(),
        net,
        NeuronModel::Aarnn,
        Learning::Aarnn,
    );
    r.set_dt(1.0);
    r.stdp.eta = 0.0;
    r.net.aarnn_plasticity_enabled = false;
    #[cfg(feature = "growth3d")]
    for layer in r.topo.layers.iter_mut() {
        for node in layer.iter_mut() {
            node.type_name = None;
        }
    }
    r.net.aarnn_bio.tonic_current = 5.0;
    #[cfg(feature = "growth3d")]
    r.sync_bio_from_topo();
    let mut rng = fastrand::Rng::with_seed(seed ^ 0x9e37_79b9_7f4a_7c15);
    let mut fill = |m: &mut ndarray::Array2<f64>, density: f64, gain: f64, inhibitory: f64| {
        for w in m.iter_mut() {
            *w = if rng.f64() < density {
                let x = gain * (0.25 + rng.f64());
                if rng.f64() < inhibitory { -x } else { x }
            } else {
                0.0
            };
        }
    };
    let fan_in = (density * cells as f64).max(1.0);
    fill(&mut r.w_in, 0.2, 60.0, 0.0);
    fill(&mut r.w_hh_rec[0], density, 400.0 / fan_in, 0.2);
    fill(&mut r.w_out, 0.1, 40.0, 0.0);
    r
}

/// One row of [`run_scaling_bench`].
#[derive(Debug, Clone)]
pub struct ScalingRow {
    pub shards: usize,
    pub storage: String,
    /// Resident weight bytes held by shard 0 (each shard holds about the same).
    pub weight_bytes_per_shard: usize,
    pub state_bytes_per_shard: usize,
    pub init_bytes_per_follower: usize,
    pub ms_per_step: f64,
    pub bytes_per_step: f64,
    pub bit_identical: bool,
    pub spikes: usize,
}

/// Open-loop scaling of a neuron-shard group on loopback: for each shard
/// count, form the group from a fresh copy of [`scaling_network`], step it
/// `steps` times next to a single node and record per-shard memory, step
/// time and transport bytes; every step is checked for bit-identity.
pub fn run_scaling_bench(
    cells: usize,
    density: f64,
    steps: usize,
    counts: &[usize],
) -> io::Result<Vec<ScalingRow>> {
    let input = |step: usize, width: usize| -> Vec<i8> {
        (0..width)
            .map(|i| i8::from((step / 20 + i) % 4 == 0))
            .collect()
    };
    let mut rows = Vec::new();
    let mut single = scaling_network(cells, density, 7);
    canonicalize_runner(&mut single)?;
    let width = single.net.num_sensory_neurons;
    let mut reference = Vec::with_capacity(steps);
    let started = Instant::now();
    let mut spikes = 0usize;
    for step in 0..steps {
        let out = single.step(Some(&input(step, width)));
        spikes += single.last_spk_h[0].iter().filter(|s| **s != 0).count();
        reference.push((
            out.spk_o.to_vec(),
            single.last_spk_h[0].to_vec(),
            single.v_h[0].to_vec(),
        ));
    }
    rows.push(ScalingRow {
        shards: 1,
        storage: "single node".into(),
        weight_bytes_per_shard: single.resident_weight_bytes(),
        state_bytes_per_shard: single.resident_state_bytes(),
        init_bytes_per_follower: 0,
        ms_per_step: started.elapsed().as_secs_f64() * 1000.0 / steps as f64,
        bytes_per_step: 0.0,
        bit_identical: true,
        spikes,
    });
    drop(single);
    for &count in counts.iter().filter(|c| **c > 1) {
        let listener = ShardGroupListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?.to_string();
        let followers: Vec<_> = (1..count)
            .map(|_| {
                let addr = addr.clone();
                std::thread::spawn(move || run_follower(&addr, Duration::from_secs(60)))
            })
            .collect();
        let mut coordinator = scaling_network(cells, density, 7);
        let report = listener.attach_with(
            &mut coordinator,
            count,
            Duration::from_secs(120),
            ShardStorage::default(),
        )?;
        let mut identical = true;
        let mut elapsed = Duration::ZERO;
        let mut gathered_bytes = 0u64;
        for (step, (spk_o, spk_h, v_h)) in reference.iter().enumerate() {
            let t0 = Instant::now();
            let out = coordinator.step(Some(&input(step, width)));
            elapsed += t0.elapsed();
            let before = coordinator.neuron_shard_traffic().unwrap_or_default();
            coordinator.gather_neuron_shard_state();
            let after = coordinator.neuron_shard_traffic().unwrap_or_default();
            gathered_bytes += (after.bytes_sent + after.bytes_received)
                - (before.bytes_sent + before.bytes_received);
            identical &= out.spk_o.to_vec() == *spk_o
                && coordinator.last_spk_h[0].to_vec() == *spk_h
                && coordinator.v_h[0].to_vec() == *v_h;
            if let Some(e) = coordinator.neuron_shard_error() {
                return Err(protocol_error(e));
            }
        }
        let traffic = coordinator.neuron_shard_traffic().unwrap_or_default();
        rows.push(ScalingRow {
            shards: count,
            storage: report
                .replicated_because
                .clone()
                .map_or_else(|| "own rows".to_owned(), |r| format!("replica ({r})")),
            weight_bytes_per_shard: coordinator.resident_weight_bytes(),
            state_bytes_per_shard: coordinator.resident_state_bytes(),
            init_bytes_per_follower: report.init_bytes.iter().copied().max().unwrap_or(0),
            ms_per_step: elapsed.as_secs_f64() * 1000.0 / steps as f64,
            bytes_per_step: ((traffic.bytes_sent + traffic.bytes_received - gathered_bytes) as f64)
                / steps as f64,
            bit_identical: identical,
            spikes,
        });
        drop(coordinator);
        for f in followers {
            f.join()
                .map_err(|_| protocol_error("follower thread panicked"))??;
        }
    }
    Ok(rows)
}

#[cfg(all(test, feature = "growth3d"))]
mod tests {
    use super::*;
    use crate::layer_shard::tests::{
        CASES, Features, PLAIN, runner_from, sensory_frame, snapshot_with_layers,
    };

    /// Group of `count` neuron shards on loopback; returns the coordinator,
    /// the follower threads and what was handed out.
    fn group(
        json: &str,
        count: usize,
        storage: ShardStorage,
    ) -> (
        Runner,
        Vec<std::thread::JoinHandle<io::Result<()>>>,
        ShardGroupReport,
    ) {
        let listener = ShardGroupListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let followers = (1..count)
            .map(|_| {
                let addr = addr.clone();
                std::thread::spawn(move || run_follower(&addr, Duration::from_secs(60)))
            })
            .collect();
        let mut coordinator = runner_from(json);
        let report = listener
            .attach_with(&mut coordinator, count, Duration::from_secs(180), storage)
            .unwrap();
        (coordinator, followers, report)
    }

    /// Step a single node and a shard group side by side; after every step
    /// gather each shard's own membranes and require bit-identity of all
    /// spikes and membranes. Returns the group's traffic and whether the
    /// group was partition-stored.
    fn assert_group_matches(
        name: &str,
        json: &str,
        count: usize,
        steps: usize,
        input: impl Fn(usize, usize) -> Vec<i8>,
    ) -> (ShardTraffic, bool, usize) {
        let mut single = runner_from(json);
        let full_weights = single.resident_weight_bytes();
        let (mut coordinator, followers, report) =
            group(json, count, ShardStorage::PartitionedWhenEligible);
        let partitioned = report.replicated_because.is_none();
        if partitioned {
            // Shard 0 keeps about 1/count of the weight rows.
            let own = coordinator.resident_weight_bytes();
            assert!(
                own * count <= full_weights + full_weights / 4 + 4096,
                "{name}: shard 0 holds {own} of {full_weights} weight bytes"
            );
        }
        let width = single.net.num_sensory_neurons;
        let mut gathered = 0u64;
        for step in 0..steps {
            let s = input(step, width);
            let a = single.step(Some(&s));
            let b = coordinator.step(Some(&s));
            let before = coordinator.neuron_shard_traffic().unwrap_or_default();
            coordinator.gather_neuron_shard_state();
            let after = coordinator.neuron_shard_traffic().unwrap_or_default();
            gathered += (after.bytes_sent + after.bytes_received)
                - (before.bytes_sent + before.bytes_received);
            assert!(
                coordinator.neuron_shard_error().is_none(),
                "{name}: transport"
            );
            assert!(
                a.spk_o == b.spk_o
                    && single.last_spk_h == coordinator.last_spk_h
                    && single.v_h == coordinator.v_h
                    && single.v_o == coordinator.v_o,
                "{name}: {count} neuron shards diverged at step {step} (partitioned={partitioned}): spk_o {} spk_h {} v_h {:?} v_o {:?}",
                a.spk_o == b.spk_o,
                single.last_spk_h == coordinator.last_spk_h,
                single
                    .v_h
                    .iter()
                    .zip(&coordinator.v_h)
                    .map(|(x, y)| x
                        .iter()
                        .zip(y.iter())
                        .enumerate()
                        .filter(|(_, (p, q))| p != q)
                        .map(|(j, (p, q))| (j, *p, *q))
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
                single
                    .v_o
                    .iter()
                    .zip(coordinator.v_o.iter())
                    .enumerate()
                    .filter(|(_, (p, q))| p != q)
                    .map(|(k, _)| k)
                    .collect::<Vec<_>>()
            );
        }
        // Exclude the verification gathers from the reported volume.
        let mut traffic = coordinator.neuron_shard_traffic().unwrap();
        traffic.bytes_received = traffic.bytes_received.saturating_sub(gathered);
        let own_weights = coordinator.resident_weight_bytes();
        drop(coordinator);
        for f in followers {
            f.join().unwrap().unwrap();
        }
        (traffic, partitioned, own_weights)
    }

    fn assert_case(name: &str, f: Features, depth: usize, layers: usize) {
        let json = snapshot_with_layers(f, depth, layers);
        for count in [2usize, 3] {
            let (_, partitioned, _) =
                assert_group_matches(name, &json, count, 150, |step, _| sensory_frame(step));
            assert_eq!(
                partitioned, !f.delays,
                "{name}: partitioned storage covers frozen non-morphology networks"
            );
        }
    }

    /// One-layer circuits (the designed reflex/CPG circuits and connectome
    /// imports) split into 2 and 3 neuron-range shards over TCP reproduce
    /// single-node execution bit for bit, with each shard storing only its
    /// own cells' incoming weight rows and exchanging fired cells plus the
    /// voltages other shards read: recurrent, graded, tonic and gap junction
    /// coupling, stochastic drives and the pulse-density output code. The
    /// morphology case keeps replicated storage.
    #[test]
    fn neuron_shards_reproduce_single_layer_circuits() {
        for depth in [0usize, 3] {
            for (name, f) in CASES {
                if f.biological_io {
                    continue;
                }
                assert_case(name, f, depth, 1);
            }
        }
    }

    /// The same cell partition applies to every layer of a deeper network.
    #[test]
    fn neuron_shards_reproduce_multi_layer_networks() {
        for (name, f) in CASES {
            assert_case(name, f, 3, 3);
        }
    }

    /// The repository's designed one-layer circuits (hexapod reflex,
    /// zebrafish swim CPG) and the worm connectome shard bit-identically
    /// from their shipped snapshots.
    #[test]
    fn designed_circuits_shard_bit_identically() {
        for (file, steps) in [
            ("network_hexapod_reflex.json", 400),
            ("network_zebrafish_swim.json", 400),
            // Connectome import: morphology delays, gap junctions, stochastic
            // release and synaptic-energy drive (replicated storage).
            ("network_celegans_frozen_bbc5.json", 120),
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
            let Ok(json) = std::fs::read_to_string(&path) else {
                continue;
            };
            for count in [2usize, 3] {
                let (traffic, partitioned, own_weights) =
                    assert_group_matches(file, &json, count, steps, |step, width| {
                        (0..width)
                            .map(|i| i8::from((step / 25 + i) % 3 == 0))
                            .collect()
                    });
                assert_eq!(partitioned, !file.contains("celegans"), "{file}");
                assert!(traffic.steps as usize == steps);
                eprintln!(
                    "[neuron-shard] {file} x{count}: shard-0 weight bytes {own_weights}, {:.0} transport bytes/step (partitioned={partitioned})",
                    (traffic.bytes_sent + traffic.bytes_received) as f64 / steps as f64
                );
            }
        }
    }

    #[test]
    fn follower_failure_is_reported_not_hidden() {
        let json = snapshot_with_layers(PLAIN, 0, 1);
        let listener = ShardGroupListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let follower = std::thread::spawn(move || {
            let mut s = connect_with_retry(&addr, Duration::from_secs(5)).unwrap();
            let mut frame = Vec::new();
            let mut hello = Enc::default();
            hello.u32(PROTOCOL_MAGIC);
            send(&mut s, HELLO, &hello.0, &mut frame).unwrap();
            let mut payload = Vec::new();
            assert_eq!(recv(&mut s, &mut payload).unwrap(), INIT);
            send(&mut s, READY, &[], &mut frame).unwrap();
            // Drop the connection instead of stepping.
        });
        let mut coordinator = runner_from(&json);
        listener
            .attach(&mut coordinator, 2, Duration::from_secs(10))
            .unwrap();
        follower.join().unwrap();
        coordinator.step(Some(&sensory_frame(0)));
        assert!(coordinator.neuron_shard_error().is_some());
    }

    /// The scaling benchmark's large-network path (parallel kernels above
    /// the 96-cell threshold) is bit-identical and partition-stored.
    #[test]
    fn scaling_bench_is_bit_identical() {
        let rows = run_scaling_bench(240, 0.1, 40, &[2, 4]).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].spikes > 0, "{rows:?}");
        for r in &rows[1..] {
            assert!(r.bit_identical && r.storage == "own rows", "{r:?}");
            assert!(
                r.weight_bytes_per_shard * r.shards <= rows[0].weight_bytes_per_shard + 64 * 1024,
                "{r:?} vs {:?}",
                rows[0]
            );
        }
    }

    #[test]
    fn cell_ranges_partition_every_population() {
        for n in [0usize, 1, 7, 24, 55] {
            for count in 1..5 {
                let ranges: Vec<_> = (0..count).map(|i| shard_cell_range(i, count, n)).collect();
                assert_eq!(ranges[0].start, 0);
                assert_eq!(ranges[count - 1].end, n);
                for w in ranges.windows(2) {
                    assert_eq!(w[0].end, w[1].start);
                }
            }
        }
        assert_eq!(shard_cell_range(0, 3, 24), 0..8);
        assert_eq!(shard_cell_range(2, 3, 55), 37..55);
    }
}
