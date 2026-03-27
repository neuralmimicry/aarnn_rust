// Webots NAO controller that talks to the NN server over Unix datagram sockets (UDS)
// Instead of linking the Rust FFI in-process, this controller sends sensor frames
// to a separate Rust process (nn_uds_server) and receives actuator outputs back.
//
// Build (via repo Makefile):
//   make -C examples nao_nn_controller_uds [PROFILE=release|debug]
// Run in Webots (nao_room.wbt):
//   - Set the NAO robot's Controller to "nao_nn_controller_uds"
//   - Start the NN server in a terminal first, e.g.:
//       cargo run --release --features ui,robot_io --example nn_uds_server --
//         --socket /tmp/aarnn_rust.nn --sensory 25 --output 11 --threshold 0.2 --ui
//   - Play the simulation.

#include <webots/Robot.hpp>
#include <webots/Motor.hpp>
#include <webots/Accelerometer.hpp>
#include <webots/Gyro.hpp>
#include <webots/DistanceSensor.hpp>
#include <webots/Keyboard.hpp>
#include <webots/utils/motion.h>
#include <device_mapper.hpp>
#include <keyboard_mapper.hpp>

#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#include <fcntl.h>

#include <algorithm>
#include <cerrno>
#include <cctype>
#include <chrono>
#include <cmath>
#include <cstring>
#include <cstdlib>
#include <deque>
#include <cstdint>
#include <iostream>
#include <string>
#include <vector>

#include <memory>
#include <regex>
#include <map>
#include <set>
#include <sstream>

using namespace webots;

static WbMotionRef hand_wave, forwards, backwards, side_step_left, side_step_right, turn_left_60, turn_right_60, tai_chi, wipe_forehead;
static WbMotionRef currently_playing = NULL;

static int uds_recv_timeout_ms() {
  constexpr int kDefaultMs = 100;
  constexpr int kMinMs = 5;
  constexpr int kMaxMs = 5000;
  const char* raw = std::getenv("NM_UDS_RECV_TIMEOUT_MS");
  if (!raw || !*raw) return kDefaultMs;
  char* end = nullptr;
  long parsed = std::strtol(raw, &end, 10);
  if (end == raw || parsed < kMinMs || parsed > kMaxMs) return kDefaultMs;
  return static_cast<int>(parsed);
}

static float ipc_dt_ms_override(float fallback_ms) {
  const char* raw = std::getenv("NM_IPC_DT_MS");
  if (!raw || !*raw) return fallback_ms;
  char* end = nullptr;
  float parsed = std::strtof(raw, &end);
  if (end == raw || !std::isfinite(parsed) || parsed <= 0.0f || parsed > 1000.0f) {
    return fallback_ms;
  }
  return parsed;
}

static int env_int_range(const char* key, int fallback, int min_v, int max_v) {
  const char* raw = std::getenv(key);
  if (!raw || !*raw) return fallback;
  char* end = nullptr;
  long parsed = std::strtol(raw, &end, 10);
  if (end == raw || parsed < min_v || parsed > max_v) return fallback;
  return static_cast<int>(parsed);
}

