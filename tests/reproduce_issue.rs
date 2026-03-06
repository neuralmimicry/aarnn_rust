#[cfg(feature = "growth3d")]
use neuromorphic_demo::runner::Runner;
#[cfg(feature = "growth3d")]
use neuromorphic_demo::config::{LIFParams, STDPParams, NetworkConfig};
#[cfg(feature = "growth3d")]
use neuromorphic_demo::sim::{NeuronModel, Learning};

#[test]
#[cfg(all(feature = "growth3d", feature = "morpho"))]
fn test_neuron_migration_shapes() {
    let mut net = NetworkConfig::default();
    net.growth_enabled = true;
    net.use_morphology = true;
    net.num_hidden_layers = 2;
    net.num_hidden_per_layer_initial = 5;
    net.num_sensory_neurons = 10;
    net.num_output_neurons = 5;
    
    let mut r = Runner::new(LIFParams::default(), STDPParams::default(), net, NeuronModel::Lif, Learning::Aarnn);
    
    // Bypass bootstrap reset by importing a 2-layer snapshot
    let snap_json = r#"{
        "net": {
            "num_sensory_neurons": 10,
            "num_hidden_layers": 2,
            "num_hidden_per_layer_initial": 5,
            "num_output_neurons": 5,
            "use_morphology": true,
            "growth_enabled": true
        },
        "w_in": { "rows": 5, "cols": 10, "data": [0.1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
        "w_hh_fwd": [{ "rows": 5, "cols": 5, "data": [] }],
        "w_hh_bwd": [{ "rows": 5, "cols": 5, "data": [] }],
        "w_hh_rec": [
            { "rows": 5, "cols": 5, "data": [] },
            { "rows": 5, "cols": 5, "data": [] }
        ],
        "w_out": { "rows": 5, "cols": 5, "data": [] }
    }"#;
    r.import_network_json(snap_json).unwrap();

    // Initial shapes
    assert_eq!(r.v_h.len(), 2);
    assert_eq!(r.v_h[0].len(), 5);
    assert_eq!(r.v_h[1].len(), 5);
    
    // LIF: in_l=0, out_l=1 (since num_layers=2)
    let (in_l, out_l) = r.get_io_layers();
    assert_eq!(in_l, 0);
    assert_eq!(out_l, 1);
    
    assert_eq!(r.w_in.nrows(), 5); // layer 0 size
    assert_eq!(r.w_out.ncols(), 5); // layer 1 size
    
    // Force migration of neuron 0 in layer 0 to layer 1
    r.t = 1000;
    // Set a sensory connection to be stable
    r.conn_presence_in[(0, 0)] = 1000; 
    // Ensure NO recurrent connections for neuron 0
    for k in 0..5 {
        r.conn_presence_rec[0][(k, 0)] = 0;
        r.conn_presence_rec[0][(0, k)] = 0;
    }

    r.reassign_neurons_to_next_layer();
    
    // Check if it moved
    assert_eq!(r.v_h[0].len(), 4);
    assert_eq!(r.v_h[1].len(), 6);
    
    // Check matrix shapes
    assert_eq!(r.w_in.nrows(), 4); // layer 0 shrunk
    assert_eq!(r.w_out.ncols(), 6); // layer 1 grew
    
    // Forward matrix from 0 to 1: shape (H1, H0) = (6, 4)
    assert_eq!(r.w_hh_fwd[0].nrows(), 6);
    assert_eq!(r.w_hh_fwd[0].ncols(), 4);
    
    // This should NOT panic
    r.step(None);
}

#[test]
#[cfg(all(feature = "growth3d", feature = "morpho"))]
fn test_aarnn_stp_step() {
    let mut net = NetworkConfig::default();
    net.use_morphology = true;
    net.aarnn_layer_depth = 5;
    net.aarnn_bio.stp_enabled = true;
    net.num_hidden_layers = 1;
    net.num_hidden_per_layer_initial = 1;
    net.num_sensory_neurons = 1;
    net.num_output_neurons = 1;
    
    let mut r = Runner::new(LIFParams::default(), STDPParams::default(), net, NeuronModel::Aarnn, Learning::Aarnn);
    
    // Run several steps to ensure STP state updates and no panics
    for _ in 0..10 {
        r.step(Some(&[1]));
    }
}

#[test]
#[cfg(all(feature = "growth3d", feature = "morpho"))]
fn test_import_and_stp_step() {
    let mut net = NetworkConfig::default();
    net.use_morphology = true;
    net.aarnn_layer_depth = 5;
    net.aarnn_bio.stp_enabled = true;
    
    let mut r = Runner::new(LIFParams::default(), STDPParams::default(), net, NeuronModel::Aarnn, Learning::Aarnn);
    
    // Import a 6-layer AARNN snapshot (1 neuron per layer)
    let snap_json = r#"{
        "net": {
            "num_sensory_neurons": 10,
            "num_hidden_layers": 6,
            "num_hidden_per_layer_initial": 1,
            "num_output_neurons": 10,
            "use_morphology": true,
            "aarnn_layer_depth": 5
        },
        "w_in": { "rows": 1, "cols": 10, "data": [0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1] },
        "w_hh_fwd": [
            { "rows": 1, "cols": 1, "data": [1.0] },
            { "rows": 1, "cols": 1, "data": [1.0] },
            { "rows": 1, "cols": 1, "data": [1.0] },
            { "rows": 1, "cols": 1, "data": [1.0] },
            { "rows": 1, "cols": 1, "data": [1.0] }
        ],
        "w_hh_bwd": [
            { "rows": 1, "cols": 1, "data": [0.0] },
            { "rows": 1, "cols": 1, "data": [0.0] },
            { "rows": 1, "cols": 1, "data": [0.0] },
            { "rows": 1, "cols": 1, "data": [0.0] },
            { "rows": 1, "cols": 1, "data": [0.0] }
        ],
        "w_hh_rec": [
            { "rows": 1, "cols": 1, "data": [0.0] },
            { "rows": 1, "cols": 1, "data": [0.0] },
            { "rows": 1, "cols": 1, "data": [0.0] },
            { "rows": 1, "cols": 1, "data": [0.0] },
            { "rows": 1, "cols": 1, "data": [0.0] },
            { "rows": 1, "cols": 1, "data": [0.0] }
        ],
        "w_out": { "rows": 10, "cols": 1, "data": [0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1] }
    }"#;
    r.import_network_json(snap_json).unwrap();
    
    // This should NOT panic
    for _ in 0..5 {
        r.step(Some(&[1, 0, 1, 0, 1, 0, 1, 0, 1, 0]));
    }
}

