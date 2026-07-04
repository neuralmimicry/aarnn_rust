// Copyright NeuralMimicry. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "NmRobotBase.h"
#include "NmDrosophilaActor.generated.h"

class UStaticMeshComponent;
class UPhysicsConstraintComponent;
class USceneCaptureComponent2D;
class UTextureRenderTarget2D;

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

UCLASS(ClassGroup = (NeuralMimicry), meta = (BlueprintSpawnableComponent))
class NMAERBRIDGE_API UNmDrosophilaComponent : public UNmRobotBase
{
    GENERATED_BODY()

public:
    UNmDrosophilaComponent();

    static constexpr int32 NumLegs           = 6;
    static constexpr int32 LegSegments       = 4;   // coxa, femur, tibia, tarsus
    static constexpr int32 NumWings          = 2;
    static constexpr int32 WingJoints        = 2;   // hinge joints per wing
    static constexpr int32 NumLegJoints      = NumLegs * LegSegments;  // 24
    static constexpr int32 NumFootContacts   = NumLegs;                // 6
    static constexpr int32 NumAntennaTraces  = 2;
    static constexpr int32 NumIMUSensors     = 2;   // compact accel/turn summary
    static constexpr int32 NumCoreSensors    = NumLegJoints + NumFootContacts
                                               + NumAntennaTraces + NumIMUSensors; // 34
    static constexpr int32 RetinaWidth       = 12;
    static constexpr int32 RetinaHeight      = 8;
    static constexpr int32 NumEyePixels      = RetinaWidth * RetinaHeight; // 96
    static constexpr int32 NumEyeEventChans  = NumEyePixels * 4;           // 384
    static constexpr int32 NumSensors        = NumCoreSensors + NumEyeEventChans; // 418
    static constexpr int32 NumActuators      = 48;

    // Body parts (set by actor)
    UPROPERTY()
    TObjectPtr<UStaticMeshComponent> Thorax;

    UPROPERTY()
    TObjectPtr<UStaticMeshComponent> Head;

    UPROPERTY()
    TArray<TObjectPtr<UStaticMeshComponent>> LegSegmentMeshes;  // [24]

    UPROPERTY()
    TArray<TObjectPtr<UStaticMeshComponent>> WingMeshes;        // [2]

    UPROPERTY()
    TArray<TObjectPtr<UPhysicsConstraintComponent>> LegJoints;  // [24]

    UPROPERTY()
    TArray<TObjectPtr<UPhysicsConstraintComponent>> WingJointComps; // [4]

    // Eye scene captures
    UPROPERTY()
    TObjectPtr<USceneCaptureComponent2D> EyeLeft;

    UPROPERTY()
    TObjectPtr<USceneCaptureComponent2D> EyeRight;

    UPROPERTY()
    TObjectPtr<UTextureRenderTarget2D> EyeLeftRT;

    UPROPERTY()
    TObjectPtr<UTextureRenderTarget2D> EyeRightRT;

    // Foot contact flags (set via OnComponentHit)
    bool FootContacts[NumLegs] = {};

    // Prev body velocity for accel derivative
    FVector PrevLinearVel  = FVector::ZeroVector;
    FVector PrevAngularVel = FVector::ZeroVector;

    // Previous eye luminance for event-delta encoding.
    TArray<float> PrevEyeLeftLum;
    TArray<float> PrevEyeRightLum;

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
class NMAERBRIDGE_API ANmDrosophilaActor : public AActor
{
    GENERATED_BODY()

public:
    ANmDrosophilaActor();

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmDrosophila")
    TObjectPtr<UNmDrosophilaComponent> DrosophilaComponent;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmDrosophila")
    TObjectPtr<UStaticMeshComponent> ThoraxMesh;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmDrosophila")
    TObjectPtr<UStaticMeshComponent> AbdomenMesh;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmDrosophila")
    TObjectPtr<UStaticMeshComponent> EyeLeftMesh;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmDrosophila")
    TObjectPtr<UStaticMeshComponent> EyeRightMesh;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmDrosophila")
    TObjectPtr<UStaticMeshComponent> HeadMesh;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmDrosophila")
    TArray<TObjectPtr<UStaticMeshComponent>> LegSegmentMeshes;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmDrosophila")
    TArray<TObjectPtr<UStaticMeshComponent>> WingMeshes;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmDrosophila")
    TArray<TObjectPtr<UPhysicsConstraintComponent>> LegJoints;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmDrosophila")
    TArray<TObjectPtr<UPhysicsConstraintComponent>> WingJointComps;

protected:
    virtual void BeginPlay() override;

private:
    UFUNCTION()
    void OnLegHit(UPrimitiveComponent* HitComp, AActor* OtherActor,
                  UPrimitiveComponent* OtherComp, FVector NormalImpulse,
                  const FHitResult& Hit);
};
