#pragma once
#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "NmEmbodimentWorld.generated.h"

/** Opt-in, unscaled native Chaos scene; the shared Rust ABI owns AER/filtering.
 * Legacy visual robot factories and their physics settings are not modified. */
UCLASS()
class NMAERBRIDGE_API ANmEmbodimentWorld : public AActor {
  GENERATED_BODY()
public:
  ANmEmbodimentWorld();
  virtual ~ANmEmbodimentWorld() override;
  bool Configure(const FString &Manifest);
  virtual void Tick(float DeltaSeconds) override;
  virtual void EndPlay(const EEndPlayReason::Type Reason) override;

private:
  struct FImpl;
  struct FImplDelete {
    void operator()(FImpl *) const;
  };
  TUniquePtr<FImpl, FImplDelete> Impl;
  UFUNCTION()
  void Contact(UPrimitiveComponent *Hit, AActor *OtherActor,
               UPrimitiveComponent *Other, FVector Impulse,
               const FHitResult &Result);
};
