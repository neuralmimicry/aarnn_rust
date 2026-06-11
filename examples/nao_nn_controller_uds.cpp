// Webots NAO controller that talks to the NN server over Unix datagram sockets (UDS).
// Preferred payload is AER (Address-Event Representation); legacy float frames are
// still supported via NM_IPC_FORMAT=float.
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

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

#include <memory>
#include <regex>
#include <map>
#include <set>
#include <sstream>

using namespace webots;

struct AerEvent {
  uint64_t ts_us;
  uint32_t addr;
  uint8_t value;
};

static const uint8_t AER_MAGIC[4] = {'A', 'E', 'R', '1'};

static void write_varint(uint64_t value, std::vector<uint8_t>& out) {
  while (value >= 0x80) {
    out.push_back(static_cast<uint8_t>((value & 0x7F) | 0x80));
    value >>= 7;
  }
  out.push_back(static_cast<uint8_t>(value & 0xFF));
}

static bool read_varint(const uint8_t* data, size_t len, size_t& offset, uint64_t& out) {
  uint64_t result = 0;
  uint32_t shift = 0;
  while (offset < len) {
    uint8_t b = data[offset++];
    result |= (static_cast<uint64_t>(b & 0x7F) << shift);
    if ((b & 0x80) == 0) {
      out = result;
      return true;
    }
    shift += 7;
    if (shift >= 64) return false;
  }
  return false;
}

static std::vector<uint8_t> encode_aer_events(const std::vector<AerEvent>& events) {
  if (events.empty()) return {};
  std::vector<AerEvent> sorted = events;
  std::sort(sorted.begin(), sorted.end(), [](const AerEvent& a, const AerEvent& b){ return a.ts_us < b.ts_us; });
  uint64_t base_ts = sorted.front().ts_us;
  std::vector<uint8_t> out;
  out.reserve(12 + sorted.size() * 6);
  out.insert(out.end(), AER_MAGIC, AER_MAGIC + 4);
  for (int i = 0; i < 8; ++i) out.push_back(static_cast<uint8_t>((base_ts >> (8*i)) & 0xFF));
  uint64_t prev_ts = base_ts;
  for (const auto& ev : sorted) {
    uint64_t delta = (ev.ts_us >= prev_ts) ? (ev.ts_us - prev_ts) : 0;
    prev_ts = ev.ts_us;
    write_varint(delta, out);
    write_varint(ev.addr, out);
    write_varint(ev.value, out);
  }
  return out;
}

static bool decode_aer_events(const std::vector<uint8_t>& payload, std::vector<AerEvent>& out_events) {
  if (payload.size() < 12) return false;
  if (std::memcmp(payload.data(), AER_MAGIC, 4) != 0) return false;
  uint64_t base_ts = 0;
  for (int i = 0; i < 8; ++i) {
    base_ts |= (static_cast<uint64_t>(payload[4 + i]) << (8*i));
  }
  size_t idx = 12;
  uint64_t prev_ts = base_ts;
  out_events.clear();
  while (idx < payload.size()) {
    uint64_t delta = 0, addr = 0, value = 0;
    if (!read_varint(payload.data(), payload.size(), idx, delta)) return false;
    if (!read_varint(payload.data(), payload.size(), idx, addr)) return false;
    if (!read_varint(payload.data(), payload.size(), idx, value)) return false;
    prev_ts += delta;
    out_events.push_back(AerEvent { prev_ts, static_cast<uint32_t>(addr), static_cast<uint8_t>(value & 0xFF) });
  }
  return true;
}

static std::vector<uint8_t> aer_keepalive(uint64_t ts_us) {
  std::vector<uint8_t> out;
  out.reserve(12);
  out.insert(out.end(), AER_MAGIC, AER_MAGIC + 4);
  for (int i = 0; i < 8; ++i) out.push_back(static_cast<uint8_t>((ts_us >> (8*i)) & 0xFF));
  return out;
}

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

