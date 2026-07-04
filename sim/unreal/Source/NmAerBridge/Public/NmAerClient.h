// Copyright NeuralMimicry. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "HAL/CriticalSection.h"
#include "Containers/Array.h"
#include "Containers/UnrealString.h"

class FSocket;

/**
 * FNmAerClient
 *
 * TCP client that speaks the AARNN AER wire protocol with bounded I/O waits.
 *
 * Wire format (send and receive):
 *   [u32 LE length][payload]
 *
 * AER payload layout:
 *   magic  : 4 bytes  "AER1"
 *   base_ts: 8 bytes  u64 LE microseconds
 *   events : varint-pairs (delta_ts_us, addr, value) – see EncodeAer / DecodeAer
 *
 * Handshake (once, after TCP connect):
 *   Send: JSON text  { "s_names": [...], "o_names": [...], "sensory": N, "output": M }
 *   Recv: same JSON echoed back or an ack JSON – we discard the body.
 *
 * Sensor encoding:
 *   Each sensor value above SpikeThreshold is emitted as one AER1 event at address
 *   (SensoryBase + sensor_index).  Values below threshold produce no event.
 *
 * Output decoding:
 *   Each received AER1 event with address in [OutputBase, OutputBase+M) sets the
 *   corresponding output slot to 1.0 (AARNN emits binary spikes with value == 1).
 *   Slots with no event remain 0.0.  Addresses outside that window are ignored.
 *
 * Raw (non-AER) mode (bUseAER == false):
 *   Send: [f32 t_ms][f32 s_0]...[f32 s_N]
 *   Recv: [f32 o_0]...[f32 o_M]
 */
class NMAERBRIDGE_API FNmAerClient
{
public:
    FNmAerClient();
    ~FNmAerClient();

    // Connection ---------------------------------------------------------------

    /** Open a TCP connection to Host:Port.  Returns false on failure. */
    bool Connect(const FString& Host, int32 Port);

    /** Close the socket cleanly. */
    void Disconnect();

    /** True if the socket is open and connected. */
    bool IsConnected() const;

    // Protocol -----------------------------------------------------------------

    /**
     * Send the JSON handshake.  Must be called once after Connect() and before
     * the first Step().
     *
     * @param SensorNames   Ordered list of sensory channel names.
     * @param ActuatorNames Ordered list of output / actuator channel names.
     * @return false if the send or ack receive failed.
     */
    bool SendHandshake(const TArray<FString>& SensorNames,
                       const TArray<FString>& ActuatorNames);

    /**
     * One sense→think→act round-trip.
     *
     * @param tMs           Current simulation time in milliseconds.
     * @param SensorValues  Per-sensor float values (size == sensory count).
     * @param OutputValues  Filled with per-actuator floats on success.
     * @return false if the send or receive failed.
     */
    bool Step(float tMs,
              const TArray<float>& SensorValues,
              TArray<float>& OutputValues);

    // Config -------------------------------------------------------------------

    /** Values strictly above this threshold generate a spike event. */
    float SpikeThreshold = 0.5f;

    /** AER address base for sensory channels (AARNN default: 4096). */
    int32 SensoryBase = 4096;

    /** AER address base for motor/output channels (AARNN default: 16384). */
    int32 OutputBase = 16384;

    /** When true use AER1 framing; when false use raw f32 arrays. */
    bool bUseAER = true;

    /** Maximum time allowed per framed send/recv before reporting failure. */
    float IoTimeoutSeconds = 120.0f;

private:
    // --- Framing helpers ------------------------------------------------------

    /** Send exactly Len bytes from Buf.  Returns false on partial/error. */
    bool SendAll(const uint8* Buf, int32 Len);

    /** Receive exactly Len bytes into Buf.  Returns false on partial/error. */
    bool RecvAll(uint8* Buf, int32 Len);

    /** Send a length-prefixed (u32 LE) message. */
    bool SendFrame(const TArray<uint8>& Payload);

    /** Receive a length-prefixed message into Payload. */
    bool RecvFrame(TArray<uint8>& Payload);

    // --- AER codec ------------------------------------------------------------

    /**
     * Encode sensor values into an AER1 payload.
     *
     * @param tMs           Timestamp in milliseconds (converted to microseconds).
     * @param SensorValues  Per-channel values.
     * @param OutPayload    Filled with the serialised AER1 bytes.
     */
    void EncodeAer(float tMs,
                   const TArray<float>& SensorValues,
                   TArray<uint8>& OutPayload) const;

    /**
     * Decode an AER1 payload into per-channel output values.
     *
     * @param Payload       Raw bytes received from AARNN.
     * @param NumOutputs    Expected number of output channels.
     * @param OutValues     Zeroed then filled at indices matching received addresses.
     * @return false if the payload magic is invalid.
     */
    bool DecodeAer(const TArray<uint8>& Payload,
                   int32 NumOutputs,
                   TArray<float>& OutValues) const;

    // --- Varint helpers (LEB128 unsigned) -------------------------------------

    /** Append a u64 as unsigned LEB128 to Buf. */
    static void WriteVarint(TArray<uint8>& Buf, uint64 Value);

    /**
     * Read a u64 unsigned LEB128 from Buf starting at Pos.
     * Advances Pos past the consumed bytes.  Returns false on truncation.
     */
    static bool ReadVarint(const TArray<uint8>& Buf, int32& Pos, uint64& OutValue);

    // --- Raw codec ------------------------------------------------------------

    void EncodeRaw(float tMs,
                   const TArray<float>& SensorValues,
                   TArray<uint8>& OutPayload) const;

    bool DecodeRaw(const TArray<uint8>& Payload,
                   int32 NumOutputs,
                   TArray<float>& OutValues) const;

    // --- State ----------------------------------------------------------------

    FSocket* Socket = nullptr;

    /** Number of output channels, learned from SendHandshake(). */
    int32 NumOutputChannels = 0;

    /** Number of sensory channels, learned from SendHandshake(). */
    int32 NumSensoryChannels = 0;

    mutable FCriticalSection SocketMutex;
};
