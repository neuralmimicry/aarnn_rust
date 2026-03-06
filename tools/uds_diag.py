import argparse
import socket
import struct
import sys
import os
import json
import time

MAGIC = b"AER1"

def write_varint(value):
    out = bytearray()
    while value >= 0x80:
        out.append((value & 0x7F) | 0x80)
        value >>= 7
    out.append(value & 0xFF)
    return bytes(out)

def encode_events(events):
    if not events:
        return b""
    events = sorted(events, key=lambda e: e[0])
    base_ts = events[0][0]
    out = bytearray()
    out += MAGIC
    out += int(base_ts).to_bytes(8, "little", signed=False)
    prev_ts = base_ts
    for ts_us, addr, value in events:
        delta = max(0, int(ts_us) - int(prev_ts))
        prev_ts = int(ts_us)
        out += write_varint(delta)
        out += write_varint(int(addr))
        out += write_varint(int(value))
    return bytes(out)

def read_varint(data, offset):
    result = 0
    shift = 0
    i = offset
    while i < len(data):
        b = data[i]
        result |= (b & 0x7F) << shift
        i += 1
        if (b & 0x80) == 0:
            return result, i
        shift += 7
        if shift >= 64:
            raise ValueError("varint overflow")
    raise ValueError("truncated varint")

def decode_events(payload):
    if len(payload) < 12 or payload[:4] != MAGIC:
        raise ValueError("invalid AER payload")
    base_ts = int.from_bytes(payload[4:12], "little", signed=False)
    idx = 12
    prev_ts = base_ts
    events = []
    while idx < len(payload):
        delta, idx = read_varint(payload, idx)
        addr, idx = read_varint(payload, idx)
        value, idx = read_varint(payload, idx)
        prev_ts += delta
        events.append((prev_ts, addr, value & 0xFF))
    return events