#[test]
#[cfg(all(feature = "growth3d", feature = "morpho"))]
fn test_configurable_io_layers() {
    let mut net = NetworkConfig::default();
    net.num_hidden_layers = 3;
    net.num_hidden_per_layer_initial = 1;
    net.num_sensory_neurons = 1;
    net.num_output_neurons = 1;
    net.use_morphology = true;
    
    // Custom mapping: Sensory -> H2, Output <- H0
    net.sensory_target_layer = Some(2);
    net.output_source_layer = Some(0);
    
    let mut r = Runner::new(LIFParams::default(), STDPParams::default(), net.clone(), NeuronModel::Lif, Learning::Aarnn);
    
    // Verify get_io_layers
    let (in_l, out_l) = r.get_io_layers();
    assert_eq!(in_l, 2);
    assert_eq!(out_l, 0);
    
    // Verify matrix shapes match target layers
    // In this 3-layer net (1 neuron per layer), all hidden layers have size 1.
    assert_eq!(r.w_in.nrows(), 1);
    assert_eq!(r.w_out.ncols(), 1);
    
    // Verify simulation step works with this mapping
    for _ in 0..10 {
        r.step(Some(&[1]));
    }
    
    // Now verify batch simulation path
    use neuromorphic_demo::sim::run_snn;
    use neuromorphic_demo::network::build_network;
    let mut rng = rand::rng();
    let built = build_network(&net, &mut rng);
    let sensory_spikes = ndarray::Array2::from_elem((10, 1), 1i8);
    
    let out = run_snn(10.0, &LIFParams::default(), &STDPParams::default(), &net, built, &sensory_spikes, NeuronModel::Lif, Learning::Stdp);
    
    assert_eq!(out.spikes_h.len(), 3);
}
