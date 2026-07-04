// Copyright NeuralMimicry. All Rights Reserved.

#include "NmSimManager.h"

#include "NmRobotBase.h"
#include "Engine/World.h"
#include "EngineUtils.h"       // TActorIterator
#include "GameFramework/Actor.h"

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

ANmSimManager::ANmSimManager()
{
    PrimaryActorTick.bCanEverTick = true;
    PrimaryActorTick.bStartWithTickEnabled = true;
}

// ---------------------------------------------------------------------------
// Singleton access
// ---------------------------------------------------------------------------

ANmSimManager* ANmSimManager::Get(UWorld* World)
{
    if (!World)
    {
        return nullptr;
    }

    // Return the first (and expected only) instance in the world.
    TActorIterator<ANmSimManager> It(World);
    return It ? *It : nullptr;
}

// ---------------------------------------------------------------------------
// AActor overrides
// ---------------------------------------------------------------------------

void ANmSimManager::BeginPlay()
{
    Super::BeginPlay();

    // Scan the world for any UNmRobotBase components that were placed in the
    // editor.  Components whose owners also call RegisterRobot in their own
    // BeginPlay will be skipped (already present guard in RegisterRobot).
    DiscoverRobots();
}

void ANmSimManager::EndPlay(const EEndPlayReason::Type EndPlayReason)
{
    RegisteredRobots.Empty();
    ConnectedRobotCount = 0;

    Super::EndPlay(EndPlayReason);
}

void ANmSimManager::Tick(float DeltaTime)
{
    Super::Tick(DeltaTime);
    RefreshConnectedCount();
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

void ANmSimManager::RegisterRobot(UNmRobotBase* Robot)
{
    if (!Robot)
    {
        return;
    }

    if (RegisteredRobots.Contains(Robot))
    {
        return; // Already registered.
    }

    // Assign host and compute port: base + sequential index.
    const int32 Index = RegisteredRobots.Num();
    Robot->TcpHost = AarnnHost;
    Robot->TcpPort = AarnnBasePort + Index;

    RegisteredRobots.Add(Robot);

    UE_LOG(LogTemp, Log,
           TEXT("NmSimManager: Registered robot '%s' → %s:%d (index %d)."),
           *Robot->BrainId, *Robot->TcpHost, Robot->TcpPort, Index);

    RefreshConnectedCount();
}

void ANmSimManager::UnregisterRobot(UNmRobotBase* Robot)
{
    if (!Robot)
    {
        return;
    }

    const int32 Removed = RegisteredRobots.Remove(Robot);
    if (Removed > 0)
    {
        UE_LOG(LogTemp, Log,
               TEXT("NmSimManager: Unregistered robot '%s'."), *Robot->BrainId);
    }

    RefreshConnectedCount();
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

void ANmSimManager::DiscoverRobots()
{
    UWorld* World = GetWorld();
    if (!World)
    {
        return;
    }

    int32 Found = 0;
    for (TActorIterator<AActor> ActorIt(World); ActorIt; ++ActorIt)
    {
        AActor* Actor = *ActorIt;
        TArray<UNmRobotBase*> RobotComps;
        Actor->GetComponents<UNmRobotBase>(RobotComps);

        for (UNmRobotBase* RobotComp : RobotComps)
        {
            if (!RegisteredRobots.Contains(RobotComp))
            {
                RegisterRobot(RobotComp);
                ++Found;
            }
        }
    }

    UE_LOG(LogTemp, Log,
           TEXT("NmSimManager: DiscoverRobots found %d new robot component(s). "
                "Total registered: %d."),
           Found, RegisteredRobots.Num());
}

void ANmSimManager::RefreshConnectedCount()
{
    int32 Count = 0;
    for (const UNmRobotBase* Robot : RegisteredRobots)
    {
        if (Robot && Robot->bConnected)
        {
            ++Count;
        }
    }
    ConnectedRobotCount = Count;
}
