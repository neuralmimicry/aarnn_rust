// Copyright NeuralMimicry. All Rights Reserved.

#include "NmRobotBase.h"

#include "NmAerClient.h"
#include "NmSimManager.h"
#include "GameFramework/Actor.h"
#include "HAL/PlatformProcess.h"
#include "HAL/PlatformTime.h"
#include "HAL/Runnable.h"
#include "HAL/RunnableThread.h"
#include "Misc/ScopeLock.h"

namespace
{
bool ValidateChannelNames(const TArray<FString>& Names, const TCHAR* Kind, const FString& BrainId)
{
    bool bOk = true;
    TSet<FString> Seen;
    for (int32 i = 0; i < Names.Num(); ++i)
    {
        const FString& Name = Names[i];
        if (Name.IsEmpty())
        {
            UE_LOG(LogTemp, Error,
                   TEXT("NmRobotBase [%s]: %s channel %d has an empty name."),
                   *BrainId, Kind, i);
            bOk = false;
            continue;
        }
        if (Seen.Contains(Name))
        {
            UE_LOG(LogTemp, Warning,
                   TEXT("NmRobotBase [%s]: duplicate %s channel name '%s' at index %d."),
                   *BrainId, Kind, *Name, i);
            bOk = false;
        }
        Seen.Add(Name);
    }
    return bOk;
}

const TCHAR* FirstOrDash(const TArray<FString>& Names)
{
    return Names.Num() > 0 ? *Names[0] : TEXT("-");
}

const TCHAR* LastOrDash(const TArray<FString>& Names)
{
    return Names.Num() > 0 ? *Names.Last() : TEXT("-");
}
} // namespace

class FNmRobotIoWorker final : public FRunnable
{
public:
    FNmRobotIoWorker(const FString& InBrainId,
                     const FString& InHost,
                     int32 InPort,
                     float InSpikeThreshold,
                     bool bInUseAER,
                     const TArray<FString>& InSensorNames,
                     const TArray<FString>& InActuatorNames)
        : BrainId(InBrainId)
        , Host(InHost)
        , Port(InPort)
        , SensorNames(InSensorNames)
        , ActuatorNames(InActuatorNames)
    {
        Client.SpikeThreshold = InSpikeThreshold;
        Client.bUseAER = bInUseAER;
    }

    ~FNmRobotIoWorker() override
    {
        StopAndJoin();
    }

    bool Start()
    {
        if (Thread)
        {
            return true;
        }

        const FString SafeBrainId = BrainId.IsEmpty() ? TEXT("robot") : BrainId;
        const FString ThreadName = FString::Printf(TEXT("NmAerIo_%s_%d"),
                                                   *SafeBrainId, Port);
        Thread = FRunnableThread::Create(this, *ThreadName, 0, TPri_Normal);
        return Thread != nullptr;
    }

    void StopAndJoin()
    {
        Stop();
        if (Thread)
        {
            Thread->WaitForCompletion();
            delete Thread;
            Thread = nullptr;
        }
    }

    void SubmitStep(float TimeMs, TArray<float>&& Sensors)
    {
        FScopeLock Lock(&DataMutex);
        PendingTimeMs = TimeMs;
        PendingSensors = MoveTemp(Sensors);
        bHasPendingStep = true;
    }

    bool ConsumeLatestOutputs(TArray<float>& OutOutputs)
    {
        FScopeLock Lock(&DataMutex);
        if (!bHasFreshOutputs)
        {
            return false;
        }
        OutOutputs = MoveTemp(LatestOutputs);
        LatestOutputs.Reset();
        bHasFreshOutputs = false;
        return true;
    }

    bool IsConnected() const
    {
        return bConnected.Load();
    }

    virtual void Stop() override
    {
        bStopRequested.Store(true);
    }

