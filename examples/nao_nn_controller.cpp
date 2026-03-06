// Webots NAO controller example that uses the Rust FFI bridge as the brain.
//
// Intended world: .../projects/robots/softbank/nao/worlds/nao_room.wbt
// Build notes:
// - This file is meant to be copied into your Webots project under
//   controllers/nao_nn_controller/ and built by Webots, linking against
//   libneuromorphic_demo.so produced by this repo.
// - Provide the public header path with -I<repo>/include and the library path
//   with -L<repo>/target/release -lneuromorphic_demo and set rpath to $ORIGIN
//   so the controller binary finds the .so placed next to it.
// - Example rpath: -Wl,-rpath,'$ORIGIN'
//
// Safety: we map outputs to a small set of NAO joints with conservative spans
// and smoothing. Always respect joint limits in production code.

#include <webots/Robot.hpp>
#include <webots/Motor.hpp>
#include <webots/Accelerometer.hpp>
#include <webots/Gyro.hpp>
#include <webots/DistanceSensor.hpp>
#include <webots/Keyboard.hpp>
#include <webots/utils/motion.h>
#include <device_mapper.hpp>
#include <keyboard_mapper.hpp>
#include <algorithm>
#include <vector>
#include <iostream>
#include <string>

// FFI to the Rust neural engine
#include "neuromorphic_bridge.h"

using namespace webots;

static WbMotionRef hand_wave, forwards, backwards, side_step_left, side_step_right, turn_left_60, turn_right_60, tai_chi, wipe_forehead;
static WbMotionRef currently_playing = NULL;

static void load_motions() {
  hand_wave = wbu_motion_new("../../motions/HandWave.motion");
  forwards = wbu_motion_new("../../motions/Forwards50.motion");
  backwards = wbu_motion_new("../../motions/Backwards.motion");
  side_step_left = wbu_motion_new("../../motions/SideStepLeft.motion");
  side_step_right = wbu_motion_new("../../motions/SideStepRight.motion");
  turn_left_60 = wbu_motion_new("../../motions/TurnLeft60.motion");
  turn_right_60 = wbu_motion_new("../../motions/TurnRight60.motion");
  tai_chi = wbu_motion_new("../../motions/TaiChi.motion");
  wipe_forehead = wbu_motion_new("../../motions/WipeForehead.motion");
}

static void start_motion(WbMotionRef motion) {
  if (currently_playing)
    wbu_motion_stop(currently_playing);
  if (motion) {
    wbu_motion_play(motion);
    currently_playing = motion;
  }
}

int main(int argc, char** argv){
  // 0. Parse pseudo-environment variables from controllerArgs
  for (int i = 1; i < argc; ++i) {
    std::string arg = argv[i];
    size_t eq = arg.find('=');
    if (eq != std::string::npos && eq > 0) {
      std::string key = arg.substr(0, eq);
      std::string val = arg.substr(eq + 1);
      setenv(key.c_str(), val.c_str(), 1);
    }
  }

  Robot robot;
  const int basic_time_step = (int)robot.getBasicTimeStep();

  DeviceMapper mapper;
  mapper.discover(robot, basic_time_step);

  KeyboardMapper kb_mapper;
  kb_mapper.autodetect(robot);

  const int num_sensory_neurons = mapper.get_sensory_size();
  const int num_output_neurons = mapper.get_output_size();

  std::cout << "[nao_nn_controller] Auto-mapped S=" << num_sensory_neurons << " O=" << num_output_neurons << std::endl;

  load_motions();

  // Build init JSON with port names
  std::string config_json = "{\"sensory\":" + std::to_string(num_sensory_neurons) + ",\"output\":" + std::to_string(num_output_neurons) + ",\"threshold\":0.2";
  
  config_json += ",\"s_names\":[";
  auto sensory_names = mapper.get_sensor_names();
  for (size_t i = 0; i < sensory_names.size(); ++i) {
    config_json += "\"" + sensory_names[i] + "\"" + (i == sensory_names.size() - 1 ? "" : ",");
  }
  config_json += "],\"o_names\":[";
  auto output_names = mapper.get_actuator_names();
  for (size_t i = 0; i < output_names.size(); ++i) {
    config_json += "\"" + output_names[i] + "\"" + (i == output_names.size() - 1 ? "" : ",");
  }
  config_json += "]}";

  // Initialize the neural engine
  if (nm_init(config_json.c_str()) != 0) {
    std::cerr << "[nao_nn_controller] Failed to initialize neural engine\n";
    return 1;
  }

  std::vector<float> sensory_values(num_sensory_neurons, 0.0f), actuator_values(num_output_neurons, 0.5f); // Start with neutral actuators

  // Apply initial neutral positions before the first simulation step
  mapper.apply_actuators(actuator_values);

  int step_count = 0;
  while (robot.step(basic_time_step) != -1) {
    // 0) Handle keyboard
    if (kb_mapper.is_enabled()) {
      int key = robot.getKeyboard()->getKey();
      if (key > 0) {
        KeyMapping key_mapping = kb_mapper.get_mapping();
        if (key == key_mapping.up) start_motion(forwards);
        else if (key == key_mapping.down) start_motion(backwards);
        else if (key == key_mapping.left) start_motion(side_step_left);
        else if (key == key_mapping.right) start_motion(side_step_right);
        else if (key == key_mapping.turn_left) start_motion(turn_left_60);
        else if (key == key_mapping.turn_right) start_motion(turn_right_60);
        else if (key == key_mapping.tai_chi) start_motion(tai_chi);
        else if (key == key_mapping.wipe) start_motion(wipe_forehead);
        else if (key == key_mapping.wave) start_motion(hand_wave);
      }
    }

    // 1) Auto-fill sensors
    mapper.fill_sensors(sensory_values);

    // 2) NN step
    nm_set_port_by_index(0, sensory_values.size(), sensory_values.data());
    nm_step((double)basic_time_step);
    nm_get_port_by_index(0, actuator_values.size(), actuator_values.data());

    // 3) Auto-apply actuators (only if no keyboard motion is active)
    bool is_motion_active = false;
    if (currently_playing) {
      if (wbu_motion_is_over(currently_playing)) {
        currently_playing = NULL;
      } else {
        is_motion_active = true;
      }
    }

    if (!is_motion_active) {
      mapper.apply_actuators(actuator_values);
    }

    if ((step_count++ % 20) == 0) {
      std::cout << "step " << step_count << ": auto-mapped " << num_sensory_neurons << " inputs, " << num_output_neurons << " outputs" << (is_motion_active ? " [Motion active]" : "") << std::endl;
    }
  }

  nm_shutdown();
  return 0;
}
