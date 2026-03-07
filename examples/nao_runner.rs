//! # NAO Robot Runner Example
//!
//! This example demonstrates how to interface the neuromorphic simulation engine
//! with a robot control loop using the `bridge` and `InMemoryAdapter`.
//!
//! It simulates a NAO-like robot with various sensors (Sonars, Accelerometer, Gyro,
//! GPS, Inertial, Foot sensors, Bumpers) and actuators (Joints, LEDs).
//!
//! ## Workflow
//! 1. **IO Mapping**: Define the sensor and actuator ports, mapping them to
//!    specific indices in the network's input/output layers.
//! 2. **Runner Initialization**: Setup the core SNN engine with desired
//!    neuron and learning parameters.
//! 3. **Bridge Setup**: Connect the `Runner` to `InMemoryAdapter`s via an
//!    `ExternalRunnerBridge`. This bridge handles quantization (converting
//!    analog sensor values to spikes) and dequantization (converting output
//!    spikes back to analog actuator values).
//! 4. **Main Loop**:
//!    - Simulate sensor input (mock data).
//!    - Update the bridge with new sensor data.
//!    - Step the simulation.
//!    - Read actuator values from the bridge for robot control.
//!
//! ## Build & Run
//! ```bash
//! cargo run --example nao_runner --features ui,robot_io
//! ```

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn main() {
    use aarnn_rust::bridge::{
        ExternalRunnerBridge, InMemoryAdapter, IoMapping, PortKind, PortSpec, Quantizer,
    };
    use aarnn_rust::config::{LIFParams, NetworkConfig, STDPParams};
    use aarnn_rust::runner::Runner;
    use aarnn_rust::sim::{Learning, NeuronModel};

    // --- Build a compact NAO-inspired mapping (matches examples/nao_mapping.rs layout) ---
    let sensory_size = 25usize; // see nao_mapping.rs
    let output_size = 11usize;
    let mut io_mapping = IoMapping::new(sensory_size, output_size);
    let mut current_sensory_idx = 0usize;
    io_mapping.add_port(PortSpec::new(
        "Sonar/Left",
        PortKind::Sensor,
        current_sensory_idx,
        1,
    ));
    current_sensory_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "Sonar/Right",
        PortKind::Sensor,
        current_sensory_idx,
        1,
    ));
    current_sensory_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "Accel",
        PortKind::Sensor,
        current_sensory_idx,
        3,
    ));
    current_sensory_idx += 3;
    io_mapping.add_port(PortSpec::new(
        "Gyro",
        PortKind::Sensor,
        current_sensory_idx,
        2,
    ));
    current_sensory_idx += 2;
    io_mapping.add_port(PortSpec::new(
        "GPS",
        PortKind::Sensor,
        current_sensory_idx,
        3,
    ));
    current_sensory_idx += 3;
    io_mapping.add_port(PortSpec::new(
        "InertialRPY",
        PortKind::Sensor,
        current_sensory_idx,
        3,
    ));
    current_sensory_idx += 3;
    io_mapping.add_port(PortSpec::new(
        "Foot/L",
        PortKind::Sensor,
        current_sensory_idx,
        4,
    ));
    current_sensory_idx += 4;
    io_mapping.add_port(PortSpec::new(
        "Foot/R",
        PortKind::Sensor,
        current_sensory_idx,
        4,
    ));
    current_sensory_idx += 4;
    io_mapping.add_port(PortSpec::new(
        "Bumper/L",
        PortKind::Sensor,
        current_sensory_idx,
        2,
    ));
    current_sensory_idx += 2;
    io_mapping.add_port(PortSpec::new(
        "Bumper/R",
        PortKind::Sensor,
        current_sensory_idx,
        2,
    ));
    current_sensory_idx += 2;
    let mut current_actuator_idx = 0usize;
    io_mapping.add_port(PortSpec::new(
        "RShoulderPitch",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));
    current_actuator_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "LShoulderPitch",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));
    current_actuator_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "Hands/R",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));
    current_actuator_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "Hands/L",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));
    current_actuator_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "LED/Chest",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));
    current_actuator_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "LED/RFoot",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));
    current_actuator_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "LED/LFoot",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));
    current_actuator_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "LED/FaceR",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));
    current_actuator_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "LED/FaceL",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));
    current_actuator_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "LED/EarR",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));
    current_actuator_idx += 1;
    io_mapping.add_port(PortSpec::new(
        "LED/EarL",
        PortKind::Actuator,
        current_actuator_idx,
        1,
    ));

    // --- Runner configuration ---
    let lif_params = LIFParams::default();
    let stdp_params = STDPParams::default();
    let network_config = NetworkConfig {
        num_sensory_neurons: sensory_size,
        num_hidden_layers: 2,
        num_hidden_per_layer_initial: 16,
        num_output_neurons: output_size,
        ..Default::default()
    };
    let neuron_model = NeuronModel::Lif;
    let learning_rule = Learning::Stdp;
    let runner = Runner::new(
        lif_params,
        stdp_params,
        network_config,
        neuron_model,
        learning_rule,
    );

    // --- Bridge and adapters ---
    let in_mem_adapter = InMemoryAdapter::new(io_mapping.clone());
    let mut sensor_adapter = InMemoryAdapter::new(io_mapping.clone()); // clone for sensor role
    let mut actuator_sink = InMemoryAdapter::new(io_mapping.clone());
    let quantizer = Quantizer::default();
    let mut runner_bridge = ExternalRunnerBridge::new(
        runner,
        io_mapping.clone(),
        sensor_adapter,
        actuator_sink,
        quantizer,
    );

    // Helper to push sensor values into the bridge’s sensor adapter (via in_mem_adapter snapshot)
    fn copy_inputs_to_bridge(src: &InMemoryAdapter, dst: &mut InMemoryAdapter) {
        use aarnn_rust::bridge::SensorSource;
        // Use the trait to copy flattened buffers
        let mut tmp_buffer = vec![0.0f32; src.mapping().sensory_size];
        let mut src_clone = src.clone();
        src_clone.fill_inputs(0.0, &mut tmp_buffer);
        dst.fill_inputs(0.0, &mut tmp_buffer);
    }

    // --- Mock input and step loop ---
    // Seed some deterministic patterns on sonar and IMU
    let mut current_time_ms = 0.0f64;
    for step_idx in 0..10 {
        let left_sonar = ((step_idx as f32) * 0.13).sin() * 0.5 + 0.5;
        let right_sonar = ((step_idx as f32) * 0.19).cos() * 0.5 + 0.5;
        let accel_data = [0.0, 0.1 * (step_idx as f32).sin(), 0.0];
        let gyro_data = [
            0.02 * (step_idx as f32).cos(),
            -0.015 * (step_idx as f32).sin(),
        ];

        let mut output_value_buffer = [0.0f32; 1];

        let mut current_input_source = in_mem_adapter.clone();
        current_input_source.set_port("Sonar/Left", &[left_sonar]);
        current_input_source.set_port("Sonar/Right", &[right_sonar]);
        current_input_source.set_port("Accel", &accel_data);
        current_input_source.set_port("Gyro", &gyro_data);
        // copy into the bridge sensor
        copy_inputs_to_bridge(&current_input_source, &mut runner_bridge.sensor);

        let _step_output = runner_bridge.step(current_time_ms);
        // Read a few actuator ports for display
        runner_bridge
            .actuator
            .get_port("RShoulderPitch", &mut output_value_buffer);
        println!(
            "t={:.1} ms, spikes_out[0..{}] some={:?}",
            current_time_ms,
            runner_bridge.mapping.output_size.min(4),
            &runner_bridge.out_buf[0..4.min(runner_bridge.out_buf.len())]
        );
        println!("  RShoulderPitch: {:.2}", output_value_buffer[0]);
        current_time_ms += lif_params.dt;
    }

    println!("nao_runner finished.");
}

#[cfg(not(all(feature = "ui", feature = "robot_io")))]
fn main() {
    eprintln!("Build with --features ui,robot_io to run this example.");
}
