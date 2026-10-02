#include <simulated_motor_response.hpp>

#include <cmath>
#include <cstdlib>
#include <iostream>
#include <limits>

using aarnn::webots::SimulatedMotorResponse;

static void require(bool passed, const char* message) {
    if (!passed) {
        std::cerr << "Webots motor response: " << message << '\n';
        std::exit(1);
    }
}

int main() {
    struct MotorCase {
        const char* robot;
        const char* motor;
        double min_position;
        double max_position;
        double max_velocity;
    };
    // Representative declared Webots motor limits for every local robot type.
    const MotorCase cases[] = {
      {"celegans", "celegans_spine_01", -0.38, 0.38, 1.2},
      {"drosophila_banc", "leg_left_front_coxa", -0.55, 0.55, 8.0},
      {"drosophila_fafb", "wing_left_flap", -0.75, 0.75, 40.0},
      {"hexapod", "hex_o_000_lf_coxa", -0.70, 0.70, 6.0},
      {"nao", "LShoulderPitch", -2.0, 2.0, 6.0},
      {"zebrafish", "zebrafish_o_00_tail_l0", -1.2, 1.2, 8.0},
    };
    for (const auto& item : cases) {
        const float gain = SimulatedMotorResponse::motor_alpha(
          item.motor, item.min_position, item.max_position, item.max_velocity, 32.0f);
        require(gain > 0.1f && gain <= 0.45f,
                "a physical motor is still excessively damped or exceeds its response cap");
        const float first_step = 0.5f + gain * (1.0f - 0.5f);
        require(first_step > 0.55f && first_step <= 0.725f,
                "a one-step neural output did not reach the motor safely");
        std::cout << item.robot << " motor_alpha=" << gain << '\n';
    }

    const float worm_full_step = SimulatedMotorResponse::motor_alpha(
      "celegans_spine_01", -0.38, 0.38, 1.2, 32.0f);
    const float worm_half_step = SimulatedMotorResponse::motor_alpha(
      "celegans_spine_01", -0.38, 0.38, 1.2, 16.0f);
    require(std::fabs(worm_full_step - (1.0f - std::pow(1.0f - worm_half_step, 2))) < 1e-6f,
            "motor response changes with the simulation step length");

    require(SimulatedMotorResponse::motor_alpha(
              "celegans_o_000_MDL01", -1.2, 1.2, 6.0, 32.0f) == 0.1f,
            "channel-only worm output changed its physical mapping");
    require(SimulatedMotorResponse::motor_alpha(
              "dros_o_000_node", -1.6, 1.6, 14.0, 32.0f) == 0.1f,
            "channel-only fly output changed its physical mapping");
    require(SimulatedMotorResponse::motor_alpha(
              "celegans_spine_root_lock", 0.0, 0.0, 1.2, 32.0f) == 0.1f,
            "locked joint acquired a responsive command");
    require(SimulatedMotorResponse::passive_lock("celegans_spine_root_lock") &&
              SimulatedMotorResponse::passive_lock("zebrafish_tail_root_lock") &&
              !SimulatedMotorResponse::passive_lock("celegans_spine_01"),
            "passive root locks were misclassified");
    require(SimulatedMotorResponse::motor_alpha(
              "zebrafish_tail_root_lock", -3.14, 3.14, 8.0, 32.0f) == 0.1f,
            "Webots fallback range caused a passive lock to be calibrated");
    require(SimulatedMotorResponse::motor_alpha(
              "invalid", -1.0, 1.0, std::numeric_limits<double>::quiet_NaN(), 32.0f) == 0.1f,
            "invalid motor capability was accepted");
    require(SimulatedMotorResponse::motor_alpha("invalid", -1.0, 1.0, 0.0, 32.0f) == 0.1f,
            "zero motor velocity was accepted");
}
