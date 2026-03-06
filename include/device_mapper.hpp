#pragma once
#include <webots/Robot.hpp>
#include <webots/Device.hpp>
#include <webots/Motor.hpp>
#include <webots/Accelerometer.hpp>
#include <webots/Gyro.hpp>
#include <webots/DistanceSensor.hpp>
#include <webots/TouchSensor.hpp>
#include <webots/PositionSensor.hpp>
#include <webots/Node.hpp>

#include <vector>
#include <string>
#include <iostream>
#include <algorithm>
#include <map>

namespace webots {

struct DeviceMapping {
    Device* device;
    int type;
    int size;
    std::vector<std::string> port_names;
    std::vector<double> min_values;
    std::vector<double> max_values;
};

class DeviceMapper {
public:
    DeviceMapper() {}

    void discover(Robot& robot, int timestep) {
        std::cout << "[DeviceMapper] discover (v1.2 - extra robust clamping)" << std::endl;
        sensors_.clear();
        actuators_.clear();
        total_s_ = 0;
        total_o_ = 0;

        int n = robot.getNumberOfDevices();
        for (int i = 0; i < n; ++i) {
            Device* d = robot.getDeviceByIndex(i);
            int type = d->getNodeType();
            std::string name = d->getName();

            // Sensors
            if (type == Node::ACCELEROMETER) {
                auto s = (Accelerometer*)d;
                s->enable(timestep);
                add_sensor(d, type, 3, {name + ".x", name + ".y", name + ".z"}, -20.0, 20.0);
            } else if (type == Node::GYRO) {
                auto s = (Gyro*)d;
                s->enable(timestep);
                add_sensor(d, type, 3, {name + ".x", name + ".y", name + ".z"}, -10.0, 10.0);
            } else if (type == Node::DISTANCE_SENSOR) {
                auto s = (DistanceSensor*)d;
                s->enable(timestep);
                add_sensor(d, type, 1, {name}, 0.0, s->getMaxValue());
            } else if (type == Node::TOUCH_SENSOR) {
                auto s = (TouchSensor*)d;
                s->enable(timestep);
                if (s->getType() == TouchSensor::FORCE3D) {
                    add_sensor(d, type, 3, {name + ".x", name + ".y", name + ".z"}, -100.0, 100.0);
                } else {
                    add_sensor(d, type, 1, {name}, 0.0, 1.0);
                }
            } else if (type == Node::POSITION_SENSOR) {
                auto s = (PositionSensor*)d;
                s->enable(timestep);
                add_sensor(d, type, 1, {name}, -3.14, 3.14); // Heuristic
            } 
            // Actuators
            else if (type == Node::ROTATIONAL_MOTOR || type == Node::LINEAR_MOTOR) {
                auto m = (Motor*)d;
                double min_p = m->getMinPosition();
                double max_p = m->getMaxPosition();
                // Snap very small values to 0 to avoid precision issues with Webots' strict internal limits
                if (std::abs(min_p) < 1e-9) min_p = 0.0;
                if (std::abs(max_p) < 1e-9) max_p = 0.0;
                add_actuator(d, type, 1, {name}, min_p, max_p);
            }
        }

        // Sort to ensure stable ordering across runs
        auto sort_cmp = [](const DeviceMapping& a, const DeviceMapping& b) {
            return a.device->getName() < b.device->getName();
        };
        std::sort(sensors_.begin(), sensors_.end(), sort_cmp);
        std::sort(actuators_.begin(), actuators_.end(), sort_cmp);

        // Re-calculate totals after sort
        total_s_ = 0;
        for (auto& m : sensors_) total_s_ += m.size;
        total_o_ = 0;
        for (auto& m : actuators_) total_o_ += m.size;

        // Initialize smoothing buffer
        last_output_.assign(actuators_.size(), 0.5);
    }

    int get_sensory_size() const { return total_s_; }
    int get_output_size() const { return total_o_; }