static bool env_bool(const char* key, bool fallback) {
  const char* raw = std::getenv(key);
  if (!raw || !*raw) return fallback;
  std::string v(raw);
  std::transform(v.begin(), v.end(), v.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
  if (v == "1" || v == "true" || v == "yes" || v == "on") return true;
  if (v == "0" || v == "false" || v == "no" || v == "off") return false;
  return fallback;
}

static float env_float_range(const char* key, float fallback, float min_v, float max_v) {
  const char* raw = std::getenv(key);
  if (!raw || !*raw) return fallback;
  char* end = nullptr;
  float parsed = std::strtof(raw, &end);
  if (end == raw || !std::isfinite(parsed) || parsed < min_v || parsed > max_v) return fallback;
  return parsed;
}

static bool set_fd_nonblocking(int fd) {
  int flags = ::fcntl(fd, F_GETFL, 0);
  if (flags < 0) {
    return false;
  }
  if (flags & O_NONBLOCK) {
    return true;
  }
  return ::fcntl(fd, F_SETFL, flags | O_NONBLOCK) == 0;
}

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

// Minimal UDS datagram helper
class UdsClient {
public:
  UdsClient(const std::string& server_path)
  : server_path_(server_path) {
    sock_ = ::socket(AF_UNIX, SOCK_DGRAM, 0);
    if (sock_ < 0) { perror("socket"); throw std::runtime_error("socket"); }
    if (!set_fd_nonblocking(sock_)) {
      perror("[nao_nn_controller_uds] fcntl(O_NONBLOCK)");
      throw std::runtime_error("nonblocking");
    }

    // Keep recv timeout short to avoid stalling Webots control steps for seconds
    // when the NN backend is overloaded or briefly unavailable.
    recv_timeout_ms_ = uds_recv_timeout_ms();
    struct timeval tv;
    tv.tv_sec = recv_timeout_ms_ / 1000;
    tv.tv_usec = (recv_timeout_ms_ % 1000) * 1000;
    if (::setsockopt(sock_, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv)) < 0) {
      perror("setsockopt");
    }

    // Bind client to a unique path in the same directory as the server socket
    // This helps bypass sandboxing issues where /tmp might be isolated.
    std::string client_path = server_path_ + ".ctrl_" + std::to_string(::getpid());
    struct sockaddr_un addr{};
    addr.sun_family = AF_UNIX;
    std::snprintf(addr.sun_path, sizeof(addr.sun_path), "%s", client_path.c_str());
    ::unlink(addr.sun_path);
    if (::bind(sock_, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
      perror("[nao_nn_controller_uds] bind"); throw std::runtime_error("bind");
    }
    std::cout << "[nao_nn_controller_uds] Bound local socket to " << client_path
              << " (recv timeout " << recv_timeout_ms_ << "ms)" << std::endl;
    // Prepare server addr
    server_.sun_family = AF_UNIX;
    std::snprintf(server_.sun_path, sizeof(server_.sun_path), "%s", server_path_.c_str());

    min_window_ = env_int_range("NM_IPC_WINDOW_MIN", 1, 1, 128);
    max_window_ = env_int_range("NM_IPC_WINDOW_MAX", 16, min_window_, 256);
    int init_window = env_int_range("NM_IPC_WINDOW_INIT", 2, min_window_, max_window_);
    cwnd_ = static_cast<double>(init_window);
    max_send_per_pump_ = env_int_range("NM_IPC_SEND_BUDGET_MAX", 8, 1, 128);
    max_queue_frames_ = env_int_range("NM_IPC_QUEUE_MAX_FRAMES", 256, 1, 16384);
    force_aer_ = env_bool("NM_IPC_FORCE_AER", false);
    disable_aer_ = env_bool("NM_IPC_DISABLE_AER", false);
    max_raw_payload_bytes_ = env_int_range("NM_IPC_MAX_RAW_BYTES", 60000, 256, 1 << 20);
    aer_event_threshold_ = env_float_range("NM_IPC_AER_THRESHOLD", 0.5f, 0.0f, 1.0f);
    aer_max_events_ = env_int_range("NM_IPC_AER_MAX_EVENTS", 20000, 1, 1 << 20);
    aer_max_packet_bytes_ = env_int_range("NM_IPC_AER_MAX_PACKET_BYTES", 60000, 256, 1 << 20);
    aer_runtime_packet_bytes_ = aer_max_packet_bytes_;
    aer_sensory_base_ = (uint32_t)std::max(0, env_int_range("NM_AER_S_BASE", 0, 0, 1 << 30));

    std::cout << "[nao_nn_controller_uds] IPC flow window init/min/max="
              << init_window << "/" << min_window_ << "/" << max_window_
              << " send_budget_max=" << max_send_per_pump_
              << " queue_max=" << max_queue_frames_
              << " mode=nonblocking_async"
              << " aer(force=" << (force_aer_ ? 1 : 0)
              << ",disable=" << (disable_aer_ ? 1 : 0)
              << ",thr=" << aer_event_threshold_
              << ",max_events=" << aer_max_events_
              << ",max_pkt=" << aer_max_packet_bytes_
              << ",base=" << aer_sensory_base_
              << ",raw_limit=" << max_raw_payload_bytes_
              << ")"
              << std::endl;
  }
  ~UdsClient(){ if (sock_>=0) ::close(sock_); }

  bool send_raw(const std::string& data) {
    ssize_t sent = ::sendto(sock_, data.c_str(), data.size(), 0, (struct sockaddr*)&server_, sizeof(server_));
    if (sent < 0) {
      return false;
    }
    return (size_t)sent == data.size();
  }

  // Queue + flow-control aware xfer:
  // - New unsent full-frame updates stale older unsent full-frame updates.
  // - Sliding window auto-resizes based on reply RTT and timeout/backpressure.
  // - Replies are drained in bursts; the latest reply wins per actuator channel.
  bool xfer(const std::vector<float>& req, std::vector<float>& rep, double now_s){
    enqueue_or_replace(req, now_s);
    expire_inflight(now_s);

    bool have_reply = false;
    int drain_status = drain_replies(rep, now_s, have_reply);
    if (drain_status < 0) {
      return false;
    }

    int send_budget = compute_send_budget();
    for (int i = 0; i < send_budget; ++i) {
      if (tx_queue_.empty()) break;
      PendingFrame frame = std::move(tx_queue_.front());
      tx_queue_.pop_front();

      const bool use_aer = should_use_aer(frame.payload);
      std::vector<uint8_t> aer_payload;
      const char* send_data = nullptr;
      size_t nbytes = 0;
      if (use_aer) {
        aer_payload = encode_aer_payload(frame.payload, aer_runtime_packet_bytes_);
        send_data = reinterpret_cast<const char*>(aer_payload.data());
        nbytes = aer_payload.size();
      } else {
        send_data = reinterpret_cast<const char*>(frame.payload.data());
        nbytes = frame.payload.size() * sizeof(float);
      }

      ssize_t sent = ::sendto(
        sock_,
        send_data,
        nbytes,
        0,
        (struct sockaddr*)&server_,
        sizeof(server_)
      );
      if (sent < 0) {
        if (errno == EMSGSIZE && use_aer) {
          // Datagram still too large for current socket constraints.
          // Shrink AER packet budget dynamically and keep latest frame queued.
          aer_runtime_packet_bytes_ = std::max(1024, (aer_runtime_packet_bytes_ * 3) / 4);
          on_backpressure(now_s);
          tx_queue_.push_front(std::move(frame));
          break;
        }
        if (errno == EAGAIN || errno == EWOULDBLOCK || errno == ENOBUFS) {
          on_backpressure(now_s);
          // Preserve latest unsent update for next iteration.
          tx_queue_.push_front(std::move(frame));
          break;
        }
        return false;
      }
      if ((size_t)sent != nbytes) {
        on_backpressure(now_s);
        tx_queue_.push_front(std::move(frame));
        errno = EAGAIN;
        break;
      }

      inflight_.push_back({frame.seq, now_s});
    }

    // Strictly non-blocking post-drain: harvest replies already available now.
    // No sleeps and no waits so Webots control loop never blocks on NN IPC.
    int post_attempts = std::min(4, std::max(1, window_size() / 4 + 1));
    for (int attempt = 0; attempt < post_attempts; ++attempt) {
      int post_status = drain_replies(rep, now_s, have_reply);
      if (post_status < 0) {
        return false;
      }
      if (have_reply) break;
    }

    if (have_reply) {
      return true;
    }
    errno = EAGAIN;
    return false;
  }

  int window_size() const {
    return std::max(min_window_, static_cast<int>(std::floor(cwnd_)));
  }

  size_t pending_updates() const {
    return tx_queue_.size();
  }

  size_t inflight_updates() const {
    return inflight_.size();
  }

  size_t stale_updates_dropped() const {
    return stale_updates_dropped_;
  }

private:
  struct PendingFrame {
    std::vector<float> payload;
    double enqueue_s = 0.0;
    uint64_t seq = 0;
  };

  struct InflightFrame {
    uint64_t seq = 0;
    double sent_s = 0.0;
  };

  static void append_varint(uint64_t value, std::vector<uint8_t>& out) {
    while (value >= 0x80u) {
      out.push_back(static_cast<uint8_t>((value & 0x7fu) | 0x80u));
      value >>= 7;
    }
    out.push_back(static_cast<uint8_t>(value));
  }

  bool should_use_aer(const std::vector<float>& payload) const {
    if (disable_aer_) return false;
    if (force_aer_) return true;
    const size_t raw_bytes = payload.size() * sizeof(float);
    return raw_bytes > (size_t)max_raw_payload_bytes_;
  }

  std::vector<uint8_t> encode_aer_payload(
    const std::vector<float>& payload,
    int packet_budget_bytes
  ) {
    // AER wire format: "AER1" + base_ts_us(u64 LE) + varint(delta_ts, addr, value)...
    std::vector<uint8_t> out;
    out.reserve(64);
    out.push_back('A');
    out.push_back('E');
    out.push_back('R');
    out.push_back('1');

    const uint64_t ts_us = static_cast<uint64_t>(
      std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()
      ).count()
    );
    for (int i = 0; i < 8; ++i) {
      out.push_back(static_cast<uint8_t>((ts_us >> (8 * i)) & 0xffu));
    }

    if (payload.size() <= 1) {
      return out;
    }

    int emitted = 0;
    const size_t sensory_count = payload.size() - 1;
    size_t start = 0;
    if (sensory_count > 0) {
      start = aer_emit_phase_ % sensory_count;
      aer_emit_phase_ = (aer_emit_phase_ + 9973u) % sensory_count;
    }

    auto emit_index = [&](size_t sensory_idx) {
      const float v = payload[sensory_idx + 1];
      if (!std::isfinite(v) || v < aer_event_threshold_) return false;

      const size_t old_size = out.size();
      append_varint(0u, out);  // same timestamp as base frame timestamp
      append_varint(static_cast<uint64_t>(aer_sensory_base_ + static_cast<uint32_t>(sensory_idx)), out);
      append_varint(1u, out);
      emitted += 1;

      if (emitted >= aer_max_events_ || static_cast<int>(out.size()) > packet_budget_bytes) {
        if (static_cast<int>(out.size()) > packet_budget_bytes) {
          out.resize(old_size);
        }
        return true;
      }
      return false;
    };

    // payload[0] is dt_ms, payload[1..] are sensory values.
    for (size_t off = 0; off < sensory_count; ++off) {
      size_t idx = start + off;
      if (idx >= sensory_count) idx -= sensory_count;
      if (emit_index(idx)) {
        return out;
      }
    }
    return out;
  }

  void enqueue_or_replace(const std::vector<float>& req, double now_s) {
    // Full-frame updates supersede any older unsent full-frame updates.
    if (!tx_queue_.empty()) {
      stale_updates_dropped_ += tx_queue_.size();
      tx_queue_.clear();
    }

    if ((int)tx_queue_.size() >= max_queue_frames_) {
      stale_updates_dropped_ += 1;
      tx_queue_.pop_front();
    }

    PendingFrame frame;
    frame.payload = req;
    frame.enqueue_s = now_s;
    frame.seq = next_seq_++;
    tx_queue_.push_back(std::move(frame));
  }

  int compute_send_budget() const {
    int in_flight = static_cast<int>(inflight_.size());
    int credit = window_size() - in_flight;
    if (credit <= 0) return 0;
    credit = std::min(credit, max_send_per_pump_);
    credit = std::min(credit, static_cast<int>(tx_queue_.size()));
    return std::max(0, credit);
  }

  void on_reply_acked(double now_s) {
    if (!inflight_.empty()) {
      double rtt_ms = (now_s - inflight_.front().sent_s) * 1000.0;
      if (rtt_ms < 0.0) rtt_ms = 0.0;
      inflight_.pop_front();

      if (srtt_ms_ <= 0.0) {
        srtt_ms_ = rtt_ms;
        rttvar_ms_ = rtt_ms * 0.5;
      } else {
        double err = std::fabs(srtt_ms_ - rtt_ms);
        rttvar_ms_ = 0.75 * rttvar_ms_ + 0.25 * err;
        srtt_ms_ = 0.875 * srtt_ms_ + 0.125 * rtt_ms;
      }
    }

    // Additive increase when progress is observed.
    if (cwnd_ < static_cast<double>(max_window_)) {
      double inc = std::max(0.05, 1.0 / std::max(1.0, cwnd_));
      cwnd_ = std::min(static_cast<double>(max_window_), cwnd_ + inc);
    }
  }

  void on_backpressure(double now_s) {
    (void)now_s;
    cwnd_ = std::max(static_cast<double>(min_window_), cwnd_ * 0.70);
  }

  void expire_inflight(double now_s) {
    double timeout_ms = static_cast<double>(recv_timeout_ms_) * 2.0;
    if (srtt_ms_ > 0.0) {
      timeout_ms = std::max(timeout_ms, srtt_ms_ + 4.0 * std::max(1.0, rttvar_ms_));
    }
    double timeout_s = timeout_ms / 1000.0;

    bool expired_any = false;
    while (!inflight_.empty()) {
      if ((now_s - inflight_.front().sent_s) <= timeout_s) break;
      inflight_.pop_front();
      expired_any = true;
    }
    if (expired_any) {
      on_backpressure(now_s);
    }
  }

  int drain_replies(std::vector<float>& rep_out, double now_s, bool& have_reply) {
    std::vector<char> buf(rep_out.size() * sizeof(float));
    constexpr int kMaxDrains = 32;
    for (int i = 0; i < kMaxDrains; ++i) {
      struct sockaddr_un from{};
      socklen_t fromlen = sizeof(from);
      ssize_t r = ::recvfrom(
        sock_,
        buf.data(),
        buf.size(),
        MSG_DONTWAIT,
        (struct sockaddr*)&from,
        &fromlen
      );
      if (r < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
          return have_reply ? 1 : 0;
        }
        return -1;
      }
      if ((size_t)r == buf.size()) {
        std::memcpy(rep_out.data(), buf.data(), buf.size());
        have_reply = true;
        on_reply_acked(now_s);
        continue;
      }
      if (r > 0 && (buf[0] == '{' || buf[0] == '[')) {
        // Skip textual control packets from the IPC server.
        continue;
      }
      // Ignore malformed datagrams and continue draining.
    }
    return have_reply ? 1 : 0;
  }

  int sock_ = -1;
  int recv_timeout_ms_ = 100;
  int min_window_ = 1;
  int max_window_ = 16;
  int max_send_per_pump_ = 8;
  int max_queue_frames_ = 256;
  bool force_aer_ = false;
  bool disable_aer_ = false;
  int max_raw_payload_bytes_ = 60000;
  float aer_event_threshold_ = 0.5f;
  int aer_max_events_ = 20000;
  int aer_max_packet_bytes_ = 60000;
  int aer_runtime_packet_bytes_ = 60000;
  size_t aer_emit_phase_ = 0;
  uint32_t aer_sensory_base_ = 0;
  double cwnd_ = 2.0;
  double srtt_ms_ = 0.0;
  double rttvar_ms_ = 0.0;
  uint64_t next_seq_ = 1;
  size_t stale_updates_dropped_ = 0;
  std::deque<PendingFrame> tx_queue_;
  std::deque<InflightFrame> inflight_;
  std::string server_path_;
  struct sockaddr_un server_{};
};

