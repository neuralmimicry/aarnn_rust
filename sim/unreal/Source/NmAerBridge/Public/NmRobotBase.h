// Copyright NeuralMimicry. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "Components/ActorComponent.h"
#include "NmRobotBase.generated.h"

class FNmRobotIoWorker;

/**
 * UNmRobotBase
 *
 * Abstract UActorComponent that couples an Unreal actor to a remote AARNN brain
 * via the AER TCP protocol.
 *
 * Subclasses must implement:
 *   CollectSensors    – fill sensor values from the actor's environment
 *   ApplyActuators    – drive joints/movement from AARNN motor outputs
 *   GetSensorNames    – return ordered sensor channel names for handshake
 *   GetActuatorNames  – return ordered actuator channel names for handshake
 *
 * The component:
 *   1. Starts a background I/O worker that connects to TcpHost:TcpPort and handshakes.
 *   2. On every Tick (game thread): CollectSensors / ApplyActuators only.
 *   3. Runs TCP send/recv on the worker thread so gameplay never blocks on sockets.
 *   4. Exposes bConnected as a BlueprintReadOnly property for UI / debug.
 */
UCLASS(Abstract, ClassGroup = "NeuralMimicry", meta = (BlueprintSpawnableComponent))
class NMAERBRIDGE_API UNmRobotBase : public UActorComponent
{
    GENERATED_BODY()

public:
    UNmRobotBase();

    // -------------------------------------------------------------------------
    // Configuration (editable per-instance in the Details panel)
    // -------------------------------------------------------------------------

    /** Brain identifier, e.g. "celegans", "hexapod", "zebrafish". */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmAerBridge")
    FString BrainId;

    /** Hostname or IP of the AARNN TCP server. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmAerBridge")
    FString TcpHost = TEXT("127.0.0.1");

    /** TCP port to connect to. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmAerBridge")
    int32 TcpPort = 7890;

    /** Values above this threshold are encoded as AER spike events. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmAerBridge",
              meta = (ClampMin = "0.0", ClampMax = "1.0"))
    float SpikeThreshold = 0.5f;

    /** Use AER1 framing (true) or raw f32 arrays (false). */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmAerBridge")
    bool bUseAER = true;

    // -------------------------------------------------------------------------
    // Runtime state (read-only in Blueprint)
    // -------------------------------------------------------------------------

    /** True when the TCP connection to AARNN is live. */
    UPROPERTY(BlueprintReadOnly, Category = "NmAerBridge")
    bool bConnected = false;

    // -------------------------------------------------------------------------
    // UActorComponent interface
    // -------------------------------------------------------------------------

    virtual void BeginPlay() override;
    virtual void EndPlay(const EEndPlayReason::Type EndPlayReason) override;
    virtual void TickComponent(float DeltaTime,
                               ELevelTick TickType,
                               FActorComponentTickFunction* ThisTickFunction) override;

    // -------------------------------------------------------------------------
    // Pure-virtual robot interface (implement per-robot subclass)
    // -------------------------------------------------------------------------

    /**
     * Fill OutSensors with the current sensory reading.
     * Array size must match GetSensorNames() count.
     */
    virtual void CollectSensors(TArray<float>& OutSensors) PURE_VIRTUAL(
        UNmRobotBase::CollectSensors, );

    /**
     * Consume motor outputs produced by AARNN.
     * Actuators.Num() == GetActuatorNames() count.
     */
    virtual void ApplyActuators(const TArray<float>& Actuators) PURE_VIRTUAL(
        UNmRobotBase::ApplyActuators, );

    /** Return the ordered list of sensor channel names used in the handshake. */
    virtual void GetSensorNames(TArray<FString>& OutNames) const PURE_VIRTUAL(
        UNmRobotBase::GetSensorNames, );

    /** Return the ordered list of actuator channel names used in the handshake. */
    virtual void GetActuatorNames(TArray<FString>& OutNames) const PURE_VIRTUAL(
        UNmRobotBase::GetActuatorNames, );

protected:
    /** Start the background I/O worker if needed. Safe to call repeatedly. */
    bool TryConnect();

    /** Background worker that owns the TCP client and AER request/response loop. */
    FNmRobotIoWorker* IoWorker = nullptr;

    /** Accumulated simulation time in milliseconds, incremented each tick. */
    float SimTimeMs = 0.0f;

    /** Seconds accumulated since the last (re)connect attempt while disconnected. */
    float ReconnectAccum = 0.0f;

    /** Last actuator vector received from the async worker. */
    TArray<float> LastActuatorOutputs;

    /** True once at least one actuator vector has been received this session. */
    bool bHasLastActuatorOutputs = false;
};
