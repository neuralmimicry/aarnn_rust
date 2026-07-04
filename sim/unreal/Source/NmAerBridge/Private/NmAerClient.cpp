// Copyright NeuralMimicry. All Rights Reserved.

#include "NmAerClient.h"

#include "Dom/JsonObject.h"
#include "Serialization/JsonSerializer.h"
#include "Serialization/JsonWriter.h"
#include "Sockets.h"
#include "SocketSubsystem.h"
#include "IPAddress.h"
#include "Interfaces/IPv4/IPv4Address.h"
#include "Interfaces/IPv4/IPv4Endpoint.h"
#include "HAL/PlatformTime.h"
#include "HAL/UnrealMemory.h"
#include "Misc/ByteSwap.h"

// ---------------------------------------------------------------------------
// AER1 magic bytes
// ---------------------------------------------------------------------------

static constexpr uint8 kAerMagic[4] = { 'A', 'E', 'R', '1' };

namespace
{
bool WaitForSocketEvent(FSocket* Socket,
                        ESocketWaitConditions::Type Condition,
                        double DeadlineSeconds)
{
    if (!Socket)
    {
        return false;
    }

    while (FPlatformTime::Seconds() < DeadlineSeconds)
    {
        const double Remaining = DeadlineSeconds - FPlatformTime::Seconds();
        if (Remaining <= 0.0)
        {
            return false;
        }

        const double SliceSeconds = FMath::Min(0.02, Remaining);
        if (Socket->Wait(Condition, FTimespan::FromSeconds(SliceSeconds)))
        {
            return true;
        }
    }

    return false;
}
} // namespace

// ---------------------------------------------------------------------------
// Construction / destruction
// ---------------------------------------------------------------------------

FNmAerClient::FNmAerClient()
{
}