struct BrainInstance {
    std::string id;
    std::string sock_path;
    std::unique_ptr<UdsClient> cli;
    std::string handshake_json;
    bool connected = false;
    double last_handshake_time = -10.0;
    double last_error_time = -10.0;
    size_t stale_dropped_last_log = 0;

    // Mapping info
    std::vector<int> real_sensor_indices;   // Indices in DeviceMapper sensory buffer
    std::vector<int> real_actuator_indices; // Indices in DeviceMapper actuator buffer
    
    // Interconnects
    struct Link {
        std::string peer_id;
        int count;
        int local_start; // index in this brain's S or O buffer
    };
    std::vector<Link> virtual_sensors;   // From peer outputs to our inputs
    std::vector<Link> virtual_actuators; // From our outputs to peer inputs

    // Buffers for this specific brain
    std::vector<float> s_buf;
    std::vector<float> a_buf;
    std::vector<std::string> s_names;
    std::vector<std::string> o_names;
};

static std::vector<std::string> split(const std::string& s, char delim) {
    std::vector<std::string> result;
    std::stringstream ss(s);
    std::string item;
    while (std::getline(ss, item, delim)) {
        if (!item.empty()) result.push_back(item);
    }
    return result;
}

struct CelegansMuscleBridge {
    std::vector<int> spine_indices;  // 1-based segment joints, size 24
    std::vector<int> mdl_indices;
    std::vector<int> mdr_indices;
    std::vector<int> mvl_indices;
    std::vector<int> mvr_indices;
    std::vector<float> mdl_trace;
    std::vector<float> mdr_trace;
    std::vector<float> mvl_trace;
    std::vector<float> mvr_trace;
    int mvulva_index = -1;
    float mvulva_trace = 0.0f;
    std::vector<float> smoothed_targets;
    bool active = false;

