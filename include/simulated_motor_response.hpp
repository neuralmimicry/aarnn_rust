#pragma once

#include <algorithm>
#include <cmath>
#include <string_view>

namespace aarnn::webots {

// Command filtering for sandboxed Webots motors. Range and maximum speed are
// read from the motor itself; Webots still enforces velocity and torque limits.
struct SimulatedMotorResponse {
    static float alpha(float step_ms, float tau_ms) {
        if (!std::isfinite(step_ms) || step_ms <= 0.0f ||
            !std::isfinite(tau_ms) || tau_ms <= 0.0f) return 0.0f;
        return 1.0f - std::exp(-step_ms / tau_ms);
    }

    static bool channel_only(std::string_view name) {
        return name.rfind("celegans_o_", 0) == 0 ||
               name.rfind("dros_o_", 0) == 0;
    }

    static bool passive_lock(std::string_view name) {
        constexpr std::string_view suffix = "_root_lock";
        return name.size() >= suffix.size() &&
               name.substr(name.size() - suffix.size()) == suffix;
    }

    static float motor_alpha(std::string_view name, double min_position,
                             double max_position, double max_velocity,
                             float step_ms) {
        constexpr float kLegacyAlpha = 0.1f;
        if (channel_only(name) || passive_lock(name) ||
            !std::isfinite(min_position) ||
            !std::isfinite(max_position) || !std::isfinite(max_velocity) ||
            max_position <= min_position || max_velocity <= 0.0) {
            return kLegacyAlpha;
        }
        // Thirty per cent of the motor's half-range travel time, bounded to
        // avoid an excessively fast actuator or a sluggish wide-range joint.
        const double half_range_travel_ms =
          500.0 * (max_position - min_position) / max_velocity;
        const float tau_ms = static_cast<float>(std::clamp(
          0.30 * half_range_travel_ms, 55.0, 115.0));
        return std::clamp(alpha(step_ms, tau_ms), 0.0f, 0.45f);
    }
};

} // namespace aarnn::webots
