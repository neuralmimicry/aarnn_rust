#pragma once
#include <webots/Robot.hpp>
#include <webots/Device.hpp>
#include <webots/Motor.hpp>
#include <webots/Camera.hpp>
#include <webots/Accelerometer.hpp>
#include <webots/Gyro.hpp>
#include <webots/DistanceSensor.hpp>
#include <webots/LightSensor.hpp>
#include <webots/TouchSensor.hpp>
#include <webots/PositionSensor.hpp>
#include <webots/Node.hpp>

#include <vector>
#include <string>
#include <iostream>
#include <algorithm>
#include <map>
#include <cmath>
#include <cctype>
#include <cstdlib>
#include <cstdint>
#include <iomanip>
#include <sstream>
#include <thread>

namespace webots {

struct DeviceMapping {
    Device* device;
    int type;
    int size;
    std::vector<std::string> port_names;
    std::vector<double> min_values;
    std::vector<double> max_values;
    // Camera event encoder state (used only when type == Node::CAMERA).
    int camera_retina_rows = 0;
    int camera_retina_cols = 0;
    bool camera_state_initialized = false;
    std::vector<float> camera_curr_log_gray;
    std::vector<float> camera_prev_log_gray;
    std::vector<float> camera_event_state;
    std::vector<int> camera_refractory_left;
    std::vector<int8_t> camera_last_polarity;
};

class DeviceMapper {
public:
    DeviceMapper() {}