// Minimal UDS datagram helper
class UdsClient {
public:
  UdsClient(const std::string& server_path)
  : server_path_(server_path) {
    sock_ = ::socket(AF_UNIX, SOCK_DGRAM, 0);
    if (sock_ < 0) { perror("socket"); throw std::runtime_error("socket"); }

    // Set receive timeout (2 seconds)
    struct timeval tv;
    tv.tv_sec = 2;
    tv.tv_usec = 0;
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
    std::cout << "[nao_nn_controller_uds] Bound local socket to " << client_path << std::endl;
    // Prepare server addr
    server_.sun_family = AF_UNIX;
    std::snprintf(server_.sun_path, sizeof(server_.sun_path), "%s", server_path_.c_str());
  }
  ~UdsClient(){ if (sock_>=0) ::close(sock_); }

  bool send_raw(const std::string& data) {
    ssize_t sent = ::sendto(sock_, data.c_str(), data.size(), 0, (struct sockaddr*)&server_, sizeof(server_));
    if (sent < 0) {
      return false;
    }
    return (size_t)sent == data.size();
  }

  bool transfer_data(const std::vector<float>& request, std::vector<float>& reply){
    // Send request bytes
    const size_t num_bytes = request.size()*sizeof(float);
    ssize_t bytes_sent = ::sendto(sock_, (const char*)request.data(), num_bytes, 0, (struct sockaddr*)&server_, sizeof(server_));
    if (bytes_sent < 0) {
      // Return false and let main handle throttled error reporting
      return false;
    }
    if ((size_t)bytes_sent != num_bytes) {
      return false;
    }

    // Receive reply bytes
    std::vector<char> buffer(reply.size()*sizeof(float));
    struct sockaddr_un from_address{};
    socklen_t from_len = sizeof(from_address);
    ssize_t bytes_received = ::recvfrom(sock_, buffer.data(), buffer.size(), 0, (struct sockaddr*)&from_address, &from_len);
    if (bytes_received < 0) {
      return false;
    }
    if ((size_t)bytes_received != buffer.size()) {
      return false;
    }
    std::memcpy(reply.data(), buffer.data(), buffer.size());
    return true;
  }

  bool transfer_bytes(const std::vector<uint8_t>& request, std::vector<uint8_t>& reply, size_t max_reply = 8192) {
    ssize_t bytes_sent = ::sendto(sock_, (const char*)request.data(), request.size(), 0, (struct sockaddr*)&server_, sizeof(server_));
    if (bytes_sent < 0) {
      return false;
    }
    if ((size_t)bytes_sent != request.size()) {
      return false;
    }

    std::vector<uint8_t> buffer(max_reply);
    struct sockaddr_un from_address{};
    socklen_t from_len = sizeof(from_address);
    ssize_t bytes_received = ::recvfrom(sock_, buffer.data(), buffer.size(), 0, (struct sockaddr*)&from_address, &from_len);
    if (bytes_received < 0) {
      return false;
    }
    buffer.resize(static_cast<size_t>(bytes_received));
    reply.swap(buffer);
    return true;
  }

private:
  int sock_ = -1;
  std::string server_path_;
  struct sockaddr_un server_{};
};

struct BrainInstance {
    std::string id;
    std::string socket_path;
    std::unique_ptr<UdsClient> uds_client;
    std::string handshake_json;
    bool is_connected = false;
    double last_handshake_time = -10.0;
    double last_error_time = -10.0;

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
    std::vector<float> sensory_buffer;
    std::vector<float> actuator_buffer;
    std::vector<std::string> sensory_names;
    std::vector<std::string> output_names;
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

