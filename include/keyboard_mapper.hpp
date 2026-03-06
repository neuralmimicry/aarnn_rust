#pragma once
#include <webots/Keyboard.hpp>
#include <webots/Robot.hpp>
#include <string>
#include <vector>
#include <iostream>

namespace webots {

enum KeySet {
    KEYSET_ARROWS,
    KEYSET_WASD,
    KEYSET_IJKL,
    KEYSET_NONE
};

struct KeyMapping {
    int up, down, left, right, turn_left, turn_right;
    int tai_chi, wipe, wave;
};

class KeyboardMapper {
public:
    KeyboardMapper() : key_set_(KEYSET_NONE) {}

    void autodetect(Robot& robot) {
        const char* env = std::getenv("NM_KEYBOARD");
        if (env) {
            std::string s(env);
            if (s == "ARROWS") key_set_ = KEYSET_ARROWS;
            else if (s == "WASD") key_set_ = KEYSET_WASD;
            else if (s == "IJKL") key_set_ = KEYSET_IJKL;
            else if (s == "NONE") key_set_ = KEYSET_NONE;
            else if (s == "AUTO") autodetect_by_name(robot);
            else {
                std::cerr << "[KeyboardMapper] Unknown NM_KEYBOARD value: " << s << ". Defaulting to NONE." << std::endl;
                key_set_ = KEYSET_NONE;
            }
        } else {
            autodetect_by_name(robot);
        }

        if (key_set_ != KEYSET_NONE) {
            robot.getKeyboard()->enable((int)robot.getBasicTimeStep() * 10);
            std::cout << "[KeyboardMapper] Enabled key set: " << get_set_name() << " for robot '" << robot.getName() << "'" << std::endl;
        } else {
            std::cout << "[KeyboardMapper] Keyboard control disabled for robot '" << robot.getName() << "'" << std::endl;
        }
    }

    bool is_enabled() const { return key_set_ != KEYSET_NONE; }

    KeyMapping get_mapping() const {
        switch (key_set_) {
            case KEYSET_WASD:
                // W/S: Forwards/Backwards, A/D: SideStep, Q/E: Turn
                return {'W', 'S', 'A', 'D', 'Q', 'E', 'Z', 'X', 'C'};
            case KEYSET_IJKL:
                // I/K: Forwards/Backwards, J/L: SideStep, U/O: Turn
                return {'I', 'K', 'J', 'L', 'U', 'O', 'N', 'M', ','};
            case KEYSET_ARROWS:
            default:
                return {Keyboard::UP, Keyboard::DOWN, Keyboard::LEFT, Keyboard::RIGHT, 
                        Keyboard::LEFT | Keyboard::SHIFT, Keyboard::RIGHT | Keyboard::SHIFT,
                        'T', 'W', 'M'};
        }
    }

    std::string get_set_name() const {
        switch (key_set_) {
            case KEYSET_ARROWS: return "ARROWS";
            case KEYSET_WASD: return "WASD";
            case KEYSET_IJKL: return "IJKL";
            case KEYSET_NONE: return "NONE";
            default: return "UNKNOWN";
        }
    }

private:
    KeySet key_set_;

    void autodetect_by_name(Robot& robot) {
        std::string name = robot.getName();
        // Heuristic: use name to assign sets
        // common Webots names: "NAO", "NAO(1)", "NAO(2)", etc.
        if (name == "NAO" || name == "nao") {
            key_set_ = KEYSET_ARROWS;
        } else if (name.find("(1)") != std::string::npos) {
            key_set_ = KEYSET_WASD;
        } else if (name.find("(2)") != std::string::npos) {
            key_set_ = KEYSET_IJKL;
        } else {
            // Default to NONE if not the primary robot to avoid conflicts
            key_set_ = KEYSET_NONE; 
        }
    }
};

} // namespace webots
