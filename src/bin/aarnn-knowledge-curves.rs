//! `aarnn-knowledge-curves [points] [steps]`
//!
//! Exports the measured steady-state transfer (f-I) curves of AARNN's neuron
//! models as JSON on stdout. Evelyn fits its feed-forward population codes
//! against these curves, so converted knowledge regions run on AARNN's real
//! dynamics. Covers LIF and Izhikevich RS at detail depths 0 and 2.

use aarnn_rust::config::{AarnnBioParams, IzhikevichParams, LIFParams};
use aarnn_rust::knowledge::{KnowledgeNeuron, current_grid, transfer_curve, transfer_curve_noisy};
use serde::Serialize;
use std::process::ExitCode;

/// A measured curve plus the exact neuron configuration it was measured on,
/// which Evelyn copies verbatim into knowledge-region mesh descriptions.
#[derive(Serialize)]
struct CurveWithSpec {
    #[serde(flatten)]
    curve: aarnn_rust::knowledge::TransferCurve,
    spec: KnowledgeNeuron,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let parse = |i: usize, d: usize| args.get(i).map_or(Ok(d), |s| s.parse::<usize>());
    let (Ok(points), Ok(steps)) = (parse(1, 401), parse(2, 4000)) else {
        eprintln!("usage: aarnn-knowledge-curves [points] [steps]");
        return ExitCode::from(2);
    };
    let params = IzhikevichParams::from_preset("RS", 1.0);
    let bio = AarnnBioParams::default();
    let neurons = [
        (
            KnowledgeNeuron::Lif(LIFParams::default()),
            current_grid(0.0, 0.4, points),
        ),
        (
            KnowledgeNeuron::Izhikevich {
                params,
                bio: bio.clone(),
                detail_depth: 0,
            },
            current_grid(0.0, 40.0, points),
        ),
        (
            KnowledgeNeuron::Izhikevich {
                params,
                bio,
                detail_depth: 2,
            },
            current_grid(0.0, 40.0, points),
        ),
    ];
    // Each neuron is measured noiselessly first to find its rheobase-to-
    // saturation span. It is then re-measured with membrane noise of 20 % of
    // that span, the graded response knowledge regions are fitted and run with.
    let curves: Vec<_> = neurons
        .iter()
        .map(|(n, g)| {
            let plain = transfer_curve(n, g, steps, steps / 10);
            let max = plain.rates.iter().cloned().fold(0.0, f64::max);
            let rheo = plain
                .currents
                .iter()
                .zip(&plain.rates)
                .find(|(_, r)| **r > 0.0)
                .map_or(0.0, |(c, _)| *c);
            let sat = plain
                .currents
                .iter()
                .zip(&plain.rates)
                .find(|(_, r)| **r >= 0.98 * max)
                .map_or(rheo, |(c, _)| *c);
            CurveWithSpec {
                curve: transfer_curve_noisy(
                    n,
                    g,
                    steps * 5,
                    steps / 10,
                    0.2 * (sat - rheo).max(1e-6),
                ),
                spec: n.clone(),
            }
        })
        .collect();
    match serde_json::to_string(&curves) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("serialise curves: {e}");
            ExitCode::from(1)
        }
    }
}