    virtual uint32 Run() override
    {
        constexpr float RetrySleepSeconds = 0.01f;
        constexpr float IdleSleepSeconds = 0.001f;
        double NextConnectAt = 0.0;

        while (!bStopRequested.Load())
        {
            if (!bConnected.Load())
            {
                const double Now = FPlatformTime::Seconds();
                if (Now >= NextConnectAt)
                {
                    if (!ConnectAndHandshake())
                    {
                        NextConnectAt = Now + 1.0;
                    }
                }
                FPlatformProcess::SleepNoStats(RetrySleepSeconds);
                continue;
            }

            float LocalTimeMs = 0.0f;
            TArray<float> LocalSensors;
            bool bHaveStep = false;
            {
                FScopeLock Lock(&DataMutex);
                if (bHasPendingStep)
                {
                    LocalTimeMs = PendingTimeMs;
                    LocalSensors = MoveTemp(PendingSensors);
                    PendingSensors.Reset();
                    bHasPendingStep = false;
                    bHaveStep = true;
                }
            }

            if (!bHaveStep)
            {
                FPlatformProcess::SleepNoStats(IdleSleepSeconds);
                continue;
            }

            TArray<float> LocalOutputs;
            if (!Client.Step(LocalTimeMs, LocalSensors, LocalOutputs))
            {
                HandleDisconnect(TEXT("step failed"));
                NextConnectAt = FPlatformTime::Seconds() + 1.0;
                continue;
            }

            {
                FScopeLock Lock(&DataMutex);
                LatestOutputs = MoveTemp(LocalOutputs);
                bHasFreshOutputs = true;
            }
        }

        HandleDisconnect(TEXT("worker stop"));
        return 0;
    }

private:
    bool ConnectAndHandshake()
    {
        if (!Client.Connect(Host, Port))
        {
            bConnected.Store(false);
            return false;
        }
        if (!Client.SendHandshake(SensorNames, ActuatorNames))
        {
            Client.Disconnect();
            bConnected.Store(false);
            return false;
        }

        bConnected.Store(true);
        UE_LOG(LogTemp, Log,
               TEXT("NmRobotIoWorker [%s]: connected to %s:%d."),
               *BrainId, *Host, Port);
        return true;
    }

    void HandleDisconnect(const TCHAR* Reason)
    {
        const bool bWasConnected = bConnected.Load();
        bConnected.Store(false);
        Client.Disconnect();

        {
            FScopeLock Lock(&DataMutex);
            bHasFreshOutputs = false;
            LatestOutputs.Reset();
        }

        if (bWasConnected)
        {
            UE_LOG(LogTemp, Warning,
                   TEXT("NmRobotIoWorker [%s]: disconnected (%s)."),
                   *BrainId, Reason ? Reason : TEXT("unknown"));
        }
    }

    FString BrainId;
    FString Host;
    int32 Port = 7890;

    TArray<FString> SensorNames;
    TArray<FString> ActuatorNames;

    FNmAerClient Client;

    FRunnableThread* Thread = nullptr;
    TAtomic<bool> bStopRequested = false;
    TAtomic<bool> bConnected = false;

    mutable FCriticalSection DataMutex;
    float PendingTimeMs = 0.0f;
    TArray<float> PendingSensors;
    bool bHasPendingStep = false;

    TArray<float> LatestOutputs;
    bool bHasFreshOutputs = false;
};

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

UNmRobotBase::UNmRobotBase()
{
    PrimaryComponentTick.bCanEverTick = true;
    PrimaryComponentTick.bStartWithTickEnabled = true;
}

// ---------------------------------------------------------------------------
// UActorComponent overrides
// ---------------------------------------------------------------------------

void UNmRobotBase::BeginPlay()
{
    Super::BeginPlay();

    // Let the sim manager assign TcpHost / TcpPort before we try to connect,
    // so that auto-registration has already happened if the manager is also in
    // BeginPlay on this frame.
    if (ANmSimManager* Mgr = ANmSimManager::Get(GetWorld()))
    {
        Mgr->RegisterRobot(this);
        // RegisterRobot() may have updated TcpHost and TcpPort – connect after.
    }

    // Start background I/O now; if thread startup fails, TickComponent retries.
    if (!TryConnect())
    {
        UE_LOG(LogTemp, Warning,
               TEXT("NmRobotBase [%s]: failed to start async I/O worker for %s:%d – will keep retrying."),
               *BrainId, *TcpHost, TcpPort);
    }
}

