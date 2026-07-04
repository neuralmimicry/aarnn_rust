// Copyright NeuralMimicry. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "NmRobotBase.h"
#include "NmHexapodActor.generated.h"

class UStaticMeshComponent;
class UPhysicsConstraintComponent;
class USceneCaptureComponent2D;
class UTextureRenderTarget2D;

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

UCLASS(ClassGroup = (NeuralMimicry), meta = (BlueprintSpawnableComponent))
class NMAERBRIDGE_API UNmHexapodComponent : public UNmRobotBase
{
    GENERATED_BODY()

public:
    UNmHexapodComponent();

    static constexpr int32 NumLegs          = 6;
    static constexpr int32 JointsPerLeg     = 3;   // coxa, femur, tibia
    static constexpr int32 NumJoints        = NumLegs * JointsPerLeg; // 18
    static constexpr int32 NumFootContacts  = NumLegs;
    static constexpr int32 NumIMUSensors    = 6;   // 3 accel + 3 gyro
    static constexpr int32 NumSonarSensors  = 2;   // front + rear
    static constexpr int32 NumCameraChannels= 2;
    static constexpr int32 NumSensors       = NumJoints + NumFootContacts
                                              + NumIMUSensors + NumSonarSensors
                                              + NumCameraChannels; // 34
    static constexpr int32 NumActuators     = 18;

    // Set by actor
    UPROPERTY()
    TObjectPtr<UStaticMeshComponent> BodyMesh;

    UPROPERTY()
    TArray<TObjectPtr<UStaticMeshComponent>> LegSegments;   // [NumLegs * JointsPerLeg]

    UPROPERTY()
    TArray<TObjectPtr<UStaticMeshComponent>> FootSpheres;   // [NumLegs]

    UPROPERTY()
    TArray<TObjectPtr<UPhysicsConstraintComponent>> LegJoints; // [NumJoints]

    UPROPERTY()
    TObjectPtr<USceneCaptureComponent2D> HeadCamera;

    // Foot contact flags
    bool FootContacts[NumLegs] = {};

    // Prev body velocity
    FVector PrevLinearVel  = FVector::ZeroVector;
    FVector PrevAngularVel = FVector::ZeroVector;

    // Prev camera luminance for optical-flow-like event channels
    float PrevLuminance[2] = {0.f, 0.f};

    // UNmRobotBase interface
    virtual void CollectSensors(TArray<float>& OutSensors) override;
    virtual void ApplyActuators(const TArray<float>& Actuators) override;
    virtual void GetSensorNames(TArray<FString>& OutNames) const override;
    virtual void GetActuatorNames(TArray<FString>& OutNames) const override;
};

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

UCLASS(ClassGroup = (NeuralMimicry))
class NMAERBRIDGE_API ANmHexapodActor : public AActor
{
    GENERATED_BODY()

public:
    ANmHexapodActor();

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TObjectPtr<UNmHexapodComponent> HexapodComponent;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TObjectPtr<UStaticMeshComponent> BodyMesh;

    // Visual chassis pieces (non-physical) so the robot reads as a Freenove-like
    // stacked board platform rather than a single cuboid.
    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TObjectPtr<UStaticMeshComponent> ChassisDeckLower;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TObjectPtr<UStaticMeshComponent> ChassisDeckUpper;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TObjectPtr<UStaticMeshComponent> ChassisNose;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TArray<TObjectPtr<UStaticMeshComponent>> LegSegments;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TArray<TObjectPtr<UStaticMeshComponent>> FootSpheres;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TArray<TObjectPtr<UPhysicsConstraintComponent>> LegJoints;

    // Visible sensor package on the body front (cosmetic, matches the Freenove
    // hexapod: a camera housing between two ultrasonic "eyes").
    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TObjectPtr<UStaticMeshComponent> CameraHousing;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TObjectPtr<UStaticMeshComponent> SonarLeft;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TObjectPtr<UStaticMeshComponent> SonarRight;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TArray<TObjectPtr<UStaticMeshComponent>> Standoffs;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmHexapod")
    TArray<TObjectPtr<UStaticMeshComponent>> WireRuns;

    // Max ultrasonic range in cm (UE units = cm)
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmHexapod")
    float MaxSonarRangeCm = 150.f;

protected:
    virtual void BeginPlay() override;

private:
    UFUNCTION()
    void OnFootHit(UPrimitiveComponent* HitComp, AActor* OtherActor,
                   UPrimitiveComponent* OtherComp, FVector NormalImpulse,
                   const FHitResult& Hit);
};