    CelegansMuscleBridge() {
        spine_indices.assign(24, -1);
        mdl_indices.assign(24, -1);
        mdr_indices.assign(24, -1);
        mvl_indices.assign(24, -1);
        mvr_indices.assign(24, -1);
        mdl_trace.assign(24, 0.0f);
        mdr_trace.assign(24, 0.0f);
        mvl_trace.assign(24, 0.0f);
        mvr_trace.assign(24, 0.0f);
        smoothed_targets.assign(24, 0.5f);
    }

    void discover(const std::vector<std::string>& actuator_names) {
        std::regex spine_re("^celegans_spine_([0-9]{2})$");
        std::regex muscle_re("^celegans_o_[0-9]{3}_(MDL|MDR|MVL|MVR)([0-9]{2})$");
        std::regex mvulva_re("^celegans_o_[0-9]{3}_MVULVA$");
        std::smatch m;

        int matched_spine = 0;
        int matched_muscle = 0;

        for (size_t i = 0; i < actuator_names.size(); ++i) {
            const std::string& name = actuator_names[i];
            if (std::regex_match(name, m, spine_re)) {
                int seg = std::stoi(m[1]);
                if (seg >= 1 && seg <= 24) {
                    spine_indices[seg - 1] = (int)i;
                    matched_spine++;
                }
                continue;
            }
            if (std::regex_match(name, m, muscle_re)) {
                const std::string grp = m[1];
                int seg = std::stoi(m[2]);
                if (seg < 1 || seg > 24) continue;
                int idx = seg - 1;
                if (grp == "MDL") mdl_indices[idx] = (int)i;
                else if (grp == "MDR") mdr_indices[idx] = (int)i;
                else if (grp == "MVL") mvl_indices[idx] = (int)i;
                else if (grp == "MVR") mvr_indices[idx] = (int)i;
                matched_muscle++;
                continue;
            }
            if (std::regex_match(name, mvulva_re)) {
                mvulva_index = (int)i;
                matched_muscle++;
            }
        }

        active = matched_spine >= 8 && matched_muscle >= 24;
        if (active) {
            std::cout << "[nao_nn_controller_uds] Celegans muscle bridge active"
                      << " (spine motors=" << matched_spine
                      << ", muscle channels=" << matched_muscle << ")"
                      << std::endl;
        }
    }

