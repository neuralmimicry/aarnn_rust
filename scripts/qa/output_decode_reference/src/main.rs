// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Prints Actuation (actuation.rs) reference vectors for the Unity/Unreal
//! legacy decoders (scripts/qa/test_unity_output_decode.cs WakeAndDecodeTau,
//! scripts/qa/test_unreal_output_decode.cpp): NAO decode_tau_s + wake, plus
//! two modified joints. Offline; keep build products under target/:
//!
//!   CARGO_TARGET_DIR=target/output-decode-reference cargo run --offline \
//!     --manifest-path scripts/qa/output_decode_reference/Cargo.toml -- \
//!     sim/content/embodiments/generated/nao.json
use aarnn_embodiment::actuation::Actuation;
use aarnn_embodiment::aer::{Model, OutputCode, Reply};

fn main() {
    let path = std::env::args().nth(1).expect("nao.json");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for a in v["actuators"].as_array_mut().unwrap() {
        match a["name"].as_str().unwrap() {
            // B: non-zero wake pose, short wake, custom tau.
            "LHipPitch" => {
                a["wake"] = serde_json::json!({"from": -0.45, "seconds": 0.25});
                a["decode_tau_s"] = serde_json::json!(0.05);
            }
            // C: seed-only wake (seconds 0), default tau.
            "HeadYaw" => {
                a["wake"] = serde_json::json!({"from": 0.5, "seconds": 0.0});
                a.as_object_mut().unwrap().remove("decode_tau_s");
            }
            _ => {}
        }
    }
    let model: Model = serde_json::from_value(v).unwrap();
    let idx = |n: &str| model.actuators.iter().position(|a| a.name == n).unwrap();
    let cases = [("A", idx("LKneePitch")), ("B", idx("LHipPitch")), ("C", idx("HeadYaw"))];
    for (tag, j) in cases {
        let a = &model.actuators[j];
        println!("joint {tag} {} channel {} range {:?} {:?} wake {:?} tau {:?}", a.name, a.readout[0].channel,
                 a.control_range[0], a.control_range[1], a.wake.map(|w| (w.from, w.seconds)), a.decode_tau_s);
    }
    let width = model.io.output_names.len();
    let mut state = Actuation::new(&model);
    state.arm();
    let mut phase = [0.0f64; 3];
    let mut replies = 0u64;
    let density = |tag: usize, reply: u64| -> f64 {
        match tag {
            0 => if reply > 20 { 0.37 } else { 0.0 },
            1 => 0.8,
            _ => 0.0,
        }
    };
    let print_at: &[u64] = &[1, 2, 3, 10, 20, 21, 22, 60, 125, 126, 250, 499, 500, 501, 502, 1000, 1500];
    let resume_at: &[u64] = &[1561, 1562, 1600, 1660];
    for step in 1..=1660u64 {
        let reply = if step <= 1500 || step > 1560 {
            replies += 1;
            let mut spikes = vec![0u32; width];
            for (k, (_, j)) in cases.iter().enumerate() {
                phase[k] += density(k, replies);
                if phase[k] >= 1.0 - 1e-12 {
                    phase[k] -= 1.0;
                    spikes[model.actuators[*j].readout[0].channel] = 1;
                }
            }
            Some(Reply { source_step: step, timestamp_us: step * 2000, spikes, output_code: OutputCode::PulseDensity })
        } else {
            None
        };
        state.select(&model, step, false, true, reply.as_ref(), None).unwrap();
        if print_at.contains(&step) || resume_at.contains(&step) {
            let c = state.controls(&model);
            for (tag, j) in cases {
                println!("{tag} step {step} reply {replies} control {:?} rate {:?}", c[j], state.rates()[j]);
            }
        }
    }
}
