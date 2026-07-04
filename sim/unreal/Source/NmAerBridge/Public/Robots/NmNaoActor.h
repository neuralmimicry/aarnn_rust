// Copyright NeuralMimicry. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "NmRobotBase.h"
#include "NmNaoActor.generated.h"

class UStaticMeshComponent;
class UPhysicsConstraintComponent;
class USceneCaptureComponent2D;
class UTextureRenderTarget2D;

// ---------------------------------------------------------------------------
// Enumerates each named joint for array indexing
// ---------------------------------------------------------------------------

UENUM(BlueprintType)
enum class ENaoJoint : uint8
{
    // Head (2)
    HeadYaw = 0,
    HeadPitch,
    // Left arm (5)
    LShoulderPitch,
    LShoulderRoll,
    LElbowYaw,
    LElbowRoll,
    LWristYaw,
    // Right arm (5)
    RShoulderPitch,
    RShoulderRoll,
    RElbowYaw,
    RElbowRoll,
    RWristYaw,
    // Left leg (6)
    LHipYawPitch,
    LHipRoll,
    LHipPitch,
    LKneePitch,
    LAnklePitch,
    LAnkleRoll,
    // Right leg (6)
    RHipYawPitch,
    RHipRoll,
    RHipPitch,
    RKneePitch,
    RAnklePitch,
    RAnkleRoll,

    Count
};

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

UCLASS(ClassGroup = (NeuralMimicry), meta = (BlueprintSpawnableComponent))
class NMAERBRIDGE_API UNmNaoComponent : public UNmRobotBase
{
    GENERATED_BODY()

public:
    UNmNaoComponent();

    static constexpr int32 NumJoints          = static_cast<int32>(ENaoJoint::Count); // 24
    static constexpr int32 NumSonar           = 2;
    static constexpr int32 NumAccel           = 3;
    static constexpr int32 NumGyro            = 2;
    static constexpr int32 NumGPS             = 3;
    static constexpr int32 NumInertial        = 3;
    static constexpr int32 NumFootPressure    = 8;   // 4L + 4R
    static constexpr int32 NumBumpers         = 2;
    static constexpr int32 NumJointPosSensors = NumJoints; // 24
    static constexpr int32 NumJointVelSensors = 11;
    static constexpr int32 NumBaseSensors     = NumSonar + NumAccel + NumGyro
                                                + NumGPS + NumInertial
                                                + NumFootPressure + NumBumpers
                                                + NumJointPosSensors + NumJointVelSensors; // 58
    static constexpr int32 NumEyeChannels     = 192; // 8x6 retina × (L/R) × (on/off)
    static constexpr int32 NumSensors         = NumBaseSensors + NumEyeChannels; // 250
    static constexpr int32 NumActuators       = 40;

    // Set by actor
    UPROPERTY()
    TObjectPtr<UStaticMeshComponent> TorsoMesh;

    UPROPERTY()
    TObjectPtr<UStaticMeshComponent> HeadMesh;

    UPROPERTY()
    TArray<TObjectPtr<UStaticMeshComponent>> ArmSegments;     // L+R, shoulder→wrist

    UPROPERTY()
    TArray<TObjectPtr<UStaticMeshComponent>> LegSegments;     // L+R, hip→foot

    UPROPERTY()
    TArray<TObjectPtr<UStaticMeshComponent>> FeetMeshes;      // [2]

    UPROPERTY()
    TArray<TObjectPtr<UPhysicsConstraintComponent>> JointConstraints; // [NumJoints]

    UPROPERTY()
    TObjectPtr<USceneCaptureComponent2D> EyeCamera;

    // LED blueprint-readable floats
    UPROPERTY(BlueprintReadOnly, Category = "NmNao|LEDs")
    float LedChestR = 0.f;
    UPROPERTY(BlueprintReadOnly, Category = "NmNao|LEDs")
    float LedChestG = 0.f;
    UPROPERTY(BlueprintReadOnly, Category = "NmNao|LEDs")
    float LedChestB = 0.f;
    UPROPERTY(BlueprintReadOnly, Category = "NmNao|LEDs")
    float LedFootL  = 0.f;
    UPROPERTY(BlueprintReadOnly, Category = "NmNao|LEDs")
    float LedFootR  = 0.f;
    UPROPERTY(BlueprintReadOnly, Category = "NmNao|LEDs")
    float LedEarL   = 0.f;
    UPROPERTY(BlueprintReadOnly, Category = "NmNao|LEDs")
    float LedEarR   = 0.f;
    UPROPERTY(BlueprintReadOnly, Category = "NmNao|LEDs")
    float LedEyeL   = 0.f;

    // Prev velocity for IMU derivatives
    FVector PrevLinearVel  = FVector::ZeroVector;
    FVector PrevAngularVel = FVector::ZeroVector;
    TArray<float> PrevJointAnglesDeg;

    // Foot contact flags
    bool FootContacts[2] = {};   // [0]=L, [1]=R

    // UNmRobotBase interface
    virtual void CollectSensors(TArray<float>& OutSensors) override;
    virtual void ApplyActuators(const TArray<float>& Actuators) override;
    virtual void GetSensorNames(TArray<FString>& OutNames) const override;
    virtual void GetActuatorNames(TArray<FString>& OutNames) const override;

private:
    float SampleFootPressure(int32 FootIndex, int32 PointIndex) const;
};

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

UCLASS(ClassGroup = (NeuralMimicry))
class NMAERBRIDGE_API ANmNaoActor : public AActor
{
    GENERATED_BODY()

public:
    ANmNaoActor();

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmNao")
    TObjectPtr<UNmNaoComponent> NaoComponent;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmNao")
    TObjectPtr<UStaticMeshComponent> TorsoMesh;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmNao")
    TObjectPtr<UStaticMeshComponent> HeadMesh;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmNao")
    TArray<TObjectPtr<UStaticMeshComponent>> ArmSegments;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmNao")
    TArray<TObjectPtr<UStaticMeshComponent>> LegSegments;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmNao")
    TArray<TObjectPtr<UStaticMeshComponent>> FeetMeshes;

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "NmNao")
    TArray<TObjectPtr<UPhysicsConstraintComponent>> JointConstraints;

    // Sonar max range in cm
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "NmNao")
    float MaxSonarRangeCm = 255.f;

protected:
    virtual void BeginPlay() override;

private:
    UFUNCTION()
    void OnFootHit(UPrimitiveComponent* HitComp, AActor* OtherActor,
                   UPrimitiveComponent* OtherComp, FVector NormalImpulse,
                   const FHitResult& Hit);
};