    inline float read(const std::vector<float>& all_a, int idx) const {
        if (idx < 0 || idx >= (int)all_a.size()) return 0.5f;
        return all_a[idx];
    }

    inline float contract_from_output(float raw, float& trace) {
        // Accept both binary spikes and graded commands:
        // - binary 0/1 outputs still generate twitch-like contractions
        // - graded outputs around 0.5 remain neutral; >0.5 increases contraction
        float clamped = raw;
        if (!std::isfinite(clamped)) clamped = 0.5f;
        if (clamped < 0.0f) clamped = 0.0f;
        if (clamped > 1.0f) clamped = 1.0f;
        const float graded_drive = std::max(0.0f, (clamped - 0.5f) * 2.0f);
        const float spike_boost = clamped >= 0.999f ? 1.0f : 0.0f;
        const float drive = std::max(graded_drive, spike_boost);
        trace = 0.92f * trace + 0.62f * drive;
        if (trace > 1.0f) trace = 1.0f;
        if (trace < 0.0f) trace = 0.0f;
        return 0.5f + 0.5f * trace; // neutral-centered contraction command
    }

    void apply(std::vector<float>& all_a) {
        if (!active) return;

        // Convert dorsal/ventral + left/right muscle outputs into a smoothed
        // undulation target for each spine joint.
        for (int seg = 0; seg < 24; ++seg) {
            int spine_idx = spine_indices[seg];
            if (spine_idx < 0 || spine_idx >= (int)all_a.size()) continue;

            float mdl = contract_from_output(read(all_a, mdl_indices[seg]), mdl_trace[seg]);
            float mdr = contract_from_output(read(all_a, mdr_indices[seg]), mdr_trace[seg]);
            float mvl = contract_from_output(read(all_a, mvl_indices[seg]), mvl_trace[seg]);
            float mvr = contract_from_output(read(all_a, mvr_indices[seg]), mvr_trace[seg]);
            float mvulva = contract_from_output(read(all_a, mvulva_index), mvulva_trace);

            float dorsal = 0.5f * (mdl + mdr);
            float ventral = 0.5f * (mvl + mvr);
            float left = 0.5f * (mdl + mvl);
            float right = 0.5f * (mdr + mvr);

            // MVULVA is a ventral mid-body channel. Blend it into central
            // segments so this output contributes to body undulation.
            if (mvulva_index >= 0 && seg >= 9 && seg <= 14) {
                float center_weight = 1.0f - (std::abs(seg - 11.5f) / 3.0f);
                if (center_weight < 0.0f) center_weight = 0.0f;
                ventral = (1.0f - 0.22f * center_weight) * ventral + (0.22f * center_weight) * mvulva;
            }

            // C. elegans body-wall locomotion is dorsal/ventral along the body.
            // Keep body joints dominated by D-V drive; only allow small
            // left/right quadrant asymmetry near the head.
            float drive = ventral - dorsal;
            if (seg <= 3) {
                float head_lr_bias = right - left;
                drive += 0.16f * head_lr_bias;
            }

            if (seg < 5 || seg > 20) {
                drive *= 0.72f;  // taper head/tail deflection
            }

            float target = 0.5f + 0.44f * drive;
            if (target < 0.05f) target = 0.05f;
            if (target > 0.95f) target = 0.95f;

            smoothed_targets[seg] = 0.78f * smoothed_targets[seg] + 0.22f * target;
            all_a[spine_idx] = smoothed_targets[seg];
        }
    }
};

