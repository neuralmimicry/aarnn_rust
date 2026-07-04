// Copyright NeuralMimicry. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "NmRobotBase.h"
#include "NmCelegansActor.generated.h"

class UStaticMeshComponent;
class UPhysicsConstraintComponent;
class USceneComponent;

// ---------------------------------------------------------------------------
// Component — holds all sensor/actuator logic
// ---------------------------------------------------------------------------

UCLASS(ClassGroup = (NeuralMimicry), meta = (BlueprintSpawnableComponent))
class NMAERBRIDGE_API UNmCelegansComponent : public UNmRobotBase
{
    GENERATED_BODY()

public:
    UNmCelegansComponent();

    static constexpr int32 NumSegments  = 24;
    static constexpr int32 NumSensors   = 24;
    static constexpr int32 NumActuators = 96;

    // Segment mesh refs (set by owning actor constructor)
    UPROPERTY()
    TArray<TObjectPtr<UStaticMeshComponent>> SegmentMeshes;   // [NumSegments]

    // Joint refs (NumSegments - 1 joints)
    UPROPERTY()
    TArray<TObjectPtr<UPhysicsConstraintComponent>> Joints;

    // UNmRobotBase interface
    virtual void CollectSensors(TArray<float>& OutSensors) override;
    virtual void ApplyActuators(const TArray<float>& Actuators) override;
    virtual void GetSensorNames(TArray<FString>& OutNames) const override;
    virtual void GetActuatorNames(TArray<FString>& OutNames) const override;

private:
    // Previous root angular velocity (for vibration delta)
    FVector PrevRootAngularVel = FVector::ZeroVector;

    // Per-muscle low-pass traces so sparse spike outputs still drive smooth
    // contractions instead of one-frame joint pops.
    TArray<float> MdlTrace;
    TArray<float> MdrTrace;
    TArray<float> MvlTrace;
    TArray<float> MvrTrace;

    // Per-segment filtered drive values mapped into joint swing targets.
    TArray<float> FilteredDvDrive;
    TArray<float> FilteredLrDrive;

    // Flatline fallback state: inject a mild undulation if the decoded drive
    // stays near-neutral for too long.
    int32 FlatSteps = 0;
    int32 TwitchHoldRemaining = 0;
    float TwitchPhase = 0.0f;

    // Low-rate diagnostics so runtime logs can confirm actuator activity.
    int32 DriveDiagDecimator = 0;
};

// ---------------------------------------------------------------------------
// Actor — owns geometry and the component above
// ---------------------------------------------------------------------------

UCLASS(ClassGroup = (NeuralMimicry))
class NMAERBRIDGE_API ANmCelegansActor : public AActor
{
    GENERATED_BODY()

public:
    ANmCelegansActor();

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmCelegans")
    TObjectPtr<UNmCelegansComponent> CelegansComponent;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmCelegans")
    TArray<TObjectPtr<UStaticMeshComponent>> SegmentMeshes;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmCelegans")
    TArray<TObjectPtr<UPhysicsConstraintComponent>> Joints;

protected:
    virtual void BeginPlay() override;
    virtual void Tick(float DeltaSeconds) override;

private:
    void UpdateVisualBody();

    UPROPERTY()
    TObjectPtr<USceneComponent> SceneRoot;

    // Non-physical render links that make the celegans read as one continuous worm.
    UPROPERTY()
    TArray<TObjectPtr<UStaticMeshComponent>> VisualLinks;

    // Low-rate diagnostics to verify physical body movement at runtime.
    float MotionDiagAccum = 0.0f;
};