bool UNmRobotBase::TryConnect()
{
    if (IoWorker)
    {
        return true;
    }

    TArray<FString> SNames;
    TArray<FString> ANames;
    GetSensorNames(SNames);
    GetActuatorNames(ANames);

    const bool bSensorNamesOk = ValidateChannelNames(SNames, TEXT("sensor"), BrainId);
    const bool bActuatorNamesOk = ValidateChannelNames(ANames, TEXT("actuator"), BrainId);
    UE_LOG(LogTemp, Log,
           TEXT("NmRobotBase [%s]: channel-map sensors=%d actuators=%d s0='%s' sN='%s' o0='%s' oN='%s' valid=%s"),
           *BrainId, SNames.Num(), ANames.Num(),
           FirstOrDash(SNames), LastOrDash(SNames),
           FirstOrDash(ANames), LastOrDash(ANames),
           (bSensorNamesOk && bActuatorNamesOk) ? TEXT("yes") : TEXT("check-warnings"));

    IoWorker = new FNmRobotIoWorker(
        BrainId, TcpHost, TcpPort, SpikeThreshold, bUseAER, SNames, ANames);
    if (!IoWorker->Start())
    {
        delete IoWorker;
        IoWorker = nullptr;
        bConnected = false;
        return false;
    }

    bConnected = false;
    UE_LOG(LogTemp, Log,
           TEXT("NmRobotBase [%s]: started async I/O worker for %s:%d."),
           *BrainId, *TcpHost, TcpPort);
    return true;
}

void UNmRobotBase::EndPlay(const EEndPlayReason::Type EndPlayReason)
{
    if (IoWorker)
    {
        IoWorker->StopAndJoin();
        delete IoWorker;
        IoWorker = nullptr;
    }
    bConnected = false;
    bHasLastActuatorOutputs = false;
    LastActuatorOutputs.Reset();

    if (ANmSimManager* Mgr = ANmSimManager::Get(GetWorld()))
    {
        Mgr->UnregisterRobot(this);
    }

    Super::EndPlay(EndPlayReason);
}

void UNmRobotBase::TickComponent(float DeltaTime,
                                  ELevelTick TickType,
                                  FActorComponentTickFunction* ThisTickFunction)
{
    Super::TickComponent(DeltaTime, TickType, ThisTickFunction);

    if (!IoWorker)
    {
        // Retry worker startup roughly once a second until it succeeds.
        ReconnectAccum += DeltaTime;
        if (ReconnectAccum >= 1.0f)
        {
            ReconnectAccum = 0.0f;
            TryConnect();
        }
        return;
    }

    bConnected = IoWorker->IsConnected();
    if (!bConnected)
    {
        bHasLastActuatorOutputs = false;
        LastActuatorOutputs.Reset();
        return;
    }

    // Pull the newest completed brain outputs, if any.
    TArray<float> Outputs;
    if (IoWorker->ConsumeLatestOutputs(Outputs))
    {
        LastActuatorOutputs = MoveTemp(Outputs);
        bHasLastActuatorOutputs = true;
    }

    // Re-apply the latest actuator vector every frame while connected. This keeps
    // actuation continuous even if network responses arrive slower than Tick.
    if (bHasLastActuatorOutputs)
    {
        ApplyActuators(LastActuatorOutputs);
    }

    SimTimeMs += DeltaTime * 1000.0f;

    // Collect sensor readings from the concrete subclass and queue for async step.
    TArray<float> Sensors;
    CollectSensors(Sensors);
    IoWorker->SubmitStep(SimTimeMs, MoveTemp(Sensors));
}
