// Copyright NeuralMimicry. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/GameModeBase.h"
#include "NmSimGameMode.generated.h"

/** The kind of habitat spawned around a group of robots of one type. */
enum class ENmHabitat : uint8
{
    Dish,          // C. elegans — shallow agar plate with a low rim + food spots
    Terrain,       // hexapod — uneven scattered blocks to traverse
    Room,          // NAO — flat floor room with low walls + a step
    FlightArena,   // Drosophila — walled arena with tall poles to fly around
    Tank,          // zebrafish — glass aquarium box with a water surface
};

/**
 * ANmSimGameMode
 *
 * Auto-populates the level with AARNN robots at BeginPlay so the simulation can be
 * launched entirely from the command line (see scripts/run_sim.sh --sim unreal).
 *
 * Population and connection parameters come from environment variables exported by
 * run_sim.sh:
 *
 *   NM_UE_ROBOTS         Robot spec, e.g. "celegans=1,hexapod=2,nao=1"
 *                        (same syntax and alias set as run_sim.sh --robots).
 *   NM_AARNN_HOST        AARNN TCP host          (default "127.0.0.1").
 *   NM_AARNN_BASE_PORT   First TCP port          (default 7890).
 *
 * Robots are grouped by type; each type gets its own habitat region (dish, terrain,
 * room, flight arena, or fish tank), laid out in a row so mixed populations each get
 * an appropriate environment. Ports are assigned in spec order (base + running index),
 * identical to run_sim.sh, so each spawned robot connects to its matching brain.
 * Small species (worm/fly/fish) are scaled up for visibility.
 */
UCLASS()
class NMAERBRIDGE_API ANmSimGameMode : public AGameModeBase
{
    GENERATED_BODY()

public:
    ANmSimGameMode();

protected:
    virtual void BeginPlay() override;

private:
    /** Resolve a robot type token (with aliases) to its actor class/canonical id, or nullptr. */
    static UClass* ResolveRobotClass(const FString& TypeToken, FString* OutCanonicalType = nullptr);

    /** Spawn one robot actor at Location, scaled by Scale, wiring its brain
     *  component (and, for a zebrafish, its water level) before BeginPlay. */
    AActor* SpawnRobot(UClass* RobotClass, const FString& BrainId,
                       const FString& Host, int32 Port,
                       const FVector& Location, float Scale, float WaterTopZ);

    /** Build one habitat of the given kind centred at Center. */
    void SpawnHabitat(ENmHabitat Kind, const FVector& Center,
                      float RadiusCm, float WallTopZ);

    /** Position the player's spectator camera to frame all spawned robots. */
    UFUNCTION()
    void FrameCamera();

    /** Diagnostic: log each spawned robot's world location and bounds after settle. */
    UFUNCTION()
    void LogRobotDiag();

    /** Spawned robot actors (for framing / diagnostics). */
    UPROPERTY()
    TArray<TObjectPtr<AActor>> SpawnedRobots;

    FTimerHandle DiagTimerHandle;

    /** Scene framing, computed after the robots are spawned. */
    FVector SceneCenter = FVector::ZeroVector;
    float SceneFocus = 400.f;    // extent used for camera distance (cm)
    float SceneRadius = 600.f;   // full scene half-extent (cm)
    float MinCameraDistance = 500.f;

    FTimerHandle FrameTimerHandle;
};