def main():
    argument_parser = argparse.ArgumentParser(description="UDS Diagnostic Tool")
    argument_parser.add_argument("--socket", type=str, help="Socket path")
    argument_parser.add_argument("--brain-id", type=str, default="default", help="Brain ID for default socket path naming")
    argument_parser.add_argument("--sensory", type=int, help="Number of sensory neurons (S)")
    argument_parser.add_argument("--output", type=int, help="Number of output neurons (O)")
    argument_parser.add_argument("--handshake", action="store_true", help="Send JSON handshake instead of data")
    argument_parser.add_argument("--format", choices=["aer", "float"], default="aer", help="Payload format (default: aer)")
    argument_parser.add_argument("--aer-sensory-base", type=int, default=4096, help="AER sensory base address")
    argument_parser.add_argument("--aer-output-base", type=int, default=16384, help="AER output base address")
    argument_parser.add_argument("--threshold", type=float, default=0.5, help="Float->spike threshold (AER mode)")
    args = argument_parser.parse_args()

    home_directory = os.environ.get("HOME")
    if args.socket:
        socket_path = args.socket
    elif home_directory:
        if args.brain_id == "default":
            socket_path = os.path.join(home_directory, "aarnn_rust.nn")
        else:
            socket_path = os.path.join(home_directory, f"aarnn_rust.{args.brain_id}.nn")
    else:
        if args.brain_id == "default":
            socket_path = "/tmp/aarnn_rust.nn"
        else:
            socket_path = f"/tmp/aarnn_rust.{args.brain_id}.nn"
    
    # Use a unique client path including PID to avoid conflicts and stale files
    client_socket_path = socket_path + f".diag_{os.getpid()}.sock"
    
    if os.path.exists(client_socket_path):
        os.remove(client_socket_path)

    uds_socket = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    try:
        uds_socket.bind(client_socket_path)
    except Exception as e:
        print(f"Error binding to {client_socket_path}: {e}")
        sys.exit(1)

    # Auto-detect if needed
    num_sensory_neurons = args.sensory
    num_output_neurons = args.output
    
    if num_sensory_neurons is None or num_output_neurons is None:
        print(f"Auto-detecting sizes from {socket_path}...")
        # Send a small probe (4-byte float)
        probe_packet = struct.pack("<f", 0.0)
        
        is_detected = False
        for attempt in range(5): # More retries
            try:
                if attempt > 0:
                    print(f"Retry {attempt}...")
                uds_socket.sendto(probe_packet, socket_path)
                uds_socket.settimeout(3.0) # Larger timeout for detection
                received_data, address = uds_socket.recvfrom(4096)
                
                # Check if it's a hint
                try:
                    hint_data = json.loads(received_data.decode("utf-8"))
                    if "expected_s" in hint_data and "expected_o" in hint_data:
                        if num_sensory_neurons is None: num_sensory_neurons = hint_data["expected_s"]
                        if num_output_neurons is None: num_output_neurons = hint_data["expected_o"]
                        print(f"Auto-detected S={num_sensory_neurons}, O={num_output_neurons}")
                        is_detected = True
                        break
                    else:
                        print(f"Received unknown JSON: {hint_data}")
                except json.JSONDecodeError:
                    print(f"Received non-JSON reply of {len(received_data)} bytes from {address}")
                    # If it's binary data, maybe we can guess sizes? No, let's just keep trying.
                    continue
            except socket.timeout:
                continue
            except Exception as e:
                print(f"Auto-detection attempt {attempt} failed: {e}")
                
        if not is_detected:
            print(f"Auto-detection failed after 5 attempts")

    if num_sensory_neurons is None: num_sensory_neurons = 25
    if num_output_neurons is None: num_output_neurons = 11
    print(f"Final configuration S={num_sensory_neurons}, O={num_output_neurons}")

    print(f"Connecting to {socket_path} from {client_socket_path}")
    
    # Option: send handshake
    if args.handshake:
        print("Sending handshake...")
        sensory_names = [f"S{i}" for i in range(num_sensory_neurons)]
        output_names = [f"O{i}" for i in range(num_output_neurons)]
        handshake_data = {"s_names": sensory_names, "o_names": output_names, "format": args.format}
        request_data = json.dumps(handshake_data).encode("utf-8")
    else:
        # Request data (AER by default, float legacy if --format=float)
        if args.format == "aer":
            input_values = [0.1 * (i % 10) for i in range(num_sensory_neurons)]
            if num_sensory_neurons > 0:
                input_values[num_sensory_neurons-1] = 0.7
            spikes = [1 if v >= args.threshold else 0 for v in input_values]
            ts_us = int(time.time() * 1_000_000)
            events = [(ts_us, args.aer_sensory_base + i, 1) for i, spk in enumerate(spikes) if spk]
            request_data = encode_events(events)
            if not request_data:
                request_data = MAGIC + ts_us.to_bytes(8, "little", signed=False)
        else:
            # Float legacy: [f32 simulation_time_ms] + [S f32]
            simulation_time_ms = 10.0
            input_values = [0.1 * (i % 10) for i in range(num_sensory_neurons)]
            if num_sensory_neurons > 0:
                input_values[num_sensory_neurons-1] = 0.7
            request_data = struct.pack(f"<f{num_sensory_neurons}f", simulation_time_ms, *input_values)
    
    try:
        print(f"Sending {len(request_data)} bytes...")
        uds_socket.sendto(request_data, socket_path)
        
        uds_socket.settimeout(2.0)
        print("Waiting for reply...")
        received_data, address = uds_socket.recvfrom(4096)
        print(f"Received {len(received_data)} bytes from {address}")
        
        if received_data.startswith(MAGIC):
            events = decode_events(received_data)
            spikes = [0] * num_output_neurons
            for (_ts, addr, value) in events:
                if value == 0:
                    continue
                idx = addr - args.aer_output_base if addr >= args.aer_output_base else addr
                if 0 <= idx < num_output_neurons:
                    spikes[int(idx)] = 1
            print(f"Output spikes (AER): {spikes}")
        elif len(received_data) == num_output_neurons * 4:
            output_values = struct.unpack(f"<{num_output_neurons}f", received_data)
            print(f"Outputs: {output_values}")
        else:
            print(f"Unexpected reply size: {len(received_data)} (expected {num_output_neurons*4} or AER payload)")
            
    except Exception as e:
        print(f"Error: {e}")
    finally:
        uds_socket.close()
        if os.path.exists(client_socket_path):
            os.remove(client_socket_path)

if __name__ == "__main__":
    main()
