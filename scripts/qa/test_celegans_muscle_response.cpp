#include <celegans_muscle_response.hpp>

#include <cmath>
#include <cstdlib>
#include <iostream>
#include <limits>

using aarnn::webots::CelegansMuscleResponse;

static void require(bool passed, const char* message) {
    if (!passed) {
        std::cerr << "C. elegans muscle response: " << message << '\n';
        std::exit(1);
    }
}

int main() {
    CelegansMuscleResponse response(32.0f);
    float dorsal_left = 0.0f;
    float dorsal_right = 0.0f;
    float ventral_left = 0.0f;
    float ventral_right = 0.0f;
    float spine = 0.5f;
    float motor = 0.5f;
    const float motor_alpha = aarnn::webots::SimulatedMotorResponse::motor_alpha(
      "celegans_spine_01", -0.38, 0.38, 1.2, 32.0f);

    auto step = [&](float dorsal, float ventral) {
        const float dl = response.contraction(dorsal, dorsal_left);
        const float dr = response.contraction(dorsal, dorsal_right);
        const float vl = response.contraction(ventral, ventral_left);
        const float vr = response.contraction(0.5f, ventral_right);
        const float target = response.target(0.5f * (vl + vr) - 0.5f * (dl + dr));
        spine = response.smooth_spine(target, spine);
        motor += motor_alpha * (spine - motor);
        return (motor - 0.5f) * 2.0f * CelegansMuscleResponse::kJointHalfRangeRad;
    };

    for (int i = 0; i < 10; ++i) require(std::fabs(step(0.5f, 0.5f)) < 1e-6f,
                                        "neutral input moved a motor");
    const float first_pulse_rad = step(0.5f, 1.0f);
    require(first_pulse_rad > 0.008f, "one neural pulse remains visually attenuated");
    for (int i = 0; i < 80; ++i) step(0.5f, 1.0f);
    require(motor > 0.75f && motor < 0.95f,
            "sustained unilateral output did not produce bounded flexion");
    for (int i = 0; i < 32; ++i) step(0.5f, 0.5f);
    require(std::fabs(motor - 0.5f) < 0.003f,
            "spine did not relax towards neutral after output stopped");

    float balanced_dorsal = 0.0f;
    float balanced_ventral = 0.0f;
    for (int i = 0; i < 40; ++i) {
        const float dorsal = response.contraction(1.0f, balanced_dorsal);
        const float ventral = response.contraction(1.0f, balanced_ventral);
        require(std::fabs(response.target(ventral - dorsal) - 0.5f) < 1e-6f,
                "balanced dorsal and ventral spikes bent the spine");
    }
    require(response.target(-0.5f) < 0.25f && response.target(0.5f) > 0.75f,
            "opposite muscle banks lost their bend direction");
    require(response.target(100.0f) <= 0.95f && response.target(-100.0f) >= 0.05f,
            "strong neural drive exceeded the safe command range");
    float corrupted_trace = std::numeric_limits<float>::quiet_NaN();
    require(std::isfinite(response.contraction(
              std::numeric_limits<float>::quiet_NaN(), corrupted_trace)) &&
            corrupted_trace == 0.0f,
            "non-finite neural output contaminated muscle state");

    CelegansMuscleResponse half_step(16.0f);
    float full_trace = 0.0f;
    float half_trace = 0.0f;
    for (int i = 0; i < 10; ++i) response.contraction(1.0f, full_trace);
    for (int i = 0; i < 20; ++i) half_step.contraction(1.0f, half_trace);
    require(std::fabs(full_trace - half_trace) < 1e-5f,
            "muscle response depends on simulator step size");

    std::cout << "C. elegans muscle response passed; first pulse="
              << first_pulse_rad << " rad, settled motor=" << motor << '\n';
}
