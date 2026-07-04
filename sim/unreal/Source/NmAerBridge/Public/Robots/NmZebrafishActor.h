// Copyright NeuralMimicry. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "NmRobotBase.h"
#include "NmZebrafishActor.generated.h"

class UStaticMeshComponent;
class UPhysicsConstraintComponent;
class USceneCaptureComponent2D;
class UTextureRenderTarget2D;

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

UCLASS(ClassGroup = (NeuralMimicry), meta = (BlueprintSpawnableComponent))
class NMAERBRIDGE_API UNmZebrafishComponent : public UNmRobotBase
{
    GENERATED_BODY()

public:
    UNmZebrafishComponent();

    static constexpr int32 NumBodySegments   = 7;
    static constexpr int32 NumTailSegments   = 4;
    static constexpr int32 NumSegments       = NumBodySegments + NumTailSegments; // 11
    static constexpr int32 NumLateralLine    = 16;  // proximity sensors
    static constexpr int32 NumOpticalFlow    = 8;   // 4 quadrants per eye (head only)
    static constexpr int32 NumTailAngles     = 4;   // tail joint angles
    static constexpr int32 NumSwimBladder    = 2;
    static constexpr int32 NumVestibular     = 2;
    static constexpr int32 NumSensors        = NumLateralLine + NumOpticalFlow
                                               + NumTailAngles + NumSwimBladder
                                               + NumVestibular; // 32
    static constexpr int32 NumBodyActuators  = NumSegments * 2;  // dorsal+ventral per seg = 22
    static constexpr int32 NumFinActuators   = 10;
    static constexpr int32 NumActuators      = NumBodyActuators + NumFinActuators; // 32

    // Set by actor
    UPROPERTY()
    TArray<TObjectPtr<UStaticMeshComponent>> SegmentMeshes;  // [NumSegments]

    UPROPERTY()
    TArray<TObjectPtr<UPhysicsConstraintComponent>> SegmentJoints; // [NumSegments - 1]

    UPROPERTY()
    TObjectPtr<USceneCaptureComponent2D> EyeCamera;  // head optical flow

    // Water surface world Z position (editable per-level)
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmZebrafish")
    float WaterSurfaceZ = 0.f;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> CaudalFin;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> DorsalFin;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> AnalFin;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> PectoralFinLeft;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> PectoralFinRight;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> EyeLeft;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> EyeRight;

    // Buoyancy force scale (N equivalent; applied upward when below surface)
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmZebrafish")
    float BuoyancyForceScale = 98.f;   // ~10 kg-f in UE units

    // Lateral line probe radius in cm
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmZebrafish")
    float LateralLineRadiusCm = 10.f;  // 0.1 m

    // Previous optical flow luminance per quadrant
    float PrevQuadLuminance[4] = {0.f, 0.f, 0.f, 0.f};

    // Previous root angular velocity for vestibular derivative
    FVector PrevRootAngularVel = FVector::ZeroVector;

    // UNmRobotBase interface
    virtual void CollectSensors(TArray<float>& OutSensors) override;
    virtual void ApplyActuators(const TArray<float>& Actuators) override;
    virtual void GetSensorNames(TArray<FString>& OutNames) const override;
    virtual void GetActuatorNames(TArray<FString>& OutNames) const override;

    // Called from actor Tick for buoyancy
    void ApplyBuoyancy();

private:
    float SampleSegmentLateralProximity(int32 SegIdx, bool bLeftSide) const;
};

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

UCLASS(ClassGroup = (NeuralMimicry))
class NMAERBRIDGE_API ANmZebrafishActor : public AActor
{
    GENERATED_BODY()

public:
    ANmZebrafishActor();

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UNmZebrafishComponent> ZebrafishComponent;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TArray<TObjectPtr<UStaticMeshComponent>> SegmentMeshes;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TArray<TObjectPtr<UPhysicsConstraintComponent>> SegmentJoints;

    // Water surface for buoyancy (world Z, cm)
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmZebrafish")
    float WaterSurfaceZ = 0.f;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> CaudalFin;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> DorsalFin;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> AnalFin;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> PectoralFinLeft;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> PectoralFinRight;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> EyeLeft;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmZebrafish")
    TObjectPtr<UStaticMeshComponent> EyeRight;

protected:
    virtual void BeginPlay() override;
    virtual void Tick(float DeltaSeconds) override;
};