    void discover(Robot& robot, int timestep) {
        camera_retina_cols_ = read_env_int("NM_CAMERA_RETINA_WIDTH", 160, 1, 1024);
        camera_retina_rows_ = read_env_int("NM_CAMERA_RETINA_HEIGHT", 120, 1, 1024);
        dros_camera_retina_cols_ = read_env_int("NM_DROS_CAMERA_RETINA_WIDTH", camera_retina_cols_, 1, 1024);
        dros_camera_retina_rows_ = read_env_int("NM_DROS_CAMERA_RETINA_HEIGHT", camera_retina_rows_, 1, 1024);
        camera_event_threshold_ = read_env_float("NM_CAMERA_EVENT_THRESHOLD", 0.08f, 0.005f, 1.0f);
        camera_log_gain_ = read_env_float("NM_CAMERA_LOG_GAIN", 9.0f, 0.0f, 128.0f);
        camera_prev_blend_ = read_env_float("NM_CAMERA_EVENT_BASELINE_BLEND", 0.35f, 0.01f, 1.0f);
        camera_event_leak_ms_ = read_env_float("NM_CAMERA_EVENT_LEAK_MS", 45.0f, 1.0f, 5000.0f);
        camera_event_refractory_ms_ = read_env_float("NM_CAMERA_EVENT_REFRACTORY_MS", 12.0f, 0.0f, 5000.0f);
        camera_event_hysteresis_ = read_env_float("NM_CAMERA_EVENT_HYSTERESIS", 0.02f, 0.0f, 1.0f);
        camera_event_reset_scale_ = read_env_float("NM_CAMERA_EVENT_RESET_SCALE", 0.25f, 0.0f, 1.0f);
        camera_event_threads_ = read_env_int("NM_CAMERA_EVENT_THREADS", 0, 0, 128);
        camera_sample_period_ms_ = std::max(1.0f, (float)timestep);

        std::cout << "[DeviceMapper] discover (v1.5 - camera event encoder + per-camera retina override)"
                  << " camera_retina=" << camera_retina_cols_ << "x" << camera_retina_rows_
                  << " dros_camera_retina=" << dros_camera_retina_cols_ << "x" << dros_camera_retina_rows_
                  << " event_thr=" << camera_event_threshold_
                  << " log_gain=" << camera_log_gain_
                  << " baseline_blend=" << camera_prev_blend_
                  << " leak_ms=" << camera_event_leak_ms_
                  << " refractory_ms=" << camera_event_refractory_ms_
                  << " hysteresis=" << camera_event_hysteresis_
                  << " reset_scale=" << camera_event_reset_scale_
                  << " event_threads=" << camera_event_threads_
                  << std::endl;
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
            } else if (type == Node::CAMERA) {
                auto s = (Camera*)d;
                s->enable(timestep);
                int rows = camera_retina_rows_;
                int cols = camera_retina_cols_;
                resolve_camera_retina(name, rows, cols);
                add_camera_sensor(d, name, rows, cols);
            } else if (type == Node::GYRO) {
                auto s = (Gyro*)d;
                s->enable(timestep);
                add_sensor(d, type, 3, {name + ".x", name + ".y", name + ".z"}, -10.0, 10.0);
            } else if (type == Node::DISTANCE_SENSOR) {
                auto s = (DistanceSensor*)d;
                s->enable(timestep);
                add_sensor(d, type, 1, {name}, 0.0, s->getMaxValue());
            } else if (type == Node::LIGHT_SENSOR) {
                auto s = (LightSensor*)d;
                s->enable(timestep);
                // Webots LightSensor doesn't expose a uniform max accessor across versions.
                add_sensor(d, type, 1, {name}, 0.0, 1000.0);
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
            } else if (m.type == Node::CAMERA) {
                auto c = (Camera*)m.device;
                const unsigned char* image = c->getImage();
                const int width = c->getWidth();
                const int height = c->getHeight();
                const int rows = std::max(1, m.camera_retina_rows);
                const int cols = std::max(1, m.camera_retina_cols);
                const int cells = rows * cols;
                auto& curr_log_gray = m.camera_curr_log_gray;
                if ((int)curr_log_gray.size() != cells) {
                    curr_log_gray.assign((size_t)cells, 0.0f);
                } else {
                    std::fill(curr_log_gray.begin(), curr_log_gray.end(), 0.0f);
                }
                const int camera_threads = resolve_worker_threads(camera_event_threads_, rows);

                if (image && width > 0 && height > 0) {
                    const float log_norm = (camera_log_gain_ > 0.0f)
                        ? (float)std::log1p((double)camera_log_gain_)
                        : 1.0f;
                    parallel_for_range(0, rows, camera_threads, [&](int ry) {
                        const int y0 = (ry * height) / rows;
                        int y1 = ((ry + 1) * height) / rows;
                        if (y1 <= y0) y1 = std::min(height, y0 + 1);
                        for (int rx = 0; rx < cols; ++rx) {
                            const int x0 = (rx * width) / cols;
                            int x1 = ((rx + 1) * width) / cols;
                            if (x1 <= x0) x1 = std::min(width, x0 + 1);
                            double sum_gray = 0.0;
                            int count = 0;
                            for (int y = y0; y < y1; ++y) {
                                for (int x = x0; x < x1; ++x) {
                                    sum_gray += Camera::imageGetGray(image, width, x, y);
                                    ++count;
                                }
                            }
                            float mean_gray = 0.0f;
                            if (count > 0) {
                                mean_gray = (float)(sum_gray / (255.0 * (double)count));
                            }
                            mean_gray = std::max(0.0f, std::min(1.0f, mean_gray));
                            float encoded = mean_gray;
                            if (camera_log_gain_ > 0.0f) {
                                encoded = (float)(std::log1p((double)camera_log_gain_ * (double)mean_gray) / (double)log_norm);
                            }
                            curr_log_gray[ry * cols + rx] = std::max(0.0f, std::min(1.0f, encoded));
                        }
                    });
                }

                if (!m.camera_state_initialized || (int)m.camera_prev_log_gray.size() != cells) {
                    m.camera_prev_log_gray = curr_log_gray;
                    m.camera_curr_log_gray.assign((size_t)cells, 0.0f);
                    m.camera_event_state.assign((size_t)cells, 0.0f);
                    m.camera_refractory_left.assign((size_t)cells, 0);
                    m.camera_last_polarity.assign((size_t)cells, 0);
                    m.camera_state_initialized = true;
                }

                const float dt_ms = std::max(1.0f, camera_sample_period_ms_);
                const float leak_keep = (camera_event_leak_ms_ > 0.0f)
                    ? (float)std::exp(-(double)dt_ms / (double)camera_event_leak_ms_)
                    : 0.0f;
                const int refractory_steps = (camera_event_refractory_ms_ > 0.0f)
                    ? std::max(1, (int)std::ceil((double)camera_event_refractory_ms_ / (double)dt_ms))
                    : 0;
                const int cam_base = idx;
                idx += cells * 2;

                parallel_for_range(0, cells, resolve_worker_threads(camera_event_threads_, cells), [&](int i) {
                    const float prev = m.camera_prev_log_gray[i];
                    const float curr = curr_log_gray[i];
                    const float delta = curr - prev;
                    float mem = m.camera_event_state[i] * leak_keep + delta;

                    float on_event = 0.0f;
                    float off_event = 0.0f;
                    if (m.camera_refractory_left[i] > 0) {
                        --m.camera_refractory_left[i];
                    } else {
                        float on_thr = camera_event_threshold_;
                        float off_thr = -camera_event_threshold_;
                        if (m.camera_last_polarity[i] < 0) {
                            on_thr += camera_event_hysteresis_;
                        } else if (m.camera_last_polarity[i] > 0) {
                            off_thr -= camera_event_hysteresis_;
                        }

                        if (mem >= on_thr) {
                            on_event = 1.0f;
                            m.camera_last_polarity[i] = 1;
                            if (refractory_steps > 0) {
                                m.camera_refractory_left[i] = refractory_steps;
                            }
                            mem *= camera_event_reset_scale_;
                        } else if (mem <= off_thr) {
                            off_event = 1.0f;
                            m.camera_last_polarity[i] = -1;
                            if (refractory_steps > 0) {
                                m.camera_refractory_left[i] = refractory_steps;
                            }
                            mem *= camera_event_reset_scale_;
                        } else if (std::fabs(mem) < (camera_event_threshold_ * 0.5f)) {
                            // Return to neutral when membrane potential is near rest.
                            m.camera_last_polarity[i] = 0;
                        }
                    }

                    m.camera_event_state[i] = std::max(-1.0f, std::min(1.0f, mem));
                    const int out_i = cam_base + (2 * i);
                    buf[(size_t)out_i] = on_event;
                    buf[(size_t)out_i + 1] = off_event;

                    // Leaky photoreceptor adaptation baseline for temporal event coding.
                    const float blended = prev + camera_prev_blend_ * (curr - prev);
                    m.camera_prev_log_gray[i] = std::max(0.0f, std::min(1.0f, blended));
                });
            } else if (m.type == Node::GYRO) {
                const double* v = ((Gyro*)m.device)->getValues();
                for (int i = 0; i < 3; ++i) buf[idx++] = normalize(v[i], m.min_values[i], m.max_values[i]);
            } else if (m.type == Node::DISTANCE_SENSOR) {
                buf[idx++] = normalize(((DistanceSensor*)m.device)->getValue(), m.min_values[0], m.max_values[0]);
            } else if (m.type == Node::LIGHT_SENSOR) {
                buf[idx++] = normalize(((LightSensor*)m.device)->getValue(), m.min_values[0], m.max_values[0]);
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
    int camera_retina_rows_ = 120;
    int camera_retina_cols_ = 160;
    int dros_camera_retina_rows_ = 8;
    int dros_camera_retina_cols_ = 12;
    float camera_event_threshold_ = 0.08f;
    float camera_log_gain_ = 9.0f;
    float camera_prev_blend_ = 0.35f;
    float camera_event_leak_ms_ = 45.0f;
    float camera_event_refractory_ms_ = 12.0f;
    float camera_event_hysteresis_ = 0.02f;
    float camera_event_reset_scale_ = 0.25f;
    int camera_event_threads_ = 0;
    float camera_sample_period_ms_ = 16.0f;

    static int resolve_worker_threads(int requested, int work_items) {
        if (work_items <= 1) return 1;
        int threads = requested;
        if (threads <= 0) {
            unsigned int hw = std::thread::hardware_concurrency();
            threads = (hw > 0) ? (int)hw : 1;
            threads = std::min(threads, 8); // Avoid oversubscribing with per-frame worker creation.
        }
        return std::max(1, std::min(threads, work_items));
    }

    template <typename Fn>
    static void parallel_for_range(int begin, int end, int threads, Fn fn) {
        const int work = end - begin;
        if (work <= 0) return;
        if (threads <= 1 || work <= 1) {
            for (int i = begin; i < end; ++i) fn(i);
            return;
        }

        std::vector<std::thread> pool;
        pool.reserve((size_t)std::max(0, threads - 1));
        for (int t = 0; t < threads; ++t) {
            const int s = begin + (t * work) / threads;
            const int e = begin + ((t + 1) * work) / threads;
            if (s >= e) continue;
            if (t + 1 == threads) {
                for (int i = s; i < e; ++i) fn(i);
            } else {
                pool.emplace_back([s, e, &fn]() {
                    for (int i = s; i < e; ++i) fn(i);
                });
            }
        }
        for (auto& th : pool) th.join();
    }

    static int read_env_int(const char* key, int fallback, int min_v, int max_v) {
        const char* raw = std::getenv(key);
        if (!raw || !*raw) return fallback;
        char* end = nullptr;
        long parsed = std::strtol(raw, &end, 10);
        if (end == raw || parsed < min_v || parsed > max_v) return fallback;
        return (int)parsed;
    }

    static int read_env_int_str(const std::string& key, int fallback, int min_v, int max_v) {
        const char* raw = std::getenv(key.c_str());
        if (!raw || !*raw) return fallback;
        char* end = nullptr;
        long parsed = std::strtol(raw, &end, 10);
        if (end == raw || parsed < min_v || parsed > max_v) return fallback;
        return (int)parsed;
    }

    static float read_env_float(const char* key, float fallback, float min_v, float max_v) {
        const char* raw = std::getenv(key);
        if (!raw || !*raw) return fallback;
        char* end = nullptr;
        float parsed = std::strtof(raw, &end);
        if (end == raw || !std::isfinite(parsed) || parsed < min_v || parsed > max_v) return fallback;
        return parsed;
    }

    static int index_digits(int n) {
        int max_index = std::max(0, n - 1);
        int digits = 1;
        while (max_index >= 10) {
            max_index /= 10;
            ++digits;
        }
        return std::max(2, digits);
    }

    static std::string camera_channel_name(
        const std::string& camera_name,
        const std::string& polarity,
        int row,
        int col,
        int row_digits,
        int col_digits
    ) {
        std::ostringstream ss;
        ss << camera_name
           << "."
           << polarity
           << ".r"
           << std::setw(row_digits) << std::setfill('0') << row
           << "c"
           << std::setw(col_digits) << std::setfill('0') << col;
        return ss.str();
    }

    static std::string sanitize_env_suffix(const std::string& raw) {
        std::string out;
        out.reserve(raw.size() + 8);
        for (char ch : raw) {
            unsigned char u = static_cast<unsigned char>(ch);
            if (std::isalnum(u)) {
                out.push_back((char)std::toupper(u));
            } else {
                out.push_back('_');
            }
        }
        if (out.empty()) return "CAMERA";
        return out;
    }

    void resolve_camera_retina(const std::string& camera_name, int& rows, int& cols) const {
        rows = camera_retina_rows_;
        cols = camera_retina_cols_;
        if (camera_name.rfind("dros_", 0) == 0) {
            rows = dros_camera_retina_rows_;
            cols = dros_camera_retina_cols_;
        }
        const std::string suffix = sanitize_env_suffix(camera_name);
        cols = read_env_int_str("NM_CAMERA_RETINA_WIDTH_" + suffix, cols, 1, 1024);
        rows = read_env_int_str("NM_CAMERA_RETINA_HEIGHT_" + suffix, rows, 1, 1024);
    }

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

    void add_camera_sensor(Device* d, const std::string& camera_name, int rows, int cols) {
        DeviceMapping m;
        m.device = d;
        m.type = Node::CAMERA;
        m.camera_retina_rows = rows;
        m.camera_retina_cols = cols;
        const int cells = rows * cols;
        m.size = cells * 2; // ON + OFF event channel per retina cell
        m.port_names.reserve((size_t)m.size);
        const int row_digits = index_digits(rows);
        const int col_digits = index_digits(cols);
        for (int r = 0; r < rows; ++r) {
            for (int c = 0; c < cols; ++c) {
                m.port_names.push_back(
                    camera_channel_name(camera_name, "on", r, c, row_digits, col_digits)
                );
                m.port_names.push_back(
                    camera_channel_name(camera_name, "off", r, c, row_digits, col_digits)
                );
            }
        }
        m.camera_prev_log_gray.assign((size_t)cells, 0.0f);
        m.camera_curr_log_gray.assign((size_t)cells, 0.0f);
        m.camera_event_state.assign((size_t)cells, 0.0f);
        m.camera_refractory_left.assign((size_t)cells, 0);
        m.camera_last_polarity.assign((size_t)cells, 0);
        m.camera_state_initialized = false;
        for (int i = 0; i < m.size; ++i) {
            m.min_values.push_back(0.0);
            m.max_values.push_back(1.0);
        }
        sensors_.push_back(std::move(m));
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
