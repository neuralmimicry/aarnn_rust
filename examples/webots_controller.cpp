/**
 * @file webots_controller.cpp
 * @brief C++ Webots Controller Example using the Neuromorphic Demo FFI Bridge.
 *
 * This example demonstrates how to integrate the Rust-based neuromorphic engine
 * into a C++ environment, such as a Webots robot controller. It uses the
 * Foreign Function Interface (FFI) to communicate with the core library.
 *
 * ## Integration Steps:
 * 1. **Initialize**: Call `nm_init()` with a JSON configuration to setup the
 *    network and IO mapping.
 * 2. **Set Sensors**: Use `nm_set_port_by_index()` or similar FFI calls to
 *    feed robot sensor data (analog/floats) into the engine.
 * 3. **Step Simulation**: Call `nm_step()` to advance the neural network state
 *    by one time step.
 * 4. **Get Actuators**: Use `nm_get_port_by_index()` to retrieve processed
 *    actuator values (analog/floats) from the engine to drive robot joints.
 * 5. **Shutdown**: Call `nm_shutdown()` to clean up resources.
 *
 * ## Compilation:
 * 1. Build the Rust shared library: `cargo build --release --features ffi_bridge`
 * 2. Compile this controller, linking against `libneuromorphic_demo.so` (or .dylib/.dll).
 */

#include <cmath>
#include <cstdio>
#include <vector>

extern "C" {
#include "neuromorphic_bridge.h"
}

int main() {
  // Configure a tiny IO surface: 25 sensory, 11 actuator floats
  // This matches the port layout expected by the simulation bridge.
  const char* config_json = "{\"sensory\":25,\"output\":11}";
  if (nm_init(config_json) != 0) {
    std::fprintf(stderr, "nm_init failed\n");
    return 1;
  }

  std::vector<float> sensory_values(25, 0.0f);
  std::vector<float> actuator_values(11, 0.0f);

  // Main control loop (simulating a Webots step loop)
  const int num_steps = 200;
  for (int step_idx = 0; step_idx < num_steps; ++step_idx) {
    double current_time_ms = step_idx * 10.0; // 10 ms per simulation step
    
    // Simulate reading sensors (e.g. Sonar or IMU)
    float sine_value = std::sin(0.01 * static_cast<float>(step_idx));
    for (int j = 0; j < 5 && j < (int)sensory_values.size(); ++j) {
        sensory_values[j] = sine_value;
    }

    // Pass sensor data to the Rust engine
    if (nm_set_port_by_index(0, sensory_values.size(), sensory_values.data()) != 0) {
      std::fprintf(stderr, "nm_set_port_by_index failed at step %d\n", step_idx);
      break;
    }

    // Step the spiking neural network
    if (nm_step(current_time_ms) != 0) {
      std::fprintf(stderr, "nm_step failed at step %d\n", step_idx);
      break;
    }

    // Retrieve motor commands or LED values
    if (nm_get_port_by_index(0, actuator_values.size(), actuator_values.data()) != 0) {
      std::fprintf(stderr, "nm_get_port_by_index failed at step %d\n", step_idx);
      break;
    }

    // Periodic status report
    if (step_idx % 20 == 0) {
      std::printf("step %d: act0=%.3f act1=%.3f\n", step_idx, actuator_values[0], actuator_values[1]);
    }
  }

  // Cleanup FFI resources
  nm_shutdown();
  return 0;
}
