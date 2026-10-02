#pragma once

#include <algorithm>
#include <cmath>
#include <simulated_motor_response.hpp>

namespace aarnn::webots {

// Webots-only decoder for the 0.5-neutral, 1.0-spike C. elegans muscle channels.
// These are heuristic simulated-muscle time constants, not measured biology.
class CelegansMuscleResponse {
public:
    static constexpr int kVersion = 2;
    static constexpr float kMuscleTauMs = 55.0f;
    static constexpr float kSpineTauMs = 80.0f;
    static constexpr float kJointHalfRangeRad = 0.38f;

    explicit CelegansMuscleResponse(float step_ms, float neural_gain = 3.0f)
      : muscle_alpha_(alpha(step_ms, kMuscleTauMs)),
        spine_alpha_(alpha(step_ms, kSpineTauMs)),
        neural_gain_(std::clamp(neural_gain, 1.0f, 6.0f)) {}

    static float alpha(float step_ms, float tau_ms) {
        return SimulatedMotorResponse::alpha(step_ms, tau_ms);
    }

    float contraction(float raw, float& trace) const {
        // A bounded EMA preserves dorsal/ventral contrast during repeated spikes.
        const float drive = std::isfinite(raw)
          ? std::clamp((raw - 0.5f) * 2.0f, 0.0f, 1.0f) : 0.0f;
        if (!std::isfinite(trace)) trace = 0.0f;
        trace = std::clamp(trace + muscle_alpha_ * (drive - trace), 0.0f, 1.0f);
        return 0.5f + 0.5f * trace;
    }

    float target(float dorsal_ventral_drive) const {
        const float drive = std::isfinite(dorsal_ventral_drive)
          ? std::clamp(dorsal_ventral_drive, -1.0f, 1.0f) : 0.0f;
        // Saturation uses at most 0.44/0.50 of the motor's ±0.38 rad range.
        return std::clamp(0.5f + 0.44f * std::tanh(neural_gain_ * drive),
                          0.05f, 0.95f);
    }

    float smooth_spine(float target, float previous) const {
        const float safe_target = std::isfinite(target) ? std::clamp(target, 0.05f, 0.95f) : 0.5f;
        const float safe_previous = std::isfinite(previous) ? std::clamp(previous, 0.05f, 0.95f) : 0.5f;
        return safe_previous + spine_alpha_ * (safe_target - safe_previous);
    }

    float neural_gain() const { return neural_gain_; }

private:
    float muscle_alpha_;
    float spine_alpha_;
    float neural_gain_;
};

} // namespace aarnn::webots
