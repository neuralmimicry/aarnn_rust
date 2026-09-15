// Shared generated habitat and visual morphology adapter; no neural state ownership.
#pragma once
#include "CoreMinimal.h"
class UWorld;
class AActor;
namespace NmSharedContent
{
    NMAERBRIDGE_API bool SpawnHabitat(UWorld* World, const FString& Profile,
        const FVector& Centre, float RadiusCm);
    NMAERBRIDGE_API bool Channels(const FString& Profile, bool bOutputs, TArray<FString>& Names);
    NMAERBRIDGE_API float SampleField(const FString& Profile, const FString& Cue,
        const FVector& Position, const FVector& Centre, float RadiusCm);
    NMAERBRIDGE_API bool DressRobot(AActor* Robot, const FString& Profile, bool bAnatomyCutaway = false);
}
