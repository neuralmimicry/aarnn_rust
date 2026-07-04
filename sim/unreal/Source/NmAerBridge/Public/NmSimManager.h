// Copyright NeuralMimicry. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "NmSimManager.generated.h"

class UNmRobotBase;

/**
 * ANmSimManager
 *
 * Singleton AActor that acts as the per-level registry for all UNmRobotBase
 * components.
 *
 * Drop one instance into the level.  On BeginPlay it scans every actor in the
 * world for UNmRobotBase components and registers them.  Port assignment:
 *
 *   robot_port = AarnnBasePort + brain_index
 *
 * where brain_index is the sequential index of the robot as it was spawned /
 * registered (0-based).  This mirrors the AARNN multi-brain TCP port scheme.
 *
 * RobotSpec (optional editor array) is purely informational for this level of
 * the stack – it does NOT auto-spawn actors.  Populate it for documentation,
 * or extend BeginPlay in a subclass to do spec-driven spawning.
 *
 * Usage:
 *   ANmSimManager::Get(GetWorld())  – returns the singleton or nullptr.
 */
UCLASS(ClassGroup = "NeuralMimicry", meta = (BlueprintSpawnableComponent))
class NMAERBRIDGE_API ANmSimManager : public AActor
{
    GENERATED_BODY()

public:
    ANmSimManager();

    // -------------------------------------------------------------------------
    // Configuration
    // -------------------------------------------------------------------------

    /**
     * Informational robot specification, e.g. { "celegans=1", "hexapod=2" }.
     * Not used directly by this class but available to Blueprint / subclasses.
     */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmAerBridge")
    TArray<FString> RobotSpec;

    /** Hostname or IP of the AARNN server(s). Pushed into each robot on register. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmAerBridge")
    FString AarnnHost = TEXT("127.0.0.1");

    /**
     * Base TCP port.  Robot i connects to AarnnBasePort + i.
     * The per-robot TcpPort is overwritten by RegisterRobot().
     */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmAerBridge")
    int32 AarnnBasePort = 7890;

    // -------------------------------------------------------------------------
    // Runtime state
    // -------------------------------------------------------------------------

    /** Number of robots currently holding a live TCP connection. */
    UPROPERTY(BlueprintReadOnly, Category = "NmAerBridge")
    int32 ConnectedRobotCount = 0;

    // -------------------------------------------------------------------------
    // Registry
    // -------------------------------------------------------------------------

    /**
     * Register a robot component with the manager.
     * Assigns TcpHost and TcpPort before BeginPlay fires on the component,
     * so call this before the component's BeginPlay if possible.
     */
    UFUNCTION(BlueprintCallable, Category = "NmAerBridge")
    void RegisterRobot(UNmRobotBase* Robot);

    /** Remove a robot from the registry (called by EndPlay on the component). */
    UFUNCTION(BlueprintCallable, Category = "NmAerBridge")
    void UnregisterRobot(UNmRobotBase* Robot);

    /**
     * Return the singleton ANmSimManager for the given world, or nullptr if
     * none has been placed / spawned.
     */
    UFUNCTION(BlueprintCallable, Category = "NmAerBridge",
              meta = (WorldContext = "World"))
    static ANmSimManager* Get(UWorld* World);

    // -------------------------------------------------------------------------
    // AActor interface
    // -------------------------------------------------------------------------

    virtual void BeginPlay() override;
    virtual void EndPlay(const EEndPlayReason::Type EndPlayReason) override;
    virtual void Tick(float DeltaTime) override;

protected:
    /** All robots currently registered with this manager. */
    UPROPERTY()
    TArray<UNmRobotBase*> RegisteredRobots;

private:
    /** Scan the world and auto-register every UNmRobotBase found. */
    void DiscoverRobots();

    /** Refresh ConnectedRobotCount from RegisteredRobots. */
    void RefreshConnectedCount();
};
