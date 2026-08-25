use aarnn_rust::config::{LIFParams, NetworkConfig, STDPParams};
use aarnn_rust::network::build_network;
use aarnn_rust::sim::{Learning, NeuronModel, run_snn};
use ndarray::Array2;
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct StandaloneFixture {
    seed: u64,
    duration_ms: f64,
    sensory_spikes: Vec<Vec<i8>>,
    network: NetworkConfig,
    expected_digest: String,
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut digest = 0xcbf29ce484222325u64;
    for byte in bytes {
        digest ^= u64::from(*byte);
        digest = digest.wrapping_mul(0x100000001b3);
    }
    digest
}

fn append_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn append_f64(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
}

fn append_i8_array(bytes: &mut Vec<u8>, values: &[i8]) {
    bytes.extend(values.iter().map(|value| *value as u8));
}

fn append_f64_array(bytes: &mut Vec<u8>, values: impl IntoIterator<Item = f64>) {
    for value in values {
        append_f64(bytes, value);
    }
}

fn standalone_digest(fixture: &StandaloneFixture) -> String {
    let sensory = Array2::from_shape_vec(
        (
            fixture.sensory_spikes.len(),
            fixture.network.num_sensory_neurons,
        ),
        fixture.sensory_spikes.iter().flatten().copied().collect(),
    )
    .expect("fixture sensory raster shape");

    let mut rng = StdRng::seed_from_u64(fixture.seed);
    let built = build_network(&fixture.network, &mut rng);
    let result = run_snn(
        fixture.duration_ms,
        &LIFParams::default(),
        &STDPParams::default(),
        &fixture.network,
        built,
        &sensory,
        NeuronModel::Lif,
        Learning::Stdp,
    );

    let mut bytes = Vec::new();
    append_u64(&mut bytes, fixture.seed);
    append_f64(&mut bytes, fixture.duration_ms);
    for spikes in &result.spikes_h {
        append_u64(&mut bytes, spikes.len() as u64);
        append_i8_array(
            &mut bytes,
            spikes.as_slice().expect("contiguous hidden spikes"),
        );
    }
    append_u64(&mut bytes, result.spikes_o.len() as u64);
    append_i8_array(
        &mut bytes,
        result
            .spikes_o
            .as_slice()
            .expect("contiguous output spikes"),
    );
    append_u64(&mut bytes, result.longterm_conn as u64);
    append_u64(&mut bytes, result.total_conn as u64);
    append_f64_array(&mut bytes, result.weights.w_in.iter().copied());
    for matrix in result.weights.w_hh_fwd {
        append_f64_array(&mut bytes, matrix.iter().copied());
    }
    for matrix in result.weights.w_hh_bwd {
        append_f64_array(&mut bytes, matrix.iter().copied());
    }
    append_f64_array(&mut bytes, result.weights.w_out.iter().copied());
    format!("{:016x}", fnv1a64(&bytes))
}

#[test]
fn standalone_fixture_has_a_reproducible_reference_digest() {
    let fixture: StandaloneFixture =
        serde_json::from_str(include_str!("fixtures/phase0_standalone.json"))
            .expect("valid standalone Phase 0 fixture");
    assert_eq!(fixture.sensory_spikes.len(), 6);
    assert!(fixture.sensory_spikes.iter().all(|row| row.len() == 4));

    let first = standalone_digest(&fixture);
    let second = standalone_digest(&fixture);
    assert_eq!(first, second, "seeded standalone run is not reproducible");
    assert_eq!(first, fixture.expected_digest);
}
