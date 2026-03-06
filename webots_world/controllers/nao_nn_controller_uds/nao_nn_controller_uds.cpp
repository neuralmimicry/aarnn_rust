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
//         --socket /tmp/neuromorphic_demo.nn --sensory 25 --output 11 --threshold 0.2 --ui
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

  bool xfer(const std::vector<float>& req, std::vector<float>& rep){
    // Send request bytes
    const size_t nbytes = req.size()*sizeof(float);
    ssize_t sent = ::sendto(sock_, (const char*)req.data(), nbytes, 0, (struct sockaddr*)&server_, sizeof(server_));
    if (sent < 0) {
      // Return false and let main handle throttled error reporting
      return false;
    }
    if ((size_t)sent != nbytes) {
      return false;
    }

    // Receive reply bytes
    std::vector<char> buf(rep.size()*sizeof(float));
    struct sockaddr_un from{};
    socklen_t fromlen = sizeof(from);
    ssize_t r = ::recvfrom(sock_, buf.data(), buf.size(), 0, (struct sockaddr*)&from, &fromlen);
    if (r < 0) {
      return false;
    }
    if ((size_t)r != buf.size()) {
      return false;
    }
    std::memcpy(rep.data(), buf.data(), buf.size());
    return true;
  }

private:
  int sock_ = -1;
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
            b.sock_path = home_str + "/neuromorphic_demo.nn";
        } else {
            b.sock_path = home_str + "/neuromorphic_demo." + id + ".nn";
        }
        brains.push_back(std::move(b));
    }

    // 2. Webots setup
    Robot robot;
    const int dt = (int)robot.getBasicTimeStep();
    DeviceMapper mapper;
    mapper.discover(robot, dt);

    KeyboardMapper kb_mapper;
    kb_mapper.autodetect(robot);
    load_motions();

    auto all_s_names = mapper.get_sensor_names();
    auto all_o_names = mapper.get_actuator_names();

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

        // Build brain-specific handshake
        std::stringstream ss;
        ss << "{\"s_names\":[";
        for (size_t i=0; i<b.s_names.size(); ++i) ss << "\"" << b.s_names[i] << "\"" << (i==b.s_names.size()-1 ? "" : ",");
        ss << "],\"o_names\":[";
        for (size_t i=0; i<b.o_names.size(); ++i) ss << "\"" << b.o_names[i] << "\"" << (i==b.o_names.size()-1 ? "" : ",");
        ss << "]}";
        b.handshake_json = ss.str();

        b.s_buf.assign(b.s_names.size(), 0.0f);
        b.a_buf.assign(b.o_names.size(), 0.5f);
        
        b.cli = std::make_unique<UdsClient>(b.sock_path);
        std::cout << "[nao_nn_controller_uds] Brain '" << b.id << "': S=" << b.s_names.size() << " O=" << b.o_names.size() << " socket=" << b.sock_path << std::endl;
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
            req.push_back((float)dt);
            req.insert(req.end(), b.s_buf.begin(), b.s_buf.end());

            if (b.cli->xfer(req, b.a_buf)) {
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
            mapper.apply_actuators(all_a);
        }
    }
    return 0;
}
