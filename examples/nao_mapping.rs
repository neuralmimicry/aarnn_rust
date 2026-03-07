//! Example: NAO mapping declaration for Webots demo devices.
//!
//! Build (mapping only check):
//!   cargo check --features robot_io -Z unstable-options --examples
//!
//! With UI Runner demo, see `examples/nao_runner.rs`.

#[cfg(feature = "robot_io")]
fn main() {
    use aarnn_rust::bridge::{IoMapping, PortKind, PortSpec};

    // Sensor channels (rough mirror of the NAO demo snippet)
    // Sonar L/R (2), Accelerometer (3), Gyro (2), GPS (3), Inertial Unit RPY (3),
    // Foot sensors (L4 + R4 = 8), Foot bumpers (4)
    let num_sonar_sensors = 2usize;
    let num_accel_sensors = 3usize;
    let num_gyro_sensors = 2usize;
    let num_gps_sensors = 3usize;
    let num_rpy_sensors = 3usize;
    let num_foot_sensors = 8usize;
    let num_bump_sensors = 4usize;
    let sensory_size = num_sonar_sensors
        + num_accel_sensors
        + num_gyro_sensors
        + num_gps_sensors
        + num_rpy_sensors
        + num_foot_sensors
        + num_bump_sensors; // 25

    // Actuators: R/L ShoulderPitch (2), Hands (R & L groups using averaged command) (2),
    // LEDs aggregate groups (Chest, RFoot, LFoot, FaceR, FaceL, EarR, EarL) -> (7)
    // Keep it small for demo purposes: 2 + 2 + 7 = 11
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
        num_accel_sensors,
    ));
    current_sensory_idx += num_accel_sensors;
    io_mapping.add_port(PortSpec::new(
        "Gyro",
        PortKind::Sensor,
        current_sensory_idx,
        num_gyro_sensors,
    ));
    current_sensory_idx += num_gyro_sensors;
    io_mapping.add_port(PortSpec::new(
        "GPS",
        PortKind::Sensor,
        current_sensory_idx,
        num_gps_sensors,
    ));
    current_sensory_idx += num_gps_sensors;
    io_mapping.add_port(PortSpec::new(
        "InertialRPY",
        PortKind::Sensor,
        current_sensory_idx,
        num_rpy_sensors,
    ));
    current_sensory_idx += num_rpy_sensors;
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

    // Actuator ports
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
    )); // current_actuator_idx += 1;

    println!(
        "NAO IoMapping ready: S={}, O={}, ports_s={}, ports_o={}",
        io_mapping.sensory_size,
        io_mapping.output_size,
        io_mapping.sensors().len(),
        io_mapping.actuators().len()
    );
}

#[cfg(not(feature = "robot_io"))]
fn main() {
    eprintln!("Build with --features robot_io to run this example.");
}