    // IPC format preferences
    bool use_aer = true;
    if (const char* fmt_env = std::getenv("NM_IPC_FORMAT")) {
        std::string fmt = fmt_env;
        std::transform(fmt.begin(), fmt.end(), fmt.begin(), [](unsigned char c){ return std::tolower(c); });
        if (fmt == "float") use_aer = false;
        if (fmt == "aer") use_aer = true;
    }
    float ipc_threshold = 0.5f;
    if (const char* thr_env = std::getenv("NM_IPC_THRESHOLD")) {
        try { ipc_threshold = std::stof(thr_env); } catch (...) {}
    }
    uint32_t aer_s_base = 4096;
    uint32_t aer_o_base = 16384;
    if (const char* base_env = std::getenv("NM_AER_S_BASE")) {
        try { aer_s_base = static_cast<uint32_t>(std::stoul(base_env)); } catch (...) {}
    }
    if (const char* base_env = std::getenv("NM_AER_O_BASE")) {
        try { aer_o_base = static_cast<uint32_t>(std::stoul(base_env)); } catch (...) {}
    }
    std::cout << "[nao_nn_controller_uds] IPC format=" << (use_aer ? "aer" : "float")
              << " thr=" << ipc_threshold
              << " aer_s_base=" << aer_s_base
              << " aer_o_base=" << aer_o_base
              << std::endl;

    // 1. Discover Brains
    const char* brains_env = std::getenv("NM_BRAINS");
    std::vector<std::string> brain_ids = brains_env ? split(brains_env, ',') : std::vector<std::string>{"default"};
    
    std::vector<BrainInstance> brains;
    const char* home = std::getenv("HOME");
    std::string home_str = home ? home : "/tmp";

    for (const auto& id : brain_ids) {
        BrainInstance brain;
        brain.id = id;
        if (id == "default") {
            brain.socket_path = home_str + "/aarnn_rust.nn";
        } else {
            brain.socket_path = home_str + "/aarnn_rust." + id + ".nn";
        }
        brains.push_back(std::move(brain));
    }

    // 2. Webots setup
    Robot robot;
    const int basic_time_step = (int)robot.getBasicTimeStep();
    DeviceMapper mapper;
    mapper.discover(robot, basic_time_step);

    KeyboardMapper kb_mapper;
    kb_mapper.autodetect(robot);
    load_motions();

    auto all_sensory_names = mapper.get_sensor_names();
    auto all_actuator_names = mapper.get_actuator_names();

    // 3. Configure Mapping and Interconnects
    // Interconnect format: NM_INTERCONNECT=brain1->brain2:8,brain2->brain1:4
    const char* interconnect_env = std::getenv("NM_INTERCONNECT");
    std::vector<std::string> interconnect_links = interconnect_env ? split(interconnect_env, ',') : std::vector<std::string>();