FNmAerClient::~FNmAerClient()
{
    Disconnect();
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

bool FNmAerClient::Connect(const FString& Host, int32 Port)
{
    FScopeLock Lock(&SocketMutex);

    if (Socket)
    {
        // Already connected – disconnect first.
        ISocketSubsystem* SS = ISocketSubsystem::Get(PLATFORM_SOCKETSUBSYSTEM);
        SS->DestroySocket(Socket);
        Socket = nullptr;
    }

    ISocketSubsystem* SS = ISocketSubsystem::Get(PLATFORM_SOCKETSUBSYSTEM);
    if (!SS)
    {
        UE_LOG(LogTemp, Error, TEXT("NmAerClient: No socket subsystem available."));
        return false;
    }

    Socket = SS->CreateSocket(NAME_Stream, TEXT("NmAerClient"), false);
    if (!Socket)
    {
        UE_LOG(LogTemp, Error, TEXT("NmAerClient: Failed to create TCP socket."));
        return false;
    }

    // Disable Nagle for lower latency on small control-loop packets.
    Socket->SetNoDelay(true);

    // Resolve address.
    TSharedRef<FInternetAddr> Addr = SS->CreateInternetAddr();
    bool bAddrValid = false;
    Addr->SetIp(*Host, bAddrValid);
    if (!bAddrValid)
    {
        UE_LOG(LogTemp, Error, TEXT("NmAerClient: Invalid host address '%s'."), *Host);
        SS->DestroySocket(Socket);
        Socket = nullptr;
        return false;
    }
    Addr->SetPort(Port);

    if (!Socket->Connect(*Addr))
    {
        UE_LOG(LogTemp, Error,
               TEXT("NmAerClient: Could not connect to %s:%d."), *Host, Port);
        SS->DestroySocket(Socket);
        Socket = nullptr;
        return false;
    }

    // Step send/recv runs from a worker thread; use non-blocking socket mode and
    // explicit bounded waits so a stalled peer cannot park the thread forever.
    Socket->SetNonBlocking(true);
    Socket->SetRecvErr(true);

    UE_LOG(LogTemp, Log,
           TEXT("NmAerClient: Connected to %s:%d."), *Host, Port);
    return true;
}

void FNmAerClient::Disconnect()
{
    FScopeLock Lock(&SocketMutex);

    if (Socket)
    {
        Socket->Close();
        ISocketSubsystem* SS = ISocketSubsystem::Get(PLATFORM_SOCKETSUBSYSTEM);
        if (SS)
        {
            SS->DestroySocket(Socket);
        }
        Socket = nullptr;
        UE_LOG(LogTemp, Log, TEXT("NmAerClient: Disconnected."));
    }
}

bool FNmAerClient::IsConnected() const
{
    FScopeLock Lock(&SocketMutex);
    return Socket != nullptr;
}

// ---------------------------------------------------------------------------
// Protocol – handshake
// ---------------------------------------------------------------------------

bool FNmAerClient::SendHandshake(const TArray<FString>& SensorNames,
                                  const TArray<FString>& ActuatorNames)
{
    // Build JSON: { "s_names": [...], "o_names": [...], "sensory": N, "output": M }
    TSharedRef<FJsonObject> Obj = MakeShared<FJsonObject>();

    TArray<TSharedPtr<FJsonValue>> SArr, OArr;
    for (const FString& Name : SensorNames)
    {
        SArr.Add(MakeShared<FJsonValueString>(Name));
    }
    for (const FString& Name : ActuatorNames)
    {
        OArr.Add(MakeShared<FJsonValueString>(Name));
    }

    Obj->SetArrayField(TEXT("s_names"), SArr);
    Obj->SetArrayField(TEXT("o_names"), OArr);
    Obj->SetNumberField(TEXT("sensory"), SensorNames.Num());
    Obj->SetNumberField(TEXT("output"),  ActuatorNames.Num());

    FString JsonStr;
    TSharedRef<TJsonWriter<>> Writer = TJsonWriterFactory<>::Create(&JsonStr);
    FJsonSerializer::Serialize(Obj, Writer);

    // Convert to UTF-8 bytes.
    FTCHARToUTF8 Utf8(*JsonStr);
    TArray<uint8> Payload;
    Payload.Append(reinterpret_cast<const uint8*>(Utf8.Get()),
                   Utf8.Length());

    if (!SendFrame(Payload))
    {
        UE_LOG(LogTemp, Error, TEXT("NmAerClient: Failed to send handshake."));
        return false;
    }

    // Receive ack (discard body – AARNN echoes or sends a short ack).
    TArray<uint8> AckPayload;
    if (!RecvFrame(AckPayload))
    {
        UE_LOG(LogTemp, Error, TEXT("NmAerClient: No handshake ack received."));
        return false;
    }

    NumSensoryChannels  = SensorNames.Num();
    NumOutputChannels   = ActuatorNames.Num();

    UE_LOG(LogTemp, Log,
           TEXT("NmAerClient: Handshake OK – %d sensors, %d outputs."),
           NumSensoryChannels, NumOutputChannels);
    return true;
}

// ---------------------------------------------------------------------------
// Protocol – step
// ---------------------------------------------------------------------------

bool FNmAerClient::Step(float tMs,
                         const TArray<float>& SensorValues,
                         TArray<float>& OutputValues)
{
    TArray<uint8> SendPayload;
    if (bUseAER)
    {
        EncodeAer(tMs, SensorValues, SendPayload);
    }
    else
    {
        EncodeRaw(tMs, SensorValues, SendPayload);
    }

    if (!SendFrame(SendPayload))
    {
        UE_LOG(LogTemp, Warning, TEXT("NmAerClient: Step send failed."));
        return false;
    }

    TArray<uint8> RecvPayload;
    if (!RecvFrame(RecvPayload))
    {
        UE_LOG(LogTemp, Warning, TEXT("NmAerClient: Step recv failed."));
        return false;
    }

    if (bUseAER)
    {
        return DecodeAer(RecvPayload, NumOutputChannels, OutputValues);
    }
    else
    {
        return DecodeRaw(RecvPayload, NumOutputChannels, OutputValues);
    }
}

// ---------------------------------------------------------------------------
// Framing helpers
// ---------------------------------------------------------------------------

bool FNmAerClient::SendAll(const uint8* Buf, int32 Len)
{
    if (!Socket || Len <= 0)
    {
        return Len <= 0;
    }

    const double TimeoutSeconds = FMath::Max(0.05f, IoTimeoutSeconds);
    double Deadline = FPlatformTime::Seconds() + TimeoutSeconds;
    int32 TotalSent = 0;
    while (TotalSent < Len)
    {
        int32 Sent = 0;
        if (Socket->Send(Buf + TotalSent, Len - TotalSent, Sent) && Sent > 0)
        {
            TotalSent += Sent;
            Deadline = FPlatformTime::Seconds() + TimeoutSeconds;
            continue;
        }

        if (Socket->GetConnectionState() != SCS_Connected)
        {
            return false;
        }

        if (!WaitForSocketEvent(Socket, ESocketWaitConditions::WaitForWrite, Deadline))
        {
            return false;
        }
    }
    return true;
}

bool FNmAerClient::RecvAll(uint8* Buf, int32 Len)
{
    if (!Socket || Len <= 0)
    {
        return Len <= 0;
    }

    const double TimeoutSeconds = FMath::Max(0.05f, IoTimeoutSeconds);
    double Deadline = FPlatformTime::Seconds() + TimeoutSeconds;
    int32 TotalRecv = 0;
    while (TotalRecv < Len)
    {
        int32 Got = 0;
        if (Socket->Recv(Buf + TotalRecv, Len - TotalRecv, Got) && Got > 0)
        {
            TotalRecv += Got;
            Deadline = FPlatformTime::Seconds() + TimeoutSeconds;
            continue;
        }

        if (Socket->GetConnectionState() != SCS_Connected)
        {
            return false;
        }

        if (!WaitForSocketEvent(Socket, ESocketWaitConditions::WaitForRead, Deadline))
        {
            return false;
        }
    }
    return true;
}

bool FNmAerClient::SendFrame(const TArray<uint8>& Payload)
{
    FScopeLock Lock(&SocketMutex);
    if (!Socket)
    {
        return false;
    }

    // Length prefix: u32 LE.
    uint32 PayloadLen  = static_cast<uint32>(Payload.Num());
    uint32 LenLE       = INTEL_ORDER32(PayloadLen);

    if (!SendAll(reinterpret_cast<const uint8*>(&LenLE), sizeof(LenLE)))
    {
        return false;
    }
    if (PayloadLen > 0 && !SendAll(Payload.GetData(), PayloadLen))
    {
        return false;
    }
    return true;
}

bool FNmAerClient::RecvFrame(TArray<uint8>& Payload)
{
    FScopeLock Lock(&SocketMutex);
    if (!Socket)
    {
        return false;
    }

    uint32 LenLE = 0;
    if (!RecvAll(reinterpret_cast<uint8*>(&LenLE), sizeof(LenLE)))
    {
        return false;
    }

    uint32 PayloadLen = INTEL_ORDER32(LenLE);
    Payload.SetNumUninitialized(static_cast<int32>(PayloadLen));

    if (PayloadLen > 0 && !RecvAll(Payload.GetData(), static_cast<int32>(PayloadLen)))
    {
        return false;
    }
    return true;
}

// ---------------------------------------------------------------------------
// Varint (unsigned LEB128)
// ---------------------------------------------------------------------------

void FNmAerClient::WriteVarint(TArray<uint8>& Buf, uint64 Value)
{
    do
    {
        uint8 Byte = static_cast<uint8>(Value & 0x7F);
        Value >>= 7;
        if (Value != 0)
        {
            Byte |= 0x80; // More bytes follow.
        }
        Buf.Add(Byte);
    }
    while (Value != 0);
}

bool FNmAerClient::ReadVarint(const TArray<uint8>& Buf, int32& Pos, uint64& OutValue)
{
    OutValue = 0;
    int32 Shift = 0;
    while (Pos < Buf.Num())
    {
        uint8 Byte = Buf[Pos++];
        OutValue |= static_cast<uint64>(Byte & 0x7F) << Shift;
        Shift += 7;
        if ((Byte & 0x80) == 0)
        {
            return true;
        }
        if (Shift >= 64)
        {
            break; // Overflow guard.
        }
    }
    return false; // Truncated.
}

// ---------------------------------------------------------------------------
// AER codec
// ---------------------------------------------------------------------------

void FNmAerClient::EncodeAer(float tMs,
                              const TArray<float>& SensorValues,
                              TArray<uint8>& OutPayload) const
{
    // Reserve headroom: magic(4) + base_ts(8) + up to N events * ~12 bytes each.
    OutPayload.Reset(4 + 8 + SensorValues.Num() * 12);

    // Magic.
    OutPayload.Append(kAerMagic, 4);

    // base_ts_us: u64 LE.
    uint64 BaseTs = static_cast<uint64>(tMs * 1000.0f); // ms → µs
    uint64 BaseTsLE = INTEL_ORDER64(BaseTs);
    OutPayload.Append(reinterpret_cast<const uint8*>(&BaseTsLE), sizeof(BaseTsLE));

    // Events: for each sensor above threshold emit (delta_ts=0, addr, value).
    // We use delta_ts = 0 for all events within the same step frame.
    // Value is encoded as a u32 bit-cast of the float, then stored as varint.
    for (int32 i = 0; i < SensorValues.Num(); ++i)
    {
        const float Val = SensorValues[i];
        if (Val <= SpikeThreshold)
        {
            continue;
        }

        // delta_ts = 0
        WriteVarint(OutPayload, 0);

        // addr
        uint64 Addr = static_cast<uint64>(SensoryBase + i);
        WriteVarint(OutPayload, Addr);

        // value: scaled to 0..255. The AARNN codec treats any non-zero value as a
        // spike (value & 0xff), so the magnitude is informational only. A bit-cast
        // float must NOT be used here — e.g. 0.5f has a zero low byte and would be
        // read as "no spike".
        uint8 Encoded = static_cast<uint8>(FMath::RoundToInt(FMath::Clamp(Val, 0.0f, 1.0f) * 255.0f));
        if (Encoded == 0)
        {
            Encoded = 1; // guarantee an above-threshold sensor spikes
        }
        WriteVarint(OutPayload, static_cast<uint64>(Encoded));
    }
}

bool FNmAerClient::DecodeAer(const TArray<uint8>& Payload,
                              int32 NumOutputs,
                              TArray<float>& OutValues) const
{
    OutValues.Init(0.0f, NumOutputs);

    if (Payload.Num() < 12) // magic(4) + base_ts(8) minimum
    {
        // Empty response is valid (no motor spikes this step).
        return true;
    }

    // Check magic.
    if (FMemory::Memcmp(Payload.GetData(), kAerMagic, 4) != 0)
    {
        UE_LOG(LogTemp, Warning, TEXT("NmAerClient: AER decode – bad magic."));
        return false;
    }

    // Skip magic + base_ts.
    int32 Pos = 4 + 8;

    while (Pos < Payload.Num())
    {
        uint64 DeltaTs = 0;
        uint64 Addr    = 0;
        uint64 ValBits = 0;

        if (!ReadVarint(Payload, Pos, DeltaTs)) break;
        if (!ReadVarint(Payload, Pos, Addr))    break;
        if (!ReadVarint(Payload, Pos, ValBits)) break;

        // Motor addresses arrive as OutputBase + channel_index. Fall back to a raw
        // address for servers configured with an output base of 0.
        int32 AddrInt  = static_cast<int32>(Addr);
        int32 OutIdx   = (AddrInt >= OutputBase) ? (AddrInt - OutputBase) : AddrInt;
        if (OutIdx >= 0 && OutIdx < NumOutputs)
        {
            // AARNN encodes output spikes with value == 1 (binary); any non-zero low
            // byte is a full spike. A bit-cast to float would yield a denormal ~1e-45.
            OutValues[OutIdx] = ((ValBits & 0xFFu) != 0) ? 1.0f : 0.0f;
        }
        // Addresses outside [OutputBase, OutputBase+M) are ignored.
    }

    return true;
}

// ---------------------------------------------------------------------------
// Raw (f32 array) codec
// ---------------------------------------------------------------------------

void FNmAerClient::EncodeRaw(float tMs,
                              const TArray<float>& SensorValues,
                              TArray<uint8>& OutPayload) const
{
    // Layout: [f32 t_ms][f32 s_0]...[f32 s_N]  – all native-endian floats.
    const int32 Count = 1 + SensorValues.Num();
    OutPayload.SetNumUninitialized(Count * sizeof(float));

    float* Ptr = reinterpret_cast<float*>(OutPayload.GetData());
    Ptr[0] = tMs;
    for (int32 i = 0; i < SensorValues.Num(); ++i)
    {
        Ptr[1 + i] = SensorValues[i];
    }
}

bool FNmAerClient::DecodeRaw(const TArray<uint8>& Payload,
                              int32 NumOutputs,
                              TArray<float>& OutValues) const
{
    OutValues.Init(0.0f, NumOutputs);

    const int32 ExpectedBytes = NumOutputs * static_cast<int32>(sizeof(float));
    if (Payload.Num() < ExpectedBytes)
    {
        UE_LOG(LogTemp, Warning,
               TEXT("NmAerClient: Raw decode – short payload (%d < %d)."),
               Payload.Num(), ExpectedBytes);
        return false;
    }

    const float* Ptr = reinterpret_cast<const float*>(Payload.GetData());
    for (int32 i = 0; i < NumOutputs; ++i)
    {
        OutValues[i] = Ptr[i];
    }
    return true;
}