    void fill_sensors(std::vector<float>& buf) {
        if (buf.size() < (size_t)total_s_) buf.resize(total_s_);
        int idx = 0;
        for (auto& m : sensors_) {
            if (m.type == Node::ACCELEROMETER) {
                const double* v = ((Accelerometer*)m.device)->getValues();
                for (int i = 0; i < 3; ++i) buf[idx++] = normalize(v[i], m.min_values[i], m.max_values[i]);
            } else if (m.type == Node::GYRO) {
                const double* v = ((Gyro*)m.device)->getValues();
                for (int i = 0; i < 3; ++i) buf[idx++] = normalize(v[i], m.min_values[i], m.max_values[i]);
            } else if (m.type == Node::DISTANCE_SENSOR) {
                buf[idx++] = normalize(((DistanceSensor*)m.device)->getValue(), m.min_values[0], m.max_values[0]);
            } else if (m.type == Node::TOUCH_SENSOR) {
                auto s = (TouchSensor*)m.device;
                if (m.size == 3) {
                    const double* v = s->getValues();
                    for (int i = 0; i < 3; ++i) buf[idx++] = normalize(v[i], m.min_values[i], m.max_values[i]);
                } else {
                    buf[idx++] = normalize(s->getValue(), m.min_values[0], m.max_values[0]);
                }
            } else if (m.type == Node::POSITION_SENSOR) {
                buf[idx++] = normalize(((PositionSensor*)m.device)->getValue(), m.min_values[0], m.max_values[0]);
            }
        }
    }

    void apply_actuators(const std::vector<float>& buf) {
        if (last_output_.size() != actuators_.size()) {
            last_output_.assign(actuators_.size(), 0.5);
        }

        int idx = 0;
        for (auto& m : actuators_) {
            if (m.type == Node::ROTATIONAL_MOTOR || m.type == Node::LINEAR_MOTOR) {
                float val = buf[idx];
                if (!std::isfinite(val)) val = 0.5f; // Fallback to neutral

                // EMA Smoothing (0.9 prev + 0.1 new)
                double alpha = 0.1;
                last_output_[idx] = (1.0 - alpha) * last_output_[idx] + alpha * (double)val;

                double pos = denormalize((float)last_output_[idx], m.min_values[0], m.max_values[0]);

                // Robust clamping with inward margin (1e-3) to avoid precision-related range warnings
                double eps = 1e-3;
                double min_v = m.min_values[0];
                double max_v = m.max_values[0];
                if (min_v < max_v) {
                    double safe_min = min_v + eps;
                    double safe_max = max_v - eps;
                    // Ensure safe_min is not greater than safe_max
                    if (safe_min > safe_max) {
                        pos = (min_v + max_v) * 0.5;
                    } else {
                        if (pos < safe_min) pos = safe_min;
                        if (pos > safe_max) pos = safe_max;
                    }
                } else {
                    pos = min_v;
                }

                if (pos < 0.0 && min_v >= 0.0) {
                    pos = 0.0;
                }

                ((Motor*)m.device)->setPosition(pos);
                idx++;
            }
        }
    }

    std::vector<std::string> get_sensor_names() const {
        std::vector<std::string> names;
        for (auto& m : sensors_) {
            for (auto& n : m.port_names) names.push_back(n);
        }
        return names;
    }

    std::vector<std::string> get_actuator_names() const {
        std::vector<std::string> names;
        for (auto& m : actuators_) {
            for (auto& n : m.port_names) names.push_back(n);
        }
        return names;
    }

private:
    std::vector<DeviceMapping> sensors_;
    std::vector<DeviceMapping> actuators_;
    std::vector<double> last_output_;
    int total_s_ = 0;
    int total_o_ = 0;

    void add_sensor(Device* d, int type, int size, std::vector<std::string> names, double min_v, double max_v) {
        DeviceMapping m;
        m.device = d;
        m.type = type;
        m.size = size;
        m.port_names = names;
        for (int i = 0; i < size; ++i) {
            m.min_values.push_back(min_v);
            m.max_values.push_back(max_v);
        }
        sensors_.push_back(m);
    }

    void add_actuator(Device* d, int type, int size, std::vector<std::string> names, double min_v, double max_v) {
        DeviceMapping m;
        m.device = d;
        m.type = type;
        m.size = size;
        m.port_names = names;
        // Motor limits can be infinite in Webots
        if (min_v == max_v) { // Probably 0,0 or infinite
            min_v = -3.14;
            max_v = 3.14;
        }
        for (int i = 0; i < size; ++i) {
            m.min_values.push_back(min_v);
            m.max_values.push_back(max_v);
        }
        actuators_.push_back(m);
    }

    float normalize(double val, double min_v, double max_v) {
        if (max_v == min_v) return 0.5f;
        double norm = (val - min_v) / (max_v - min_v);
        return (float)std::max(0.0, std::min(1.0, norm));
    }

    double denormalize(float norm, double min_v, double max_v) {
        if (norm <= 0.0f) return min_v;
        if (norm >= 1.0f) return max_v;
        return min_v + (double)norm * (max_v - min_v);
    }
};

} // namespace webots