    for (auto& brain : brains) {
        // Real sensors ownership (default all)
        std::string sensory_regex_str = ".*";
        char* sensors_env = std::getenv(("NM_SENSORS_" + brain.id).c_str());
        if (sensors_env) sensory_regex_str = sensors_env;
        std::regex sensory_regex(sensory_regex_str);

        for (size_t i = 0; i < all_sensory_names.size(); ++i) {
            if (std::regex_match(all_sensory_names[i], sensory_regex)) {
                brain.real_sensor_indices.push_back(i);
                brain.sensory_names.push_back(all_sensory_names[i]);
            }
        }

        // Real actuators ownership (default all)
        std::string actuator_regex_str = ".*";
        char* actuators_env = std::getenv(("NM_ACTUATORS_" + brain.id).c_str());
        if (actuators_env) actuator_regex_str = actuators_env;
        std::regex actuator_regex(actuator_regex_str);

        for (size_t i = 0; i < all_actuator_names.size(); ++i) {
            if (std::regex_match(all_actuator_names[i], actuator_regex)) {
                brain.real_actuator_indices.push_back(i);
                brain.output_names.push_back(all_actuator_names[i]);
            }
        }

        // Virtual sensors (Inputs to this brain)
        for (const auto& link_str : interconnect_links) {
            // parse brainA->brainB:count
            std::smatch match_result;
            if (std::regex_match(link_str, match_result, std::regex("(.+)->(.+):(\\d+)"))) {
                std::string src = match_result[1];
                std::string dst = match_result[2];
                int count = std::stoi(match_result[3]);
                if (dst == brain.id) {
                    BrainInstance::Link l{src, count, (int)brain.sensory_names.size()};
                    for (int i=0; i<count; ++i) brain.sensory_names.push_back("From_" + src + "_" + std::to_string(i));
                    brain.virtual_sensors.push_back(l);
                }
                if (src == brain.id) {
                    BrainInstance::Link l{dst, count, (int)brain.output_names.size()};
                    for (int i=0; i<count; ++i) brain.output_names.push_back("To_" + dst + "_" + std::to_string(i));
                    brain.virtual_actuators.push_back(l);
                }
            }
        }

        // Build brain-specific handshake
        std::stringstream ss;
        ss << "{\"s_names\":[";
        for (size_t i=0; i<brain.sensory_names.size(); ++i) ss << "\"" << brain.sensory_names[i] << "\"" << (i==brain.sensory_names.size()-1 ? "" : ",");
        ss << "],\"o_names\":[";
        for (size_t i=0; i<brain.output_names.size(); ++i) ss << "\"" << brain.output_names[i] << "\"" << (i==brain.output_names.size()-1 ? "" : ",");
        ss << "],\"format\":\"" << (use_aer ? "aer" : "float") << "\"";
        ss << ",\"dt_ms\":" << static_cast<float>(basic_time_step);
        ss << ",\"aer_s_base\":" << aer_s_base;
        ss << ",\"aer_o_base\":" << aer_o_base;
        ss << "}";
        brain.handshake_json = ss.str();

        brain.sensory_buffer.assign(brain.sensory_names.size(), 0.0f);
        brain.actuator_buffer.assign(brain.output_names.size(), 0.5f);
        
        brain.uds_client = std::make_unique<UdsClient>(brain.socket_path);
        std::cout << "[nao_nn_controller_uds] Brain '" << brain.id << "': S=" << brain.sensory_names.size() << " O=" << brain.output_names.size() << " socket=" << brain.socket_path << std::endl;
    }

    // 4. Interconnect storage
    // Map LinkKey -> vector of floats
    struct LinkKey { 
        std::string src, dst; 
        bool operator<(const LinkKey& o) const { return src < o.src || (src == o.src && dst < o.dst); }
    };
    std::map<LinkKey, std::vector<float>> interconnect_bus;

    // 5. Main Loop
    std::vector<float> all_sensory_values(all_sensory_names.size(), 0.0f);
    std::vector<float> all_actuator_values(all_actuator_names.size(), 0.5f);