int main(int argc, char** argv) {
    // 0. Parse pseudo-environment variables from controllerArgs
    // This allows setting NM_BRAINS=vision,motor etc in Webots controllerArgs
    for (int i = 1; i < argc; ++i) {
        std::string arg = argv[i];
        size_t eq = arg.find('=');
        if (eq != std::string::npos && eq > 0) {
            std::string key = arg.substr(0, eq);
            std::string val = arg.substr(eq + 1);
            setenv(key.c_str(), val.c_str(), 1);
        }
    }

    // 1. Discover Brains
    const char* brains_env = std::getenv("NM_BRAINS");
    std::vector<std::string> brain_ids = brains_env ? split(brains_env, ',') : std::vector<std::string>{"default"};
    
    std::vector<BrainInstance> brains;
    const char* home = std::getenv("HOME");
    std::string home_str = home ? home : "/tmp";

    for (const auto& id : brain_ids) {
        BrainInstance b;
        b.id = id;
        if (id == "default") {
            b.sock_path = home_str + "/aarnn_rust.nn";
        } else {
            b.sock_path = home_str + "/aarnn_rust." + id + ".nn";
        }
        brains.push_back(std::move(b));
    }

    // 2. Webots setup
    Robot robot;
    const int dt = (int)robot.getBasicTimeStep();
    const float ipc_dt_ms = ipc_dt_ms_override(1.0f);
    DeviceMapper mapper;
    mapper.discover(robot, dt);
    std::cout << "[nao_nn_controller_uds] Webots timestep=" << dt
              << "ms, NN IPC dt=" << ipc_dt_ms << "ms" << std::endl;

    KeyboardMapper kb_mapper;
    kb_mapper.autodetect(robot);
    if (kb_mapper.is_enabled()) {
        load_motions();
    }

    auto all_s_names = mapper.get_sensor_names();
    auto all_o_names = mapper.get_actuator_names();
    CelegansMuscleBridge celegans_bridge;
    celegans_bridge.discover(all_o_names);
    const bool handshake_include_names = env_bool("NM_IPC_HANDSHAKE_INCLUDE_NAMES", false);
    const int handshake_max_bytes = env_int_range("NM_IPC_HANDSHAKE_MAX_BYTES", 7000, 256, 1 << 20);

    // 3. Configure Mapping and Interconnects
    // Interconnect format: NM_INTERCONNECT=brain1->brain2:8,brain2->brain1:4
    const char* inter_env = std::getenv("NM_INTERCONNECT");
    std::vector<std::string> inter_links = inter_env ? split(inter_env, ',') : std::vector<std::string>();

    for (auto& b : brains) {
        // Real sensors ownership (default all)
        std::string s_regex_str = ".*";
        char* s_env = std::getenv(("NM_SENSORS_" + b.id).c_str());
        if (s_env) s_regex_str = s_env;
        std::regex s_regex(s_regex_str);

        for (size_t i = 0; i < all_s_names.size(); ++i) {
            if (std::regex_match(all_s_names[i], s_regex)) {
                b.real_sensor_indices.push_back(i);
                b.s_names.push_back(all_s_names[i]);
            }
        }

        // Real actuators ownership (default all)
        std::string o_regex_str = ".*";
        char* o_env = std::getenv(("NM_ACTUATORS_" + b.id).c_str());
        if (o_env) o_regex_str = o_env;
        std::regex o_regex(o_regex_str);

        for (size_t i = 0; i < all_o_names.size(); ++i) {
            if (std::regex_match(all_o_names[i], o_regex)) {
                b.real_actuator_indices.push_back(i);
                b.o_names.push_back(all_o_names[i]);
            }
        }

        // Virtual sensors (Inputs to this brain)
        for (const auto& link_str : inter_links) {
            // parse brainA->brainB:count
            std::smatch m;
            if (std::regex_match(link_str, m, std::regex("(.+)->(.+):(\\d+)"))) {
                std::string src = m[1];
                std::string dst = m[2];
                int count = std::stoi(m[3]);
                if (dst == b.id) {
                    BrainInstance::Link l{src, count, (int)b.s_names.size()};
                    for (int i=0; i<count; ++i) b.s_names.push_back("From_" + src + "_" + std::to_string(i));
                    b.virtual_sensors.push_back(l);
                }
                if (src == b.id) {
                    BrainInstance::Link l{dst, count, (int)b.o_names.size()};
                    for (int i=0; i<count; ++i) b.o_names.push_back("To_" + dst + "_" + std::to_string(i));
                    b.virtual_actuators.push_back(l);
                }
            }
        }

        // Build brain-specific handshake.
        // Default to compact counts to avoid oversized datagrams with dense camera channels.
        std::stringstream ss;
        ss << "{\"sensory\":" << b.s_names.size() << ",\"output\":" << b.o_names.size();
        if (handshake_include_names) {
            std::stringstream labels;
            labels << ",\"s_names\":[";
            for (size_t i = 0; i < b.s_names.size(); ++i) {
                labels << "\"" << b.s_names[i] << "\"" << (i + 1 == b.s_names.size() ? "" : ",");
            }
            labels << "],\"o_names\":[";
            for (size_t i = 0; i < b.o_names.size(); ++i) {
                labels << "\"" << b.o_names[i] << "\"" << (i + 1 == b.o_names.size() ? "" : ",");
            }
            labels << "]";
            std::string labels_str = labels.str();
            if ((int)(ss.str().size() + labels_str.size() + 1) <= handshake_max_bytes) {
                ss << labels_str;
            } else {
                std::cerr << "[nao_nn_controller_uds] Brain '" << b.id
                          << "': Omitted handshake label arrays (size would exceed "
                          << handshake_max_bytes << " bytes)." << std::endl;
            }
        }
        ss << "}";
        b.handshake_json = ss.str();

        b.s_buf.assign(b.s_names.size(), 0.0f);
        b.a_buf.assign(b.o_names.size(), 0.5f);
        
        b.cli = std::make_unique<UdsClient>(b.sock_path);
        std::cout << "[nao_nn_controller_uds] Brain '" << b.id << "': S=" << b.s_names.size() << " O=" << b.o_names.size() << " socket=" << b.sock_path << std::endl;
        std::cout << "[nao_nn_controller_uds] Brain '" << b.id
                  << "': owned real sensors=" << b.real_sensor_indices.size()
                  << " real actuators=" << b.real_actuator_indices.size()
                  << " virtual in/out=" << b.virtual_sensors.size() << "/" << b.virtual_actuators.size()
                  << std::endl;
        if (b.real_sensor_indices.empty() || b.real_actuator_indices.empty()) {
            std::cerr << "[nao_nn_controller_uds] Brain '" << b.id
                      << "': WARNING zero mapped real "
                      << (b.real_sensor_indices.empty() ? "sensors" : "actuators")
                      << " (check NM_SENSORS_/NM_ACTUATORS_ regex)." << std::endl;
        }
    }

    // 4. Interconnect storage
    // Map LinkKey -> vector of floats
    struct LinkKey { 
        std::string src, dst; 
        bool operator<(const LinkKey& o) const { return src < o.src || (src == o.src && dst < o.dst); }
    };
    std::map<LinkKey, std::vector<float>> interconnect_bus;

    // 5. Main Loop
    std::vector<float> all_s(all_s_names.size(), 0.0f);
    std::vector<float> all_a(all_o_names.size(), 0.5f);

    while (robot.step(dt) != -1) {
        double now = robot.getTime();

        // 0) Handle keyboard
        if (kb_mapper.is_enabled()) {
            int key = robot.getKeyboard()->getKey();
            if (key > 0) {
                KeyMapping km = kb_mapper.get_mapping();
                if (key == km.up) start_motion(forwards);
                else if (key == km.down) start_motion(backwards);
                else if (key == km.left) start_motion(side_step_left);
                else if (key == km.right) start_motion(side_step_right);
                else if (key == km.turn_left) start_motion(turn_left_60);
                else if (key == km.turn_right) start_motion(turn_right_60);
                else if (key == km.tai_chi) start_motion(tai_chi);
                else if (key == km.wipe) start_motion(wipe_forehead);
                else if (key == km.wave) start_motion(hand_wave);
            }
        }

        // 1) Read physical sensors
        mapper.fill_sensors(all_s);

        // 2) Process each Brain
        for (auto& b : brains) {
            // handshake
            if (!b.connected && (now - b.last_handshake_time >= 5.0)) {
                if (b.cli->send_raw(b.handshake_json)) {
                    std::cout << "[nao_nn_controller_uds] Brain '" << b.id << "': Sent handshake JSON" << std::endl;
                    b.last_handshake_time = now;
                } else {
                    if (errno == ECONNREFUSED) {
                        std::cerr << "[nao_nn_controller_uds] Brain '" << b.id << "': Handshake failed (Connection refused)." << std::endl;
                    } else {
                        std::cerr << "[nao_nn_controller_uds] Brain '" << b.id << "': Handshake failed (errno=" << errno << ")." << std::endl;
                    }
                    b.last_handshake_time = now;
                }
            }

            // Fill sensory buffer for this brain
            // Part A: Real sensors
            for (size_t i=0; i<b.real_sensor_indices.size(); ++i) {
                b.s_buf[i] = all_s[b.real_sensor_indices[i]];
            }
            // Part B: Virtual sensors (Interconnects)
            for (const auto& link : b.virtual_sensors) {
                std::vector<float>& data = interconnect_bus[{link.peer_id, b.id}];
                if (data.size() == (size_t)link.count) {
                    for (int i=0; i<link.count; ++i) {
                        b.s_buf[link.local_start + i] = data[i];
                    }
                }
            }

            // Request frame: [t_ms] + [S floats]
            std::vector<float> req;
            req.reserve(1 + b.s_buf.size());
            req.push_back(ipc_dt_ms);
            req.insert(req.end(), b.s_buf.begin(), b.s_buf.end());

            if (b.cli->xfer(req, b.a_buf, now)) {
                if (!b.connected) {
                    std::cout << "[nao_nn_controller_uds] Brain '" << b.id << "': Connected." << std::endl;
                }
                b.connected = true;

                // Process outputs
                // Part A: Real actuators (Update global actuator buffer)
                for (size_t i=0; i<b.real_actuator_indices.size(); ++i) {
                    all_a[b.real_actuator_indices[i]] = b.a_buf[i];
                }
                // Part B: Virtual actuators (Update interconnect bus)
                for (const auto& link : b.virtual_actuators) {
                    std::vector<float>& data = interconnect_bus[{b.id, link.peer_id}];
                    data.resize(link.count);
                    for (int i=0; i<link.count; ++i) {
                        data[i] = b.a_buf[link.local_start + i];
                    }
                }
            } else {
                bool should_log = (now - b.last_error_time >= 5.0);
                if (errno == EAGAIN || errno == EWOULDBLOCK) {
                    // Treat receive timeout as transient; keep last actuator outputs and
                    // maintain the connected state to avoid flapping under load.
                    if (should_log) {
                        size_t stale_total = b.cli->stale_updates_dropped();
                        size_t stale_delta = stale_total >= b.stale_dropped_last_log
                                               ? (stale_total - b.stale_dropped_last_log)
                                               : stale_total;
                        std::cerr << "[nao_nn_controller_uds] Brain '" << b.id
                                  << "': Transfer timeout (errno=" << errno
                                  << ", win=" << b.cli->window_size()
                                  << ", in_flight=" << b.cli->inflight_updates()
                                  << ", queued=" << b.cli->pending_updates()
                                  << ", stale_dropped+=" << stale_delta
                                  << ")" << std::endl;
                        b.stale_dropped_last_log = stale_total;
                        b.last_error_time = now;
                    }
                    continue;
                }
                if (b.connected) {
                    std::cerr << "[nao_nn_controller_uds] Brain '" << b.id << "': Connection lost." << std::endl;
                    should_log = true;
                }
                if (should_log) {
                    std::cerr << "[nao_nn_controller_uds] Brain '" << b.id << "': Transfer failed (errno=" << errno << ")" << std::endl;
                    b.last_error_time = now;
                }
                b.connected = false;
            }
        }

        // 3) Apply pooled actuators
        bool motion_active = false;
        if (currently_playing) {
            if (wbu_motion_is_over(currently_playing)) currently_playing = NULL;
            else motion_active = true;
        }

        if (!motion_active) {
            celegans_bridge.apply(all_a);
            mapper.apply_actuators(all_a);
        }
    }
    return 0;
}
