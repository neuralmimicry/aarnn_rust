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
#include <array>
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
#include <thread>

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

static int ipc_timeout_grace_ms() {
  constexpr int kDefaultMs = 1500;
  constexpr int kMinMs = 0;
  constexpr int kMaxMs = 60000;
  const char* raw = std::getenv("NM_IPC_TIMEOUT_GRACE_MS");
  if (!raw || !*raw) return kDefaultMs;
  char* end = nullptr;
  long parsed = std::strtol(raw, &end, 10);
  if (end == raw || parsed < kMinMs || parsed > kMaxMs) return kDefaultMs;
  return static_cast<int>(parsed);
}

static int ipc_timeout_log_interval_ms() {
  constexpr int kDefaultMs = 5000;
  constexpr int kMinMs = 250;
  constexpr int kMaxMs = 60000;
  const char* raw = std::getenv("NM_IPC_TIMEOUT_LOG_INTERVAL_MS");
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

static double monotonic_now_seconds() {
  using clock = std::chrono::steady_clock;
  return std::chrono::duration<double>(clock::now().time_since_epoch()).count();
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
    uds_buf_bytes_ = env_int_range("NM_IPC_UDS_CTRL_BUF_BYTES", 262144, 16384, 1 << 24);
    if (::setsockopt(sock_, SOL_SOCKET, SO_RCVBUF, &uds_buf_bytes_, sizeof(uds_buf_bytes_)) < 0) {
      perror("[nao_nn_controller_uds] setsockopt(SO_RCVBUF)");
    }
    if (::setsockopt(sock_, SOL_SOCKET, SO_SNDBUF, &uds_buf_bytes_, sizeof(uds_buf_bytes_)) < 0) {
      perror("[nao_nn_controller_uds] setsockopt(SO_SNDBUF)");
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

    const bool ipc_lockstep_enabled = env_bool("NM_IPC_LOCKSTEP", true);
    const bool ipc_strict_lockstep = env_bool("NM_IPC_STRICT_LOCKSTEP", true);
    strict_lockstep_ = ipc_lockstep_enabled && ipc_strict_lockstep;
    min_window_ = env_int_range("NM_IPC_WINDOW_MIN", 1, 1, 128);
    max_window_ = env_int_range("NM_IPC_WINDOW_MAX", 8, min_window_, 256);
    int init_window = env_int_range("NM_IPC_WINDOW_INIT", 1, min_window_, max_window_);
    max_send_per_pump_ = env_int_range("NM_IPC_SEND_BUDGET_MAX", 4, 1, 128);
    if (strict_lockstep_) {
      min_window_ = 1;
      max_window_ = 1;
      init_window = 1;
      max_send_per_pump_ = 1;
    }
    cwnd_ = static_cast<double>(init_window);
    max_queue_frames_ = env_int_range("NM_IPC_QUEUE_MAX_FRAMES", 256, 1, 16384);
    force_aer_ = env_bool("NM_IPC_FORCE_AER", false);
    disable_aer_ = env_bool("NM_IPC_DISABLE_AER", false);
    max_raw_payload_bytes_ = env_int_range("NM_IPC_MAX_RAW_BYTES", 60000, 256, 1 << 20);
    aer_event_threshold_ = env_float_range("NM_IPC_AER_THRESHOLD", 0.5f, 0.0f, 1.0f);
    aer_max_events_ = env_int_range("NM_IPC_AER_MAX_EVENTS", 20000, 1, 1 << 20);
    aer_max_packet_bytes_ = env_int_range("NM_IPC_AER_MAX_PACKET_BYTES", 60000, 256, 1 << 20);
    aer_runtime_packet_bytes_ = aer_max_packet_bytes_;
    aer_sensory_base_ = (uint32_t)std::max(0, env_int_range("NM_AER_S_BASE", 0, 0, 1 << 30));
    aer_output_base_ = (uint32_t)std::max(0, env_int_range("NM_AER_O_BASE", 16384, 0, 1 << 30));

    std::cout << "[nao_nn_controller_uds] IPC flow window init/min/max="
              << init_window << "/" << min_window_ << "/" << max_window_
              << " send_budget_max=" << max_send_per_pump_
              << " queue_max=" << max_queue_frames_
              << " mode=nonblocking_async"
              << " strict_lockstep=" << (strict_lockstep_ ? 1 : 0)
              << " uds_buf=" << uds_buf_bytes_
              << " aer(force=" << (force_aer_ ? 1 : 0)
              << ",disable=" << (disable_aer_ ? 1 : 0)
              << ",thr=" << aer_event_threshold_
              << ",max_events=" << aer_max_events_
              << ",max_pkt=" << aer_max_packet_bytes_
              << ",s_base=" << aer_sensory_base_
              << ",o_base=" << aer_output_base_
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
  bool xfer(
    const std::vector<float>& req,
    std::vector<float>& rep,
    double now_s,
    bool enqueue_request = true
  ){
    if (enqueue_request) {
      enqueue_or_replace(req, now_s);
    }
    if (!strict_lockstep_) {
      expire_inflight(now_s);
    }

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
      const char* send_data = nullptr;
      size_t nbytes = 0;
      if (use_aer) {
        const std::vector<uint8_t>& aer_payload =
          encode_aer_payload(frame.payload, aer_runtime_packet_bytes_);
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

      tx_frames_total_ += 1;
      if (use_aer) {
        tx_aer_frames_ += 1;
        tx_aer_events_ += last_aer_encoded_events_;
      } else {
        tx_raw_frames_ += 1;
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

  uint64_t tx_frames_total() const { return tx_frames_total_; }
  uint64_t tx_aer_frames() const { return tx_aer_frames_; }
  uint64_t tx_raw_frames() const { return tx_raw_frames_; }
  uint64_t tx_aer_events() const { return tx_aer_events_; }
  uint64_t rx_frames_total() const { return rx_frames_total_; }
  uint64_t rx_aer_frames() const { return rx_aer_frames_; }
  uint64_t rx_raw_frames() const { return rx_raw_frames_; }
  uint64_t rx_aer_events() const { return rx_aer_events_; }

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

  static bool read_varint(const char* data, size_t len, size_t& idx, uint64_t& value) {
    value = 0;
    uint32_t shift = 0;
    while (idx < len) {
      const uint8_t byte = static_cast<uint8_t>(data[idx++]);
      value |= static_cast<uint64_t>(byte & 0x7fu) << shift;
      if ((byte & 0x80u) == 0) {
        return true;
      }
      shift += 7;
      if (shift >= 64) {
        return false;
      }
    }
    return false;
  }

  bool decode_aer_reply(const char* data, size_t len, std::vector<float>& rep_out, uint64_t& decoded_events) const {
    decoded_events = 0;
    if (len < 12 || std::memcmp(data, "AER1", 4) != 0) {
      return false;
    }

    std::fill(rep_out.begin(), rep_out.end(), 0.5f);
    size_t idx = 12;  // magic + base timestamp; output commands are carried by events.
    while (idx < len) {
      uint64_t delta_ts = 0;
      uint64_t addr = 0;
      uint64_t value = 0;
      if (!read_varint(data, len, idx, delta_ts)
          || !read_varint(data, len, idx, addr)
          || !read_varint(data, len, idx, value)) {
        return false;
      }
      (void)delta_ts;
      size_t output_idx = 0;
      if (addr >= aer_output_base_) {
        output_idx = static_cast<size_t>(addr - aer_output_base_);
      } else {
        output_idx = static_cast<size_t>(addr);
      }
      if (output_idx < rep_out.size()) {
        rep_out[output_idx] = (value & 0xffu) != 0 ? 1.0f : 0.0f;
        decoded_events += 1;
      }
    }
    return true;
  }

  bool should_use_aer(const std::vector<float>& payload) const {
    if (disable_aer_) return false;
    if (force_aer_) return true;
    const size_t raw_bytes = payload.size() * sizeof(float);
    return raw_bytes > (size_t)max_raw_payload_bytes_;
  }

  const std::vector<uint8_t>& encode_aer_payload(
    const std::vector<float>& payload,
    int packet_budget_bytes
  ) {
    // AER wire format: "AER1" + base_ts_us(u64 LE) + varint(delta_ts, addr, value)...
    // aer_out_buf_ is a member reused across calls to avoid per-send heap allocation.
    std::vector<uint8_t>& out = aer_out_buf_;
    out.clear();
    out.reserve(64);
    last_aer_encoded_events_ = 0;
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
      last_aer_encoded_events_ = 0;
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
      if (static_cast<int>(out.size()) > packet_budget_bytes) {
        out.resize(old_size);
        return true;
      }
      emitted += 1;
      if (emitted >= aer_max_events_) return true;
      return false;
    };

    // payload[0] is dt_ms, payload[1..] are sensory values.
    for (size_t off = 0; off < sensory_count; ++off) {
      size_t idx = start + off;
      if (idx >= sensory_count) idx -= sensory_count;
      if (emit_index(idx)) {
        last_aer_encoded_events_ = static_cast<uint64_t>(emitted);
        return out;
      }
    }
    last_aer_encoded_events_ = static_cast<uint64_t>(emitted);
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
    // Resize recv_buf_ once; avoids per-call heap allocation on the hot path.
    const size_t expected_bytes = rep_out.size() * sizeof(float);
    const size_t max_reply_bytes = std::max(expected_bytes, static_cast<size_t>(aer_max_packet_bytes_));
    if (recv_buf_.size() != max_reply_bytes) {
      recv_buf_.resize(max_reply_bytes);
    }
    std::vector<char>& buf = recv_buf_;
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
      if ((size_t)r == expected_bytes) {
        std::memcpy(rep_out.data(), buf.data(), expected_bytes);
        have_reply = true;
        rx_frames_total_ += 1;
        rx_raw_frames_ += 1;
        on_reply_acked(now_s);
        continue;
      }
      if (r >= 4 && std::memcmp(buf.data(), "AER1", 4) == 0) {
        uint64_t decoded_events = 0;
        if (decode_aer_reply(buf.data(), static_cast<size_t>(r), rep_out, decoded_events)) {
          have_reply = true;
          rx_frames_total_ += 1;
          rx_aer_frames_ += 1;
          rx_aer_events_ += decoded_events;
          on_reply_acked(now_s);
        }
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
  bool strict_lockstep_ = false;
  int max_raw_payload_bytes_ = 60000;
  float aer_event_threshold_ = 0.5f;
  int aer_max_events_ = 20000;
  int aer_max_packet_bytes_ = 60000;
  int aer_runtime_packet_bytes_ = 60000;
  int uds_buf_bytes_ = 262144;
  size_t aer_emit_phase_ = 0;
  uint32_t aer_sensory_base_ = 0;
  uint32_t aer_output_base_ = 16384;
  double cwnd_ = 2.0;
  double srtt_ms_ = 0.0;
  double rttvar_ms_ = 0.0;
  uint64_t next_seq_ = 1;
  size_t stale_updates_dropped_ = 0;
  uint64_t tx_frames_total_ = 0;
  uint64_t tx_aer_frames_ = 0;
  uint64_t tx_raw_frames_ = 0;
  uint64_t tx_aer_events_ = 0;
  uint64_t rx_frames_total_ = 0;
  uint64_t rx_aer_frames_ = 0;
  uint64_t rx_raw_frames_ = 0;
  uint64_t rx_aer_events_ = 0;
  uint64_t last_aer_encoded_events_ = 0;
  std::deque<PendingFrame> tx_queue_;
  std::deque<InflightFrame> inflight_;
  std::string server_path_;
  struct sockaddr_un server_{};
  std::vector<char> recv_buf_;       // reused across drain_replies() calls
  std::vector<uint8_t> aer_out_buf_; // reused across encode_aer_payload() calls
};

struct BrainInstance {
    std::string id;
    std::string sock_path;
    std::unique_ptr<UdsClient> cli;
    std::string handshake_json;
    bool connected = false;
    double last_handshake_time = -10.0;
    double last_error_time = -10.0;
    double last_success_time = -10.0;
    size_t stale_dropped_last_log = 0;
    double last_diag_time = -10.0;
    uint64_t diag_tx_frames_last = 0;
    uint64_t diag_rx_frames_last = 0;
    uint64_t diag_aer_frames_last = 0;
    uint64_t diag_raw_frames_last = 0;
    uint64_t diag_aer_events_last = 0;
    uint64_t diag_rx_aer_frames_last = 0;
    uint64_t diag_rx_raw_frames_last = 0;
    uint64_t diag_rx_aer_events_last = 0;

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
    std::vector<float> req_buf;  // reused request frame: [dt_ms, s0, s1, ...]
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
    bool twitch_fallback = true;
    int flat_steps = 0;
    int flat_steps_trigger = 4;
    int twitch_hold_steps = 24;
    int twitch_hold_remaining = 0;
    float flat_drive_eps = 0.08f;
    float twitch_amp = 0.42f;
    float twitch_phase = 0.0f;
    float twitch_phase_step = 0.56f;
    float twitch_segment_lag = 0.42f;
    size_t apply_counter = 0;
    size_t debug_interval = 0;

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
        twitch_fallback = env_bool("NM_CELEGANS_TWITCH_FALLBACK", true);
        flat_steps_trigger = env_int_range("NM_CELEGANS_TWITCH_FLAT_STEPS", 4, 1, 100000);
        twitch_hold_steps = env_int_range("NM_CELEGANS_TWITCH_HOLD_STEPS", 24, 1, 100000);
        flat_drive_eps = env_float_range("NM_CELEGANS_TWITCH_DRIVE_EPS", 0.08f, 0.0001f, 0.5f);
        twitch_amp = env_float_range("NM_CELEGANS_TWITCH_AMP", 0.42f, 0.01f, 1.5f);
        twitch_phase_step = env_float_range("NM_CELEGANS_TWITCH_PHASE_STEP", 0.56f, 0.01f, 6.0f);
        twitch_segment_lag = env_float_range("NM_CELEGANS_TWITCH_SEG_LAG", 0.42f, 0.0f, 6.0f);
        debug_interval =
          static_cast<size_t>(env_int_range("NM_CELEGANS_BRIDGE_DEBUG_INTERVAL", 0, 0, 100000));
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
                      << ", muscle channels=" << matched_muscle
                      << ", twitch_fallback=" << (twitch_fallback ? 1 : 0)
                      << ", flat_steps=" << flat_steps_trigger
                      << ", hold_steps=" << twitch_hold_steps
                      << ", flat_eps=" << flat_drive_eps
                      << ", twitch_amp=" << twitch_amp
                      << ")"
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
        std::array<int, 24> spine_for_seg{};
        std::array<float, 24> drive_for_seg{};
        std::array<float, 24> target_for_seg{};
        spine_for_seg.fill(-1);
        drive_for_seg.fill(0.0f);
        target_for_seg.fill(0.5f);
        int active_segments = 0;
        float abs_drive_sum = 0.0f;
        float abs_drive_max = 0.0f;
        float mean_muscle_offset_sum = 0.0f;

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
            float mean_muscle = 0.25f * (mdl + mdr + mvl + mvr);
            mean_muscle_offset_sum += std::fabs(mean_muscle - 0.5f);

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
            spine_for_seg[seg] = spine_idx;
            drive_for_seg[seg] = drive;
            target_for_seg[seg] = target;
            const float abs_drive = std::fabs(drive);
            abs_drive_sum += abs_drive;
            abs_drive_max = std::max(abs_drive_max, abs_drive);
            active_segments += 1;
        }

        if (active_segments <= 0) return;

        const float abs_drive_mean = abs_drive_sum / std::max(1, active_segments);
        const bool low_motion_drive =
          abs_drive_max <= flat_drive_eps || abs_drive_mean <= (0.35f * flat_drive_eps);
        if (low_motion_drive) {
            flat_steps = std::min(flat_steps + 1, 1000000);
        } else {
            flat_steps = std::max(0, flat_steps - 2);
        }
        if (twitch_fallback && flat_steps >= flat_steps_trigger) {
            twitch_hold_remaining = std::max(twitch_hold_remaining, twitch_hold_steps);
        } else if (!low_motion_drive && abs_drive_max >= (1.8f * flat_drive_eps)) {
            // Disable fallback immediately once strong endogenous drive is present.
            twitch_hold_remaining = 0;
        }

        bool injected_twitch = false;
        float injected_amp = 0.0f;
        if (twitch_fallback && twitch_hold_remaining > 0) {
            injected_twitch = true;
            twitch_hold_remaining = std::max(0, twitch_hold_remaining - 1);
            const float mean_muscle_offset =
              mean_muscle_offset_sum / std::max(1, active_segments);
            const float activity_hint =
              std::max(0.0f, std::min(1.0f, mean_muscle_offset * 2.0f));
            injected_amp = twitch_amp * (0.55f + 0.45f * activity_hint);
            constexpr float kTau = 6.28318530718f;
            twitch_phase += twitch_phase_step;
            if (twitch_phase > kTau) {
                twitch_phase = std::fmod(twitch_phase, kTau);
            }

            for (int seg = 0; seg < 24; ++seg) {
                if (spine_for_seg[seg] < 0) continue;
                const float edge_taper = (seg < 5 || seg > 20) ? 0.72f : 1.0f;
                const float wave = std::sin(
                  twitch_phase + twitch_segment_lag * static_cast<float>(seg)
                );
                float driven = drive_for_seg[seg] + injected_amp * edge_taper * wave;
                float target = 0.5f + 0.44f * driven;
                if (target < 0.05f) target = 0.05f;
                if (target > 0.95f) target = 0.95f;
                target_for_seg[seg] = target;
            }
        }

        float target_min = 1.0f;
        float target_max = 0.0f;
        float spine_min = 1.0f;
        float spine_max = 0.0f;
        for (int seg = 0; seg < 24; ++seg) {
            int spine_idx = spine_for_seg[seg];
            if (spine_idx < 0 || spine_idx >= (int)all_a.size()) continue;
            float target = target_for_seg[seg];
            target_min = std::min(target_min, target);
            target_max = std::max(target_max, target);
            const float alpha = injected_twitch ? 0.34f : 0.22f;
            smoothed_targets[seg] = (1.0f - alpha) * smoothed_targets[seg] + alpha * target;
            all_a[spine_idx] = smoothed_targets[seg];
            spine_min = std::min(spine_min, smoothed_targets[seg]);
            spine_max = std::max(spine_max, smoothed_targets[seg]);
        }

        apply_counter += 1;
        if (debug_interval > 0 && apply_counter % debug_interval == 0) {
            std::cout << "[nao_nn_controller_uds] Celegans bridge diag"
                      << " segs=" << active_segments
                      << " drive_abs[mean,max]=["
                      << abs_drive_mean
                      << "," << abs_drive_max << "]"
                      << " target[min,max]=[" << target_min << "," << target_max << "]"
                      << " spine[min,max]=[" << spine_min << "," << spine_max << "]"
                      << " flat_steps=" << flat_steps
                      << " hold=" << twitch_hold_remaining
                      << " twitch=" << (injected_twitch ? 1 : 0)
                      << " amp=" << injected_amp
                      << std::endl;
        }
    }
};

struct HexapodLegBridge {
    // [leg][joint], joints order: coxa, femur, tibia
    std::array<std::array<int, 3>, 6> joint_indices{};
    std::vector<float> filtered;
    bool active = false;
    bool twitch_fallback = true;
    int flat_steps = 0;
    int flat_steps_trigger = 5;
    int twitch_hold_steps = 22;
    int twitch_hold_remaining = 0;
    float flat_drive_eps = 0.05f;
    float twitch_amp = 0.20f;
    float twitch_phase = 0.0f;
    float twitch_phase_step = 0.48f;
    size_t apply_counter = 0;
    size_t debug_interval = 0;

    HexapodLegBridge() {
        for (auto& leg : joint_indices) {
            leg = { -1, -1, -1 };
        }
        twitch_fallback = env_bool("NM_HEXAPOD_TWITCH_FALLBACK", true);
        flat_steps_trigger = env_int_range("NM_HEXAPOD_TWITCH_FLAT_STEPS", 5, 1, 100000);
        twitch_hold_steps = env_int_range("NM_HEXAPOD_TWITCH_HOLD_STEPS", 22, 1, 100000);
        flat_drive_eps = env_float_range("NM_HEXAPOD_TWITCH_DRIVE_EPS", 0.05f, 0.0001f, 0.4f);
        twitch_amp = env_float_range("NM_HEXAPOD_TWITCH_AMP", 0.20f, 0.01f, 1.0f);
        twitch_phase_step = env_float_range("NM_HEXAPOD_TWITCH_PHASE_STEP", 0.48f, 0.01f, 6.0f);
        debug_interval =
          static_cast<size_t>(env_int_range("NM_HEXAPOD_BRIDGE_DEBUG_INTERVAL", 0, 0, 100000));
    }

    static int leg_index(const std::string& leg) {
        if (leg == "lf") return 0;
        if (leg == "lm") return 1;
        if (leg == "lr") return 2;
        if (leg == "rf") return 3;
        if (leg == "rm") return 4;
        if (leg == "rr") return 5;
        return -1;
    }

    static int joint_index(const std::string& joint) {
        if (joint == "coxa") return 0;
        if (joint == "femur") return 1;
        if (joint == "tibia") return 2;
        return -1;
    }

    static float leg_phase_offset(int li) {
        static const float kOffsets[6] = {
          0.0f,
          2.094395102f,
          4.188790205f,
          3.141592654f,
          5.235987756f,
          1.047197551f
        };
        if (li < 0) li = 0;
        if (li > 5) li = 5;
        return kOffsets[li];
    }

    void discover(const std::vector<std::string>& actuator_names) {
        std::regex leg_re("^hex_o_([0-9]{3})_([A-Za-z]{2})_(coxa|femur|tibia)$");
        std::smatch m;
        int matched = 0;
        for (size_t i = 0; i < actuator_names.size(); ++i) {
            if (!std::regex_match(actuator_names[i], m, leg_re)) continue;
            std::string leg = m[2];
            std::string joint = m[3];
            std::transform(leg.begin(), leg.end(), leg.begin(), [](unsigned char c) {
                return static_cast<char>(std::tolower(c));
            });
            std::transform(joint.begin(), joint.end(), joint.begin(), [](unsigned char c) {
                return static_cast<char>(std::tolower(c));
            });
            int li = leg_index(leg);
            int ji = joint_index(joint);
            if (li < 0 || ji < 0) continue;
            joint_indices[(size_t)li][(size_t)ji] = (int)i;
            matched++;
        }

        active = true;
        for (const auto& leg : joint_indices) {
            for (int idx : leg) {
                if (idx < 0) {
                    active = false;
                    break;
                }
            }
            if (!active) break;
        }

        filtered.assign(actuator_names.size(), 0.5f);

        if (active) {
            std::cout << "[nao_nn_controller_uds] Hexapod leg bridge active"
                      << " (matched joints=" << matched
                      << ", twitch_fallback=" << (twitch_fallback ? 1 : 0)
                      << ", flat_steps=" << flat_steps_trigger
                      << ", hold_steps=" << twitch_hold_steps
                      << ", flat_eps=" << flat_drive_eps
                      << ", twitch_amp=" << twitch_amp
                      << ")"
                      << std::endl;
        }
    }

    static inline float read_cmd(const std::vector<float>& all_a, int idx) {
        if (idx < 0 || idx >= (int)all_a.size()) return 0.5f;
        float v = all_a[(size_t)idx];
        if (!std::isfinite(v)) return 0.5f;
        if (v < 0.0f) return 0.0f;
        if (v > 1.0f) return 1.0f;
        return v;
    }

    static inline float clamp01(float v) {
        if (v < 0.0f) return 0.0f;
        if (v > 1.0f) return 1.0f;
        return v;
    }

    static inline float joint_base_gain(int joint) {
        return (joint == 0) ? 0.90f : 0.72f;
    }

    static inline void joint_clamp(int joint, float& target) {
        if (joint == 0) {
            if (target < 0.06f) target = 0.06f;
            if (target > 0.94f) target = 0.94f;
        } else {
            if (target < 0.10f) target = 0.10f;
            if (target > 0.90f) target = 0.90f;
        }
    }

    static inline float joint_wave_scale(int joint) {
        if (joint == 0) return 1.0f;
        if (joint == 1) return 0.82f;
        return 1.12f;
    }

    void apply(std::vector<float>& all_a) {
        if (!active) return;
        if (filtered.size() < all_a.size()) filtered.resize(all_a.size(), 0.5f);

        std::array<std::array<float, 3>, 6> target_for_joint{};
        float abs_drive_sum = 0.0f;
        float abs_drive_max = 0.0f;
        int active_joints = 0;
        for (int li = 0; li < 6; ++li) {
            for (int j = 0; j < 3; ++j) {
                int idx = joint_indices[(size_t)li][(size_t)j];
                if (idx < 0 || idx >= (int)all_a.size()) continue;
                float cmd = read_cmd(all_a, idx);
                float centered = cmd - 0.5f;
                const float abs_drive = std::fabs(centered);
                abs_drive_sum += abs_drive;
                abs_drive_max = std::max(abs_drive_max, abs_drive);
                active_joints += 1;

                float target = 0.5f + centered * joint_base_gain(j);
                joint_clamp(j, target);
                target_for_joint[(size_t)li][(size_t)j] = target;
            }
        }
        if (active_joints <= 0) return;

        const float abs_drive_mean = abs_drive_sum / std::max(1, active_joints);
        const bool low_motion_drive =
          abs_drive_max <= flat_drive_eps || abs_drive_mean <= (0.5f * flat_drive_eps);
        if (low_motion_drive) {
            flat_steps = std::min(flat_steps + 1, 1000000);
        } else {
            flat_steps = std::max(0, flat_steps - 2);
        }
        if (twitch_fallback && flat_steps >= flat_steps_trigger) {
            twitch_hold_remaining = std::max(twitch_hold_remaining, twitch_hold_steps);
        } else if (!low_motion_drive && abs_drive_max >= (1.8f * flat_drive_eps)) {
            twitch_hold_remaining = 0;
        }

        bool injected_twitch = false;
        float injected_amp = 0.0f;
        if (twitch_fallback && twitch_hold_remaining > 0) {
            injected_twitch = true;
            twitch_hold_remaining = std::max(0, twitch_hold_remaining - 1);
            const float activity_hint =
              std::max(0.0f, std::min(1.0f, abs_drive_mean / 0.25f));
            injected_amp = twitch_amp * (0.6f + 0.4f * activity_hint);
            constexpr float kTau = 6.28318530718f;
            twitch_phase += twitch_phase_step;
            if (twitch_phase > kTau) {
                twitch_phase = std::fmod(twitch_phase, kTau);
            }

            for (int li = 0; li < 6; ++li) {
                const float leg_phase = twitch_phase + leg_phase_offset(li);
                for (int j = 0; j < 3; ++j) {
                    const int idx = joint_indices[(size_t)li][(size_t)j];
                    if (idx < 0 || idx >= (int)all_a.size()) continue;
                    const float wave =
                      std::sin(leg_phase + static_cast<float>(j) * 1.1f);
                    float target = target_for_joint[(size_t)li][(size_t)j] +
                                   injected_amp * joint_wave_scale(j) * wave;
                    joint_clamp(j, target);
                    target_for_joint[(size_t)li][(size_t)j] = target;
                }
            }
        }

        float leg_min = 1.0f;
        float leg_max = 0.0f;
        for (int li = 0; li < 6; ++li) {
            for (int j = 0; j < 3; ++j) {
                int idx = joint_indices[(size_t)li][(size_t)j];
                if (idx < 0 || idx >= (int)all_a.size()) continue;
                const float alpha = injected_twitch ? ((j == 0) ? 0.30f : 0.26f) :
                                                     ((j == 0) ? 0.20f : 0.16f);
                filtered[(size_t)idx] =
                  (1.0f - alpha) * filtered[(size_t)idx] +
                  alpha * target_for_joint[(size_t)li][(size_t)j];
                all_a[(size_t)idx] = clamp01(filtered[(size_t)idx]);
                leg_min = std::min(leg_min, all_a[(size_t)idx]);
                leg_max = std::max(leg_max, all_a[(size_t)idx]);
            }
        }

        apply_counter += 1;
        if (debug_interval > 0 && apply_counter % debug_interval == 0) {
            std::cout << "[nao_nn_controller_uds] Hexapod bridge diag"
                      << " drive_abs[mean,max]=[" << abs_drive_mean << "," << abs_drive_max << "]"
                      << " out[min,max]=[" << leg_min << "," << leg_max << "]"
                      << " flat_steps=" << flat_steps
                      << " hold=" << twitch_hold_remaining
                      << " twitch=" << (injected_twitch ? 1 : 0)
                      << " amp=" << injected_amp
                      << std::endl;
        }
    }
};

struct DrosophilaMotionBridge {
    // [leg][joint], leg order: LF, LM, LR, RF, RM, RR.
    // Joint order: coxa, femur, tibia, tarsus.
    std::array<std::array<int, 4>, 6> leg_indices{};
    std::vector<int> dros_output_indices;
    int wing_left_index = -1;
    int wing_right_index = -1;
    std::vector<float> filtered;
    bool active = false;
    bool twitch_fallback = true;
    int flat_steps = 0;
    int flat_steps_trigger = 4;
    int twitch_hold_steps = 26;
    int twitch_hold_remaining = 0;
    float flat_drive_eps = 0.06f;
    float wing_flutter_amp = 0.32f;
    float leg_twitch_amp = 0.19f;
    float phase = 0.0f;
    float phase_step = 0.78f;
    size_t apply_counter = 0;
    size_t debug_interval = 0;

    DrosophilaMotionBridge() {
        for (auto& leg : leg_indices) {
            leg = { -1, -1, -1, -1 };
        }
        twitch_fallback = env_bool("NM_DROS_TWITCH_FALLBACK", true);
        flat_steps_trigger = env_int_range("NM_DROS_TWITCH_FLAT_STEPS", 4, 1, 100000);
        twitch_hold_steps = env_int_range("NM_DROS_TWITCH_HOLD_STEPS", 26, 1, 100000);
        flat_drive_eps = env_float_range("NM_DROS_TWITCH_DRIVE_EPS", 0.06f, 0.0001f, 0.5f);
        wing_flutter_amp = env_float_range("NM_DROS_WING_TWITCH_AMP", 0.32f, 0.01f, 1.2f);
        leg_twitch_amp = env_float_range("NM_DROS_LEG_TWITCH_AMP", 0.19f, 0.01f, 1.2f);
        phase_step = env_float_range("NM_DROS_TWITCH_PHASE_STEP", 0.78f, 0.01f, 6.0f);
        debug_interval =
          static_cast<size_t>(env_int_range("NM_DROS_BRIDGE_DEBUG_INTERVAL", 0, 0, 100000));
    }

    static int leg_index(const std::string& side, const std::string& position) {
        if (side == "left") {
            if (position == "front") return 0;
            if (position == "mid") return 1;
            if (position == "rear") return 2;
        } else if (side == "right") {
            if (position == "front") return 3;
            if (position == "mid") return 4;
            if (position == "rear") return 5;
        }
        return -1;
    }

    static int joint_index(const std::string& joint) {
        if (joint == "coxa") return 0;
        if (joint == "femur") return 1;
        if (joint == "tibia") return 2;
        if (joint == "tarsus") return 3;
        return -1;
    }

    static float leg_phase_offset(int li) {
        static const float kOffsets[6] = {
          0.0f,
          2.094395102f,
          4.188790205f,
          3.141592654f,
          5.235987756f,
          1.047197551f
        };
        if (li < 0) li = 0;
        if (li > 5) li = 5;
        return kOffsets[li];
    }

    static inline float clamp01(float v) {
        if (v < 0.0f) return 0.0f;
        if (v > 1.0f) return 1.0f;
        return v;
    }

    static inline float read_cmd(const std::vector<float>& all_a, int idx) {
        if (idx < 0 || idx >= (int)all_a.size()) return 0.5f;
        float v = all_a[(size_t)idx];
        if (!std::isfinite(v)) return 0.5f;
        if (v < 0.0f) return 0.0f;
        if (v > 1.0f) return 1.0f;
        return v;
    }

    static float joint_base_gain(int joint) {
        if (joint == 0) return 0.52f;
        if (joint == 1) return 0.45f;
        if (joint == 2) return 0.48f;
        return 0.56f;
    }

    static float joint_wave_scale(int joint) {
        if (joint == 0) return 0.85f;
        if (joint == 1) return 0.75f;
        if (joint == 2) return 0.72f;
        return 0.95f;
    }

    static void clamp_leg_joint(int joint, float& target) {
        if (joint == 0) {
            if (target < 0.20f) target = 0.20f;
            if (target > 0.80f) target = 0.80f;
        } else if (joint == 1) {
            if (target < 0.18f) target = 0.18f;
            if (target > 0.82f) target = 0.82f;
        } else if (joint == 2) {
            if (target < 0.12f) target = 0.12f;
            if (target > 0.88f) target = 0.88f;
        } else {
            if (target < 0.10f) target = 0.10f;
            if (target > 0.90f) target = 0.90f;
        }
    }

    float project_pair(const std::vector<float>& all_a, int seed_a, int seed_b) const {
        if (dros_output_indices.empty()) return 0.5f;
        const int n = static_cast<int>(dros_output_indices.size());
        int ia = seed_a % n;
        if (ia < 0) ia += n;
        int ib = seed_b % n;
        if (ib < 0) ib += n;
        const float a = read_cmd(all_a, dros_output_indices[(size_t)ia]);
        const float b = read_cmd(all_a, dros_output_indices[(size_t)ib]);
        const float mixed = 0.62f * a + 0.38f * b;
        const float centered = (mixed - 0.5f) * 0.78f;
        return clamp01(0.5f + centered);
    }

    void discover(const std::vector<std::string>& actuator_names) {
        std::regex dros_out_re("^dros_o_[0-9]{3}_.*$");
        std::regex leg_re("^leg_(left|right)_(front|mid|rear)_(coxa|femur|tibia|tarsus)$");
        std::smatch m;
        int matched_legs = 0;
        for (size_t i = 0; i < actuator_names.size(); ++i) {
            const std::string& name = actuator_names[i];
            if (std::regex_match(name, dros_out_re)) {
                dros_output_indices.push_back((int)i);
                continue;
            }
            if (name == "wing_left_flap") {
                wing_left_index = (int)i;
                continue;
            }
            if (name == "wing_right_flap") {
                wing_right_index = (int)i;
                continue;
            }
            if (std::regex_match(name, m, leg_re)) {
                std::string side = m[1];
                std::string position = m[2];
                std::string joint = m[3];
                int li = leg_index(side, position);
                int ji = joint_index(joint);
                if (li >= 0 && ji >= 0) {
                    leg_indices[(size_t)li][(size_t)ji] = (int)i;
                    matched_legs++;
                }
            }
        }

        int discovered_leg_joints = 0;
        for (const auto& leg : leg_indices) {
            for (int idx : leg) {
                if (idx >= 0) discovered_leg_joints++;
            }
        }

        active = !dros_output_indices.empty() && wing_left_index >= 0 && wing_right_index >= 0 &&
                 discovered_leg_joints >= 12;
        filtered.assign(actuator_names.size(), 0.5f);

        if (active) {
            std::cout << "[nao_nn_controller_uds] Drosophila motion bridge active"
                      << " (dros_o=" << dros_output_indices.size()
                      << ", wings=" << (wing_left_index >= 0 ? 1 : 0) + (wing_right_index >= 0 ? 1 : 0)
                      << ", leg_joints=" << discovered_leg_joints
                      << ", twitch_fallback=" << (twitch_fallback ? 1 : 0)
                      << ", flat_steps=" << flat_steps_trigger
                      << ", hold_steps=" << twitch_hold_steps
                      << ", flat_eps=" << flat_drive_eps
                      << ", wing_twitch_amp=" << wing_flutter_amp
                      << ", leg_twitch_amp=" << leg_twitch_amp
                      << ")"
                      << std::endl;
        }
    }

    void apply(std::vector<float>& all_a) {
        if (!active) return;
        if (filtered.size() < all_a.size()) filtered.resize(all_a.size(), 0.5f);

        float abs_drive_sum = 0.0f;
        float abs_drive_max = 0.0f;
        for (int idx : dros_output_indices) {
            float v = read_cmd(all_a, idx);
            const float abs_drive = std::fabs(v - 0.5f);
            abs_drive_sum += abs_drive;
            abs_drive_max = std::max(abs_drive_max, abs_drive);
        }
        const float abs_drive_mean = abs_drive_sum / std::max<size_t>(1, dros_output_indices.size());
        const bool low_motion_drive =
          abs_drive_max <= flat_drive_eps || abs_drive_mean <= (0.4f * flat_drive_eps);

        if (low_motion_drive) {
            flat_steps = std::min(flat_steps + 1, 1000000);
        } else {
            flat_steps = std::max(0, flat_steps - 2);
        }
        if (twitch_fallback && flat_steps >= flat_steps_trigger) {
            twitch_hold_remaining = std::max(twitch_hold_remaining, twitch_hold_steps);
        } else if (!low_motion_drive && abs_drive_max >= (1.8f * flat_drive_eps)) {
            twitch_hold_remaining = 0;
        }

        const float activity_hint = std::max(0.0f, std::min(1.0f, abs_drive_mean / 0.30f));
        bool injected_twitch = false;
        float injected_leg_amp = 0.0f;
        float wing_amp = 0.09f + 0.24f * activity_hint;
        if (twitch_fallback && twitch_hold_remaining > 0) {
            injected_twitch = true;
            twitch_hold_remaining = std::max(0, twitch_hold_remaining - 1);
            injected_leg_amp = leg_twitch_amp * (0.65f + 0.35f * activity_hint);
            wing_amp = std::max(wing_amp, wing_flutter_amp * (0.7f + 0.3f * activity_hint));
        }

        constexpr float kTau = 6.28318530718f;
        phase += phase_step * (0.85f + 0.65f * activity_hint);
        if (phase > kTau) {
            phase = std::fmod(phase, kTau);
        }

        float wing_min = 1.0f;
        float wing_max = 0.0f;
        if (wing_left_index >= 0 && wing_left_index < (int)all_a.size()) {
            const float wing_nn_l = project_pair(all_a, 3, 17);
            const float flutter_l = 0.5f + wing_amp * std::sin(phase);
            float target_l = 0.58f * wing_nn_l + 0.42f * flutter_l;
            if (target_l < 0.06f) target_l = 0.06f;
            if (target_l > 0.94f) target_l = 0.94f;
            const float alpha = injected_twitch ? 0.40f : 0.24f;
            filtered[(size_t)wing_left_index] =
              (1.0f - alpha) * filtered[(size_t)wing_left_index] + alpha * target_l;
            all_a[(size_t)wing_left_index] = clamp01(filtered[(size_t)wing_left_index]);
            wing_min = std::min(wing_min, all_a[(size_t)wing_left_index]);
            wing_max = std::max(wing_max, all_a[(size_t)wing_left_index]);
        }
        if (wing_right_index >= 0 && wing_right_index < (int)all_a.size()) {
            const float wing_nn_r = project_pair(all_a, 11, 29);
            const float flutter_r = 0.5f - wing_amp * std::sin(phase);
            float target_r = 0.58f * wing_nn_r + 0.42f * flutter_r;
            if (target_r < 0.06f) target_r = 0.06f;
            if (target_r > 0.94f) target_r = 0.94f;
            const float alpha = injected_twitch ? 0.40f : 0.24f;
            filtered[(size_t)wing_right_index] =
              (1.0f - alpha) * filtered[(size_t)wing_right_index] + alpha * target_r;
            all_a[(size_t)wing_right_index] = clamp01(filtered[(size_t)wing_right_index]);
            wing_min = std::min(wing_min, all_a[(size_t)wing_right_index]);
            wing_max = std::max(wing_max, all_a[(size_t)wing_right_index]);
        }

        float leg_min = 1.0f;
        float leg_max = 0.0f;
        for (int li = 0; li < 6; ++li) {
            const float leg_phase = phase + leg_phase_offset(li);
            for (int joint = 0; joint < 4; ++joint) {
                const int idx = leg_indices[(size_t)li][(size_t)joint];
                if (idx < 0 || idx >= (int)all_a.size()) continue;

                const float nn_target = project_pair(
                  all_a,
                  li * 7 + joint * 11 + 5,
                  li * 13 + joint * 3 + 19
                );
                float target = 0.5f + (nn_target - 0.5f) * joint_base_gain(joint);

                if (injected_twitch) {
                    const float wave = std::sin(leg_phase + static_cast<float>(joint) * 0.92f);
                    target += injected_leg_amp * joint_wave_scale(joint) * wave;
                }
                clamp_leg_joint(joint, target);
                const float alpha = injected_twitch ? 0.34f : 0.22f;
                filtered[(size_t)idx] = (1.0f - alpha) * filtered[(size_t)idx] + alpha * target;
                all_a[(size_t)idx] = clamp01(filtered[(size_t)idx]);
                leg_min = std::min(leg_min, all_a[(size_t)idx]);
                leg_max = std::max(leg_max, all_a[(size_t)idx]);
            }
        }

        apply_counter += 1;
        if (debug_interval > 0 && apply_counter % debug_interval == 0) {
            std::cout << "[nao_nn_controller_uds] Dros bridge diag"
                      << " dros_drive_abs[mean,max]=[" << abs_drive_mean << "," << abs_drive_max << "]"
                      << " wing[min,max]=[" << wing_min << "," << wing_max << "]"
                      << " legs[min,max]=[" << leg_min << "," << leg_max << "]"
                      << " flat_steps=" << flat_steps
                      << " hold=" << twitch_hold_remaining
                      << " twitch=" << (injected_twitch ? 1 : 0)
                      << " wing_amp=" << wing_amp
                      << " leg_amp=" << injected_leg_amp
                      << std::endl;
        }
    }
};

struct NaoPostureBridge {
    struct PairJoint {
        int left = -1;
        int right = -1;
    };

    int head_yaw = -1;
    int head_pitch = -1;
    int hip_yaw_pitch = -1;
    PairJoint shoulder_pitch;
    PairJoint shoulder_roll;
    PairJoint elbow_roll;
    PairJoint hip_roll;
    PairJoint hip_pitch;
    PairJoint knee_pitch;
    PairJoint ankle_pitch;
    PairJoint ankle_roll;
    std::vector<int> key_indices;
    std::vector<float> filtered;
    bool active = false;
    bool twitch_fallback = true;
    int flat_steps = 0;
    int flat_steps_trigger = 5;
    int twitch_hold_steps = 18;
    int twitch_hold_remaining = 0;
    float flat_drive_eps = 0.03f;
    float uniform_drive_eps = 0.04f;
    float twitch_amp = 0.14f;
    float twitch_phase = 0.0f;
    float twitch_phase_step = 0.34f;
    size_t apply_counter = 0;
    size_t debug_interval = 0;

    NaoPostureBridge() {
        twitch_fallback = env_bool("NM_NAO_TWITCH_FALLBACK", true);
        flat_steps_trigger = env_int_range("NM_NAO_TWITCH_FLAT_STEPS", 5, 1, 100000);
        twitch_hold_steps = env_int_range("NM_NAO_TWITCH_HOLD_STEPS", 18, 1, 100000);
        flat_drive_eps = env_float_range("NM_NAO_TWITCH_DRIVE_EPS", 0.03f, 0.0001f, 0.4f);
        uniform_drive_eps =
          env_float_range("NM_NAO_TWITCH_UNIFORM_EPS", 0.04f, 0.0f, 0.5f);
        twitch_amp = env_float_range("NM_NAO_TWITCH_AMP", 0.14f, 0.005f, 0.8f);
        twitch_phase_step = env_float_range("NM_NAO_TWITCH_PHASE_STEP", 0.34f, 0.01f, 6.0f);
        debug_interval =
          static_cast<size_t>(env_int_range("NM_NAO_BRIDGE_DEBUG_INTERVAL", 0, 0, 100000));
    }

    static inline float clamp01(float v) {
        if (v < 0.0f) return 0.0f;
        if (v > 1.0f) return 1.0f;
        return v;
    }

    static inline float read_cmd(const std::vector<float>& all_a, int idx) {
        if (idx < 0 || idx >= (int)all_a.size()) return 0.5f;
        float v = all_a[(size_t)idx];
        if (!std::isfinite(v)) return 0.5f;
        if (v < 0.0f) return 0.0f;
        if (v > 1.0f) return 1.0f;
        return v;
    }

    static std::string canonical_name(const std::string& raw) {
        std::string out;
        out.reserve(raw.size());
        for (char ch : raw) {
            unsigned char u = static_cast<unsigned char>(ch);
            if (std::isalnum(u)) {
                out.push_back(static_cast<char>(std::tolower(u)));
            }
        }
        return out;
    }

    static int find_canonical(const std::map<std::string, int>& by_name, const std::string& key) {
        auto it = by_name.find(key);
        if (it == by_name.end()) return -1;
        return it->second;
    }

    void discover(const std::vector<std::string>& actuator_names) {
        std::map<std::string, int> by_name;
        for (size_t i = 0; i < actuator_names.size(); ++i) {
            const std::string key = canonical_name(actuator_names[i]);
            if (key.empty()) continue;
            if (by_name.find(key) == by_name.end()) {
                by_name[key] = (int)i;
            }
        }

        head_yaw = find_canonical(by_name, "headyaw");
        head_pitch = find_canonical(by_name, "headpitch");
        hip_yaw_pitch = find_canonical(by_name, "lhipyawpitch");
        if (hip_yaw_pitch < 0) {
            hip_yaw_pitch = find_canonical(by_name, "rhipyawpitch");
        }

        shoulder_pitch.left = find_canonical(by_name, "lshoulderpitch");
        shoulder_pitch.right = find_canonical(by_name, "rshoulderpitch");
        shoulder_roll.left = find_canonical(by_name, "lshoulderroll");
        shoulder_roll.right = find_canonical(by_name, "rshoulderroll");
        elbow_roll.left = find_canonical(by_name, "lelbowroll");
        elbow_roll.right = find_canonical(by_name, "relbowroll");
        hip_roll.left = find_canonical(by_name, "lhiproll");
        hip_roll.right = find_canonical(by_name, "rhiproll");
        hip_pitch.left = find_canonical(by_name, "lhippitch");
        hip_pitch.right = find_canonical(by_name, "rhippitch");
        knee_pitch.left = find_canonical(by_name, "lkneepitch");
        knee_pitch.right = find_canonical(by_name, "rkneepitch");
        ankle_pitch.left = find_canonical(by_name, "lanklepitch");
        ankle_pitch.right = find_canonical(by_name, "ranklepitch");
        ankle_roll.left = find_canonical(by_name, "lankleroll");
        ankle_roll.right = find_canonical(by_name, "rankleroll");

        auto add_key = [&](int idx) {
            if (idx < 0) return;
            if (std::find(key_indices.begin(), key_indices.end(), idx) == key_indices.end()) {
                key_indices.push_back(idx);
            }
        };
        add_key(head_yaw);
        add_key(head_pitch);
        add_key(hip_yaw_pitch);
        add_key(shoulder_pitch.left);
        add_key(shoulder_pitch.right);
        add_key(shoulder_roll.left);
        add_key(shoulder_roll.right);
        add_key(elbow_roll.left);
        add_key(elbow_roll.right);
        add_key(hip_roll.left);
        add_key(hip_roll.right);
        add_key(hip_pitch.left);
        add_key(hip_pitch.right);
        add_key(knee_pitch.left);
        add_key(knee_pitch.right);
        add_key(ankle_pitch.left);
        add_key(ankle_pitch.right);
        add_key(ankle_roll.left);
        add_key(ankle_roll.right);

        active =
          key_indices.size() >= 10 && shoulder_pitch.left >= 0 && shoulder_pitch.right >= 0 &&
          hip_pitch.left >= 0 && hip_pitch.right >= 0;
        filtered.assign(actuator_names.size(), 0.5f);

        if (active) {
            std::cout << "[nao_nn_controller_uds] Nao posture bridge active"
                      << " (key_joints=" << key_indices.size()
                      << ", twitch_fallback=" << (twitch_fallback ? 1 : 0)
                      << ", flat_steps=" << flat_steps_trigger
                      << ", hold_steps=" << twitch_hold_steps
                      << ", flat_eps=" << flat_drive_eps
                      << ", uniform_eps=" << uniform_drive_eps
                      << ", twitch_amp=" << twitch_amp
                      << ")"
                      << std::endl;
        }
    }

    float base_target(const std::vector<float>& all_a, int idx) const {
        float raw = read_cmd(all_a, idx);
        return 0.5f + (raw - 0.5f) * 0.82f;
    }

    void apply_joint(
      std::vector<float>& all_a,
      int idx,
      float target,
      float alpha,
      float min_v,
      float max_v,
      float& out_min,
      float& out_max
    ) {
        if (idx < 0 || idx >= (int)all_a.size()) return;
        if (target < min_v) target = min_v;
        if (target > max_v) target = max_v;
        filtered[(size_t)idx] = (1.0f - alpha) * filtered[(size_t)idx] + alpha * target;
        all_a[(size_t)idx] = clamp01(filtered[(size_t)idx]);
        out_min = std::min(out_min, all_a[(size_t)idx]);
        out_max = std::max(out_max, all_a[(size_t)idx]);
    }

    void apply_pair(
      std::vector<float>& all_a,
      const PairJoint& pair,
      float left_target,
      float right_target,
      float alpha,
      float min_v,
      float max_v,
      float& out_min,
      float& out_max
    ) {
        apply_joint(all_a, pair.left, left_target, alpha, min_v, max_v, out_min, out_max);
        apply_joint(all_a, pair.right, right_target, alpha, min_v, max_v, out_min, out_max);
    }

    void apply(std::vector<float>& all_a) {
        if (!active) return;
        if (filtered.size() < all_a.size()) filtered.resize(all_a.size(), 0.5f);

        float abs_drive_sum = 0.0f;
        float abs_drive_max = 0.0f;
        float cmd_min = 1.0f;
        float cmd_max = 0.0f;
        for (int idx : key_indices) {
            float cmd = read_cmd(all_a, idx);
            const float abs_drive = std::fabs(cmd - 0.5f);
            abs_drive_sum += abs_drive;
            abs_drive_max = std::max(abs_drive_max, abs_drive);
            cmd_min = std::min(cmd_min, cmd);
            cmd_max = std::max(cmd_max, cmd);
        }
        const float abs_drive_mean = abs_drive_sum / std::max<size_t>(1, key_indices.size());
        const float drive_spread = cmd_max - cmd_min;
        const bool low_motion_drive =
          abs_drive_max <= flat_drive_eps || abs_drive_mean <= (0.5f * flat_drive_eps) ||
          drive_spread <= uniform_drive_eps;

        if (low_motion_drive) {
            flat_steps = std::min(flat_steps + 1, 1000000);
        } else {
            flat_steps = std::max(0, flat_steps - 2);
        }
        if (twitch_fallback && flat_steps >= flat_steps_trigger) {
            twitch_hold_remaining = std::max(twitch_hold_remaining, twitch_hold_steps);
        } else if (!low_motion_drive && abs_drive_max >= (1.8f * flat_drive_eps)) {
            twitch_hold_remaining = 0;
        }

        bool injected_twitch = false;
        float injected_amp = 0.0f;
        const float activity_hint =
          std::max(0.0f, std::min(1.0f, abs_drive_mean / 0.16f));
        if (twitch_fallback && twitch_hold_remaining > 0) {
            injected_twitch = true;
            twitch_hold_remaining = std::max(0, twitch_hold_remaining - 1);
            injected_amp = twitch_amp * (0.65f + 0.35f * activity_hint);
            constexpr float kTau = 6.28318530718f;
            twitch_phase += twitch_phase_step;
            if (twitch_phase > kTau) {
                twitch_phase = std::fmod(twitch_phase, kTau);
            }
        }

        const float alpha = injected_twitch ? 0.30f : 0.18f;
        float out_min = 1.0f;
        float out_max = 0.0f;

        const float sway = std::sin(twitch_phase);
        const float sway_q = std::sin(twitch_phase + 1.570796327f);
        const float roll = std::sin(twitch_phase + 3.141592654f);

        float head_yaw_target = base_target(all_a, head_yaw);
        float head_pitch_target = base_target(all_a, head_pitch);
        float hip_yaw_pitch_target = base_target(all_a, hip_yaw_pitch);
        float l_shoulder_pitch_target = base_target(all_a, shoulder_pitch.left);
        float r_shoulder_pitch_target = base_target(all_a, shoulder_pitch.right);
        float l_shoulder_roll_target = base_target(all_a, shoulder_roll.left);
        float r_shoulder_roll_target = base_target(all_a, shoulder_roll.right);
        float l_elbow_roll_target = base_target(all_a, elbow_roll.left);
        float r_elbow_roll_target = base_target(all_a, elbow_roll.right);
        float l_hip_roll_target = base_target(all_a, hip_roll.left);
        float r_hip_roll_target = base_target(all_a, hip_roll.right);
        float l_hip_pitch_target = base_target(all_a, hip_pitch.left);
        float r_hip_pitch_target = base_target(all_a, hip_pitch.right);
        float l_knee_pitch_target = base_target(all_a, knee_pitch.left);
        float r_knee_pitch_target = base_target(all_a, knee_pitch.right);
        float l_ankle_pitch_target = base_target(all_a, ankle_pitch.left);
        float r_ankle_pitch_target = base_target(all_a, ankle_pitch.right);
        float l_ankle_roll_target = base_target(all_a, ankle_roll.left);
        float r_ankle_roll_target = base_target(all_a, ankle_roll.right);

        if (injected_twitch) {
            head_yaw_target += injected_amp * 0.45f * sway_q;
            head_pitch_target += injected_amp * 0.30f * sway;
            hip_yaw_pitch_target += injected_amp * 0.25f * sway_q;

            l_shoulder_pitch_target += injected_amp * 0.70f * sway;
            r_shoulder_pitch_target -= injected_amp * 0.70f * sway;
            l_shoulder_roll_target += injected_amp * 0.55f * roll;
            r_shoulder_roll_target -= injected_amp * 0.55f * roll;
            l_elbow_roll_target -= injected_amp * 0.45f * roll;
            r_elbow_roll_target += injected_amp * 0.45f * roll;

            l_hip_roll_target += injected_amp * 0.45f * roll;
            r_hip_roll_target -= injected_amp * 0.45f * roll;
            l_hip_pitch_target -= injected_amp * 0.75f * sway;
            r_hip_pitch_target += injected_amp * 0.75f * sway;
            l_knee_pitch_target += injected_amp * 0.65f * sway;
            r_knee_pitch_target -= injected_amp * 0.65f * sway;
            l_ankle_pitch_target -= injected_amp * 0.55f * sway;
            r_ankle_pitch_target += injected_amp * 0.55f * sway;
            l_ankle_roll_target -= injected_amp * 0.45f * roll;
            r_ankle_roll_target += injected_amp * 0.45f * roll;
        }

        apply_joint(all_a, head_yaw, head_yaw_target, alpha, 0.12f, 0.88f, out_min, out_max);
        apply_joint(all_a, head_pitch, head_pitch_target, alpha, 0.12f, 0.88f, out_min, out_max);
        apply_joint(
          all_a,
          hip_yaw_pitch,
          hip_yaw_pitch_target,
          alpha,
          0.16f,
          0.84f,
          out_min,
          out_max
        );
        apply_pair(
          all_a,
          shoulder_pitch,
          l_shoulder_pitch_target,
          r_shoulder_pitch_target,
          alpha,
          0.10f,
          0.90f,
          out_min,
          out_max
        );
        apply_pair(
          all_a,
          shoulder_roll,
          l_shoulder_roll_target,
          r_shoulder_roll_target,
          alpha,
          0.12f,
          0.88f,
          out_min,
          out_max
        );
        apply_pair(
          all_a,
          elbow_roll,
          l_elbow_roll_target,
          r_elbow_roll_target,
          alpha,
          0.12f,
          0.88f,
          out_min,
          out_max
        );
        apply_pair(
          all_a,
          hip_roll,
          l_hip_roll_target,
          r_hip_roll_target,
          alpha,
          0.14f,
          0.86f,
          out_min,
          out_max
        );
        apply_pair(
          all_a,
          hip_pitch,
          l_hip_pitch_target,
          r_hip_pitch_target,
          alpha,
          0.10f,
          0.90f,
          out_min,
          out_max
        );
        apply_pair(
          all_a,
          knee_pitch,
          l_knee_pitch_target,
          r_knee_pitch_target,
          alpha,
          0.08f,
          0.92f,
          out_min,
          out_max
        );
        apply_pair(
          all_a,
          ankle_pitch,
          l_ankle_pitch_target,
          r_ankle_pitch_target,
          alpha,
          0.10f,
          0.90f,
          out_min,
          out_max
        );
        apply_pair(
          all_a,
          ankle_roll,
          l_ankle_roll_target,
          r_ankle_roll_target,
          alpha,
          0.12f,
          0.88f,
          out_min,
          out_max
        );

        apply_counter += 1;
        if (debug_interval > 0 && apply_counter % debug_interval == 0) {
            std::cout << "[nao_nn_controller_uds] Nao bridge diag"
                      << " drive_abs[mean,max]=[" << abs_drive_mean << "," << abs_drive_max << "]"
                      << " spread=" << drive_spread
                      << " out[min,max]=[" << out_min << "," << out_max << "]"
                      << " flat_steps=" << flat_steps
                      << " hold=" << twitch_hold_remaining
                      << " twitch=" << (injected_twitch ? 1 : 0)
                      << " amp=" << injected_amp
                      << std::endl;
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
    // Keep NN time in lock-step with Webots time by default.
    const float ipc_dt_ms = ipc_dt_ms_override(static_cast<float>(dt));
    DeviceMapper mapper;
    mapper.discover(robot, dt);
    std::cout << "[nao_nn_controller_uds] Webots timestep=" << dt
              << "ms, NN IPC dt=" << ipc_dt_ms << "ms" << std::endl;
    if (std::fabs(ipc_dt_ms - static_cast<float>(dt)) > 1e-6f) {
        std::cerr << "[nao_nn_controller_uds] WARNING: NM_IPC_DT_MS (" << ipc_dt_ms
                  << "ms) differs from Webots basicTimeStep (" << dt
                  << "ms); this can cause sensory timing mismatch."
                  << std::endl;
    }

    KeyboardMapper kb_mapper;
    kb_mapper.autodetect(robot);
    if (kb_mapper.is_enabled()) {
        load_motions();
    }

    auto all_s_names = mapper.get_sensor_names();
    auto all_o_names = mapper.get_actuator_names();
    CelegansMuscleBridge celegans_bridge;
    celegans_bridge.discover(all_o_names);
    DrosophilaMotionBridge dros_bridge;
    dros_bridge.discover(all_o_names);
    HexapodLegBridge hexapod_bridge;
    hexapod_bridge.discover(all_o_names);
    NaoPostureBridge nao_bridge;
    nao_bridge.discover(all_o_names);
    const bool handshake_include_names = env_bool("NM_IPC_HANDSHAKE_INCLUDE_NAMES", false);
    const int handshake_max_bytes = env_int_range("NM_IPC_HANDSHAKE_MAX_BYTES", 7000, 256, 1 << 20);
    const double timeout_grace_s = static_cast<double>(ipc_timeout_grace_ms()) / 1000.0;
    const double timeout_log_interval_s =
      static_cast<double>(ipc_timeout_log_interval_ms()) / 1000.0;
    const int webots_step_sleep_ms =
      env_int_range("NM_WEBOTS_STEP_SLEEP_MS", 0, 0, 600000);
    if (webots_step_sleep_ms > 0) {
        std::cout << "[nao_nn_controller_uds] Extra Webots step sleep="
                  << webots_step_sleep_ms << "ms (slow-mode)"
                  << std::endl;
    }
    const bool ipc_lockstep = env_bool("NM_IPC_LOCKSTEP", true);
    const int ipc_lockstep_sleep_us =
      env_int_range("NM_IPC_LOCKSTEP_SLEEP_US", 1000, 0, 500000);
    const int ipc_lockstep_max_wait_ms =
      env_int_range("NM_IPC_LOCKSTEP_MAX_WAIT_MS", 0, 0, 3600000);
    const double ipc_lockstep_wait_log_s =
      static_cast<double>(
        env_int_range("NM_IPC_LOCKSTEP_LOG_INTERVAL_MS", 2000, 100, 60000)
      ) /
      1000.0;
    const double ipc_handshake_retry_s =
      static_cast<double>(env_int_range("NM_IPC_HANDSHAKE_RETRY_MS", 1000, 100, 60000)) /
      1000.0;

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
        ss << "{\"sensory\":" << b.s_names.size()
           << ",\"output\":" << b.o_names.size()
           << ",\"dt_ms\":" << ipc_dt_ms;
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
        b.req_buf.assign(1 + b.s_names.size(), 0.0f); // [dt_ms] + s_buf; pre-allocated
        
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

    auto try_send_handshake = [&](BrainInstance& b, double now_s) {
        if (b.connected) return;
        if (now_s - b.last_handshake_time < ipc_handshake_retry_s) return;

        if (b.cli->send_raw(b.handshake_json)) {
            std::cout << "[nao_nn_controller_uds] Brain '" << b.id
                      << "': Sent handshake JSON" << std::endl;
            b.last_handshake_time = now_s;
            return;
        }

        if (errno == ECONNREFUSED) {
            std::cerr << "[nao_nn_controller_uds] Brain '" << b.id
                      << "': Handshake failed (Connection refused)." << std::endl;
        } else {
            std::cerr << "[nao_nn_controller_uds] Brain '" << b.id
                      << "': Handshake failed (errno=" << errno << ")." << std::endl;
        }
        b.last_handshake_time = now_s;
    };

    // 5. Main Loop
    std::vector<float> all_s(all_s_names.size(), 0.0f);
    std::vector<float> all_a(all_o_names.size(), 0.5f);

    while (robot.step(dt) != -1) {
        double now = monotonic_now_seconds();

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
            try_send_handshake(b, now);

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

            // Request frame: [t_ms] + [S floats] — reuse pre-allocated req_buf.
            b.req_buf[0] = ipc_dt_ms;
            std::copy(b.s_buf.begin(), b.s_buf.end(), b.req_buf.begin() + 1);

            bool xfer_ok = false;
            int xfer_errno = 0;
            const double wait_start_s = now;
            double last_wait_log_s = now;
            bool queued_req_for_step = false;

            while (true) {
                now = monotonic_now_seconds();
                try_send_handshake(b, now);

                if (b.cli->xfer(b.req_buf, b.a_buf, now, !queued_req_for_step)) {
                    xfer_ok = true;
                    break;
                }
                queued_req_for_step = true;

                xfer_errno = errno;
                if (!ipc_lockstep) {
                    break;
                }

                const double waited_s = now - wait_start_s;
                if (
                  ipc_lockstep_max_wait_ms > 0 &&
                  waited_s * 1000.0 >= static_cast<double>(ipc_lockstep_max_wait_ms)
                ) {
                    xfer_errno = ETIMEDOUT;
                    break;
                }

                if (now - last_wait_log_s >= ipc_lockstep_wait_log_s) {
                    std::cerr << "[nao_nn_controller_uds] Brain '" << b.id
                              << "': Waiting for NN lock-step reply (waited_ms="
                              << static_cast<int>(std::llround(std::max(0.0, waited_s) * 1000.0))
                              << ", win=" << b.cli->window_size()
                              << ", in_flight=" << b.cli->inflight_updates()
                              << ", queued=" << b.cli->pending_updates()
                              << ")" << std::endl;
                    last_wait_log_s = now;
                }

                if (ipc_lockstep_sleep_us > 0) {
                    std::this_thread::sleep_for(
                      std::chrono::microseconds(ipc_lockstep_sleep_us)
                    );
                }
            }

            if (xfer_ok) {
                if (!b.connected) {
                    std::cout << "[nao_nn_controller_uds] Brain '" << b.id << "': Connected." << std::endl;
                }
                b.connected = true;
                b.last_success_time = now;

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

                if (now - b.last_diag_time >= 2.0) {
                    size_t sensory_active = 0;
                    float sensory_max = 0.0f;
                    for (float v : b.s_buf) {
                        if (!std::isfinite(v)) continue;
                        if (v >= 0.5f) sensory_active += 1;
                        sensory_max = std::max(sensory_max, v);
                    }

                    size_t output_active = 0;
                    float out_min = 1.0f;
                    float out_max = 0.0f;
                    for (float v : b.a_buf) {
                        float clamped = std::isfinite(v) ? v : 0.5f;
                        clamped = std::max(0.0f, std::min(1.0f, clamped));
                        out_min = std::min(out_min, clamped);
                        out_max = std::max(out_max, clamped);
                        if (std::fabs(clamped - 0.5f) >= 0.015f) {
                            output_active += 1;
                        }
                    }
                    if (b.a_buf.empty()) {
                        out_min = 0.0f;
                    }

                    const uint64_t tx_total = b.cli->tx_frames_total();
                    const uint64_t rx_total = b.cli->rx_frames_total();
                    const uint64_t aer_total = b.cli->tx_aer_frames();
                    const uint64_t raw_total = b.cli->tx_raw_frames();
                    const uint64_t aer_events_total = b.cli->tx_aer_events();
                    const uint64_t rx_aer_total = b.cli->rx_aer_frames();
                    const uint64_t rx_raw_total = b.cli->rx_raw_frames();
                    const uint64_t rx_aer_events_total = b.cli->rx_aer_events();

                    const uint64_t tx_delta =
                      tx_total >= b.diag_tx_frames_last ? (tx_total - b.diag_tx_frames_last) : tx_total;
                    const uint64_t rx_delta =
                      rx_total >= b.diag_rx_frames_last ? (rx_total - b.diag_rx_frames_last) : rx_total;
                    const uint64_t aer_delta =
                      aer_total >= b.diag_aer_frames_last ? (aer_total - b.diag_aer_frames_last) : aer_total;
                    const uint64_t raw_delta =
                      raw_total >= b.diag_raw_frames_last ? (raw_total - b.diag_raw_frames_last) : raw_total;
                    const uint64_t aer_events_delta =
                      aer_events_total >= b.diag_aer_events_last ? (aer_events_total - b.diag_aer_events_last) : aer_events_total;
                    const uint64_t rx_aer_delta =
                      rx_aer_total >= b.diag_rx_aer_frames_last ? (rx_aer_total - b.diag_rx_aer_frames_last) : rx_aer_total;
                    const uint64_t rx_raw_delta =
                      rx_raw_total >= b.diag_rx_raw_frames_last ? (rx_raw_total - b.diag_rx_raw_frames_last) : rx_raw_total;
                    const uint64_t rx_aer_events_delta =
                      rx_aer_events_total >= b.diag_rx_aer_events_last ? (rx_aer_events_total - b.diag_rx_aer_events_last) : rx_aer_events_total;

                    b.diag_tx_frames_last = tx_total;
                    b.diag_rx_frames_last = rx_total;
                    b.diag_aer_frames_last = aer_total;
                    b.diag_raw_frames_last = raw_total;
                    b.diag_aer_events_last = aer_events_total;
                    b.diag_rx_aer_frames_last = rx_aer_total;
                    b.diag_rx_raw_frames_last = rx_raw_total;
                    b.diag_rx_aer_events_last = rx_aer_events_total;
                    b.last_diag_time = now;

                    std::cout << "[nao_nn_controller_uds] Brain '" << b.id
                              << "': IPC diag sensory>=0.5 " << sensory_active << "/" << b.s_buf.size()
                              << " (max=" << sensory_max << ")"
                              << " output!=neutral " << output_active << "/" << b.a_buf.size()
                              << " out[min,max]=[" << out_min << "," << out_max << "]"
                              << " tx/rx+=" << tx_delta << "/" << rx_delta
                              << " mode+=" << aer_delta << " AER, " << raw_delta << " raw"
                              << " aer_events+=" << aer_events_delta
                              << " rx_mode+=" << rx_aer_delta << " AER, " << rx_raw_delta << " raw"
                              << " rx_aer_events+=" << rx_aer_events_delta
                              << std::endl;
                }
            } else {
                errno = xfer_errno;
                if (errno == EAGAIN || errno == EWOULDBLOCK) {
                    // Treat receive timeout as transient; keep last actuator outputs and
                    // maintain the connected state to avoid flapping under load.
                    const double since_ok_s = now - b.last_success_time;
                    if (b.last_success_time > 0.0 && since_ok_s < timeout_grace_s) {
                        continue;
                    }
                    bool should_log = (now - b.last_error_time >= timeout_log_interval_s);
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
                                  << ", since_last_ok_ms="
                                  << static_cast<int>(std::max(0.0, since_ok_s) * 1000.0)
                                  << ", stale_dropped+=" << stale_delta
                                  << ")" << std::endl;
                        b.stale_dropped_last_log = stale_total;
                        b.last_error_time = now;
                    }
                    continue;
                }
                bool should_log = (now - b.last_error_time >= timeout_log_interval_s);
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
            dros_bridge.apply(all_a);
            hexapod_bridge.apply(all_a);
            nao_bridge.apply(all_a);
            mapper.apply_actuators(all_a);
        }
        if (webots_step_sleep_ms > 0) {
            std::this_thread::sleep_for(
              std::chrono::milliseconds(webots_step_sleep_ms)
            );
        }
    }
    return 0;
}