    while (robot.step(basic_time_step) != -1) {
        double current_time = robot.getTime();

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

        // 1) Read physical sensors
        mapper.fill_sensors(all_sensory_values);

        // 2) Process each Brain
        for (auto& brain : brains) {
            // handshake
            if (!brain.is_connected && (current_time - brain.last_handshake_time >= 5.0)) {
                if (brain.uds_client->send_raw(brain.handshake_json)) {
                    std::cout << "[nao_nn_controller_uds] Brain '" << brain.id << "': Sent handshake JSON" << std::endl;
                    brain.last_handshake_time = current_time;
                } else {
                    if (errno == ECONNREFUSED) {
                        std::cerr << "[nao_nn_controller_uds] Brain '" << brain.id << "': Handshake failed (Connection refused)." << std::endl;
                    } else {
                        std::cerr << "[nao_nn_controller_uds] Brain '" << brain.id << "': Handshake failed (errno=" << errno << ")." << std::endl;
                    }
                    brain.last_handshake_time = current_time;
                }
            }

            // Fill sensory buffer for this brain
            // Part A: Real sensors
            for (size_t i = 0; i < brain.real_sensor_indices.size(); ++i) {
                brain.sensory_buffer[i] = all_sensory_values[brain.real_sensor_indices[i]];
            }
            // Part B: Virtual sensors (Interconnects)
            for (const auto& link : brain.virtual_sensors) {
                std::vector<float>& link_data = interconnect_bus[{link.peer_id, brain.id}];
                if (link_data.size() == (size_t)link.count) {
                    for (int i = 0; i < link.count; ++i) {
                        brain.sensory_buffer[link.local_start + i] = link_data[i];
                    }
                }
            }

            bool ok = false;
            if (use_aer) {
                uint64_t ts_us = static_cast<uint64_t>(current_time * 1e6);
                std::vector<AerEvent> events;
                events.reserve(brain.sensory_buffer.size());
                for (size_t i = 0; i < brain.sensory_buffer.size(); ++i) {
                    if (brain.sensory_buffer[i] >= ipc_threshold) {
                        events.push_back(AerEvent { ts_us, aer_s_base + static_cast<uint32_t>(i), 1 });
                    }
                }
                std::vector<uint8_t> request_bytes = encode_aer_events(events);
                if (request_bytes.empty()) {
                    request_bytes = aer_keepalive(ts_us);
                }
                std::vector<uint8_t> reply_bytes;
                if (brain.uds_client->transfer_bytes(request_bytes, reply_bytes)) {
                    std::vector<AerEvent> out_events;
                    if (decode_aer_events(reply_bytes, out_events)) {
                        std::fill(brain.actuator_buffer.begin(), brain.actuator_buffer.end(), 0.0f);
                        for (const auto& ev : out_events) {
                            if (ev.value == 0) continue;
                            uint32_t idx = (ev.addr >= aer_o_base) ? (ev.addr - aer_o_base) : ev.addr;
                            if (idx < brain.actuator_buffer.size()) {
                                brain.actuator_buffer[idx] = 1.0f;
                            }
                        }
                        ok = true;
                    }
                }
            } else {
                // Legacy float request: [basic_time_step] + [S floats]
                std::vector<float> request_frame;
                request_frame.reserve(1 + brain.sensory_buffer.size());
                request_frame.push_back((float)basic_time_step);
                request_frame.insert(request_frame.end(), brain.sensory_buffer.begin(), brain.sensory_buffer.end());
                ok = brain.uds_client->transfer_data(request_frame, brain.actuator_buffer);
            }

            if (ok) {
                if (!brain.is_connected) {
                    std::cout << "[nao_nn_controller_uds] Brain '" << brain.id << "': Connected." << std::endl;
                }
                brain.is_connected = true;

                // Process outputs
                // Part A: Real actuators (Update global actuator buffer)
                for (size_t i = 0; i < brain.real_actuator_indices.size(); ++i) {
                    all_actuator_values[brain.real_actuator_indices[i]] = brain.actuator_buffer[i];
                }
                // Part B: Virtual actuators (Update interconnect bus)
                for (const auto& link : brain.virtual_actuators) {
                    std::vector<float>& link_data = interconnect_bus[{brain.id, link.peer_id}];
                    link_data.resize(link.count);
                    for (int i = 0; i < link.count; ++i) {
                        link_data[i] = brain.actuator_buffer[link.local_start + i];
                    }
                }
            } else {
                bool should_log_error = (current_time - brain.last_error_time >= 5.0);
                if (brain.is_connected) {
                    std::cerr << "[nao_nn_controller_uds] Brain '" << brain.id << "': Connection lost." << std::endl;
                    should_log_error = true;
                }
                if (should_log_error) {
                    std::cerr << "[nao_nn_controller_uds] Brain '" << brain.id << "': Transfer failed (errno=" << errno << ")" << std::endl;
                    brain.last_error_time = current_time;
                }
                brain.is_connected = false;
            }
        }

        // 3) Apply pooled actuators
        bool is_motion_active = false;
        if (currently_playing) {
            if (wbu_motion_is_over(currently_playing)) currently_playing = NULL;
            else is_motion_active = true;
        }

        if (!is_motion_active) {
            mapper.apply_actuators(all_actuator_values);
        }
    }
    return 0;
}
