#pragma once
#include "CoreMinimal.h"
#include "Components/ActorComponent.h"
#include "NmNaoChatComponent.generated.h"

class SWidget;
class SEditableTextBox;
class STextBlock;
class IHttpRequest;
class UTextRenderComponent;
class FJsonObject;

/** Focused local-player conversation; uses the shared NAO social transducer. */
UCLASS(ClassGroup=(NeuralMimicry), meta=(BlueprintSpawnableComponent))
class NMAERBRIDGE_API UNmNaoChatComponent : public UActorComponent
{
    GENERATED_BODY()
public:
    UNmNaoChatComponent();
    UFUNCTION(BlueprintCallable, Category="NAO|Conversation")
    bool SubmitText(AActor* Speaker, const FString& Text);
    UFUNCTION(BlueprintCallable, Category="NAO|Conversation")
    void StopConversation();
    UPROPERTY(EditAnywhere, Category="NAO|Conversation")
    float InteractionRangeCm = 800.f;
protected:
    virtual void BeginPlay() override;
    virtual void TickComponent(float DeltaTime, ELevelTick TickType, FActorComponentTickFunction* Function) override;
    virtual void EndPlay(const EEndPlayReason::Type Reason) override;
private:
    FString Url, Token, Invitation, Principal, TurnId, Status, Conversation;
    int32 Generation = 0;
    uint64 Sequence = 0;
    bool bBusy = false, bEncountered = false;
    double NextPoll = 0, Deadline = 0, BubbleUntil = 0;
    TWeakObjectPtr<AActor> CurrentSpeaker;
    TSharedPtr<SWidget> Window;
    TSharedPtr<SEditableTextBox> Input;
    TSharedPtr<STextBlock> StatusLabel, ConversationLabel;
    TSharedPtr<IHttpRequest, ESPMode::ThreadSafe> Pending;
    UPROPERTY() TObjectPtr<UTextRenderComponent> Bubble;
    bool Nearby(AActor* Speaker) const;
    void MakeWindow();
    void Post(const FString& Path, TSharedRef<FJsonObject> Body, bool bStop = false);
};
