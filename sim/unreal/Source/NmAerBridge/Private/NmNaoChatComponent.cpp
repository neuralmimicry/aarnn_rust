#include "NmNaoChatComponent.h"
#include "NmRobotBase.h"
#include "Components/TextRenderComponent.h"
#include "Engine/Engine.h"
#include "Engine/GameViewportClient.h"
#include "GameFramework/PlayerController.h"
#include "GameFramework/Pawn.h"
#include "HttpModule.h"
#include "Interfaces/IHttpRequest.h"
#include "Interfaces/IHttpResponse.h"
#include "Misc/FileHelper.h"
#include "Serialization/JsonReader.h"
#include "Serialization/JsonSerializer.h"
#include "Widgets/Layout/SBorder.h"
#include "Widgets/SOverlay.h"
#include "Widgets/SBoxPanel.h"
#include "Widgets/Input/SButton.h"
#include "Widgets/Input/SEditableTextBox.h"
#include "Widgets/Text/STextBlock.h"

UNmNaoChatComponent::UNmNaoChatComponent()
{
    PrimaryComponentTick.bCanEverTick = true;
    Principal = TEXT("unreal:") + FGuid::NewGuid().ToString(EGuidFormats::Digits);
}

void UNmNaoChatComponent::BeginPlay()
{
    Super::BeginPlay();
    FString File = FPlatformMisc::GetEnvironmentVariable(TEXT("NM_NAO_SESSION_FILE")), Json;
    if (File.IsEmpty() || !FFileHelper::LoadFileToString(Json, *File) || Json.Len() > 16384) { SetComponentTickEnabled(false); return; }
    TSharedPtr<FJsonObject> Config;
    if (!FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Json), Config) || !Config.IsValid()) { SetComponentTickEnabled(false); return; }
    FString Schema;
    if (!Config->TryGetStringField(TEXT("schema"), Schema) || Schema != TEXT("NAO-SOCIAL/1") ||
        !Config->TryGetStringField(TEXT("url"), Url) || !Url.StartsWith(TEXT("http://127.0.0.1:")) ||
        !Config->TryGetStringField(TEXT("adapter_token"), Token) || Token.Len() < 24 ||
        !Config->TryGetStringField(TEXT("join_token"), Invitation)) { SetComponentTickEnabled(false); return; }
    Status = TEXT("Small social vocabulary: try hello, name or help.");
    Bubble = NewObject<UTextRenderComponent>(GetOwner());
    Bubble->SetupAttachment(GetOwner()->GetRootComponent()); Bubble->RegisterComponent();
    Bubble->SetRelativeLocation(FVector(0,0,100)); Bubble->SetWorldSize(8);
    Bubble->SetHorizontalAlignment(EHTA_Center); Bubble->SetTextRenderColor(FColor(140,240,225));
}

bool UNmNaoChatComponent::Nearby(AActor* Speaker) const
{
    const UNmRobotBase* Brain = GetOwner()->FindComponentByClass<UNmRobotBase>();
    return IsValid(Speaker) && Speaker->GetWorld() == GetWorld() && Brain && Brain->bConnected &&
        FVector::Dist(Speaker->GetActorLocation(), GetOwner()->GetActorLocation()) <= InteractionRangeCm;
}

bool UNmNaoChatComponent::SubmitText(AActor* Speaker, const FString& Text)
{
    FTCHARToUTF8 Bytes(*Text);
    if (Token.IsEmpty() || bBusy || !Nearby(Speaker)) { Status = TEXT("Move within range of connected NAO."); return false; }
    if (Text.TrimStartAndEnd().IsEmpty() || Bytes.Length() > 256 || Text.Contains(TEXT("\n")) || Text.Contains(TEXT("\r")))
    { Status = TEXT("Use 1–256 UTF-8 bytes."); return false; }
    TSharedRef<FJsonObject> Body = MakeShared<FJsonObject>();
    Body->SetStringField(TEXT("player"), Principal); Body->SetStringField(TEXT("text"), Text);
    Body->SetStringField(TEXT("modality"), TEXT("typed")); Body->SetNumberField(TEXT("sequence"), Sequence++);
    Body->SetNumberField(TEXT("capture_ns"), FMath::FloorToDouble(FPlatformTime::Seconds()*1e9));
    FVector Relative = (Speaker->GetActorLocation()-GetOwner()->GetActorLocation()) / InteractionRangeCm;
    Body->SetArrayField(TEXT("position"), {MakeShared<FJsonValueNumber>(Relative.Y), MakeShared<FJsonValueNumber>(Relative.Z), MakeShared<FJsonValueNumber>(Relative.X)});
    CurrentSpeaker = Speaker; bBusy = true; Deadline = FPlatformTime::Seconds()+60;
    Conversation = TEXT("You: ")+Text; Status = TEXT("Sending to NAO…");
    Post(TEXT("/api/turn"), Body); return true;
}

void UNmNaoChatComponent::Post(const FString& Path, TSharedRef<FJsonObject> Body, bool bStop)
{
    FString Json; FJsonSerializer::Serialize(Body, TJsonWriterFactory<>::Create(&Json));
    const int32 Epoch = Generation;
    auto Request = FHttpModule::Get().CreateRequest(); Pending = Request;
    Request->SetURL(Url+Path); Request->SetVerb(TEXT("POST")); Request->SetTimeout(5.f);
    Request->SetHeader(TEXT("Content-Type"), TEXT("application/json")); Request->SetHeader(TEXT("Authorization"), TEXT("Bearer ")+Token);
    Request->SetContentAsString(Json);
    Request->OnProcessRequestComplete().BindWeakLambda(this, [this, Epoch, bStop](FHttpRequestPtr RequestPtr, FHttpResponsePtr Response, bool Ok)
    {
        if (Epoch != Generation || Pending != RequestPtr) return;
        Pending.Reset(); if (bStop) return;
        TSharedPtr<FJsonObject> Data;
        if (!Ok || !Response.IsValid() || Response->GetResponseCode()!=200 || Response->GetContentLength()>16384 ||
            !FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Response->GetContentAsString()), Data) || !Data.IsValid())
        { bBusy=false; TurnId.Empty(); Status=TEXT("Conversation unavailable; no reply was replayed."); return; }
        FString Schema, State, Id;
        if (!Data->TryGetStringField(TEXT("schema"),Schema) || Schema!=TEXT("NAO-SOCIAL/1") ||
            !Data->TryGetStringField(TEXT("state"),State) || !Data->TryGetStringField(TEXT("id"),Id) || !Nearby(CurrentSpeaker.Get()))
        { StopConversation(); return; }
        if (!TurnId.IsEmpty() && TurnId!=Id) { StopConversation(); return; }
        TurnId=Id;
        const TSharedPtr<FJsonObject>* Reply;
        if (State==TEXT("replied") && Data->TryGetObjectField(TEXT("reply"),Reply))
        {
            FString Text;
            if (!(*Reply)->TryGetStringField(TEXT("text"),Text) || Text.Len()>512) { StopConversation(); return; }
            Conversation += TEXT("\nNAO: ")+Text; Status=TEXT("Your turn."); bBusy=false; TurnId.Empty();
            Bubble->SetText(FText::FromString(Text)); BubbleUntil=FPlatformTime::Seconds()+10;
        }
        else if (State==TEXT("queued") || State==TEXT("active")) { Status=State==TEXT("queued")?TEXT("Waiting for NAO’s attention…"):TEXT("NAO is receiving your message…"); NextPoll=FPlatformTime::Seconds()+.2; }
        else { bBusy=false; TurnId.Empty(); Status=State; }
    });
    if (!Request->ProcessRequest()) { Pending.Reset(); bBusy=false; Status=TEXT("Conversation transport unavailable."); }
}

void UNmNaoChatComponent::StopConversation()
{
    Generation++; bEncountered=true; if (Pending) { Pending->CancelRequest(); Pending.Reset(); }
    bBusy=false; TurnId.Empty(); Status=TEXT("Conversation stopped.");
    if (Bubble) Bubble->SetText(FText::GetEmpty());
    if (!Token.IsEmpty()) { auto Body=MakeShared<FJsonObject>(); Body->SetStringField(TEXT("player"),Principal); Post(TEXT("/api/stop"),Body,true); }
}

void UNmNaoChatComponent::MakeWindow()
{
    if (Window || !GEngine || !GEngine->GameViewport) return;
    Window = SNew(SOverlay) + SOverlay::Slot().HAlign(HAlign_Left).VAlign(VAlign_Bottom).Padding(16)
      [SNew(SBorder).Padding(8)
      [SNew(SVerticalBox)
        + SVerticalBox::Slot().AutoHeight()[SNew(STextBlock).Text(FText::FromString(TEXT("Talk to NAO")))]
        + SVerticalBox::Slot().AutoHeight()[SAssignNew(StatusLabel,STextBlock)]
        + SVerticalBox::Slot().AutoHeight()[SAssignNew(ConversationLabel,STextBlock).WrapTextAt(550)]
        + SVerticalBox::Slot().AutoHeight()[SAssignNew(Input,SEditableTextBox).HintText(FText::FromString(TEXT("Hello NAO")))]
        + SVerticalBox::Slot().AutoHeight()[SNew(SHorizontalBox)
          + SHorizontalBox::Slot().AutoWidth()[SNew(SButton).Text(FText::FromString(TEXT("Send"))).OnClicked_Lambda([this](){auto PC=GetWorld()->GetFirstPlayerController();if(PC && SubmitText(PC->GetPawn(),Input->GetText().ToString()))Input->SetText(FText::GetEmpty());return FReply::Handled();})]
          + SHorizontalBox::Slot().AutoWidth()[SNew(SButton).Text(FText::FromString(TEXT("Stop conversation"))).OnClicked_Lambda([this](){StopConversation();return FReply::Handled();})]
          + SHorizontalBox::Slot().AutoWidth()[SNew(SButton).Text(FText::FromString(TEXT("Voice / accessible chat"))).OnClicked_Lambda([this](){FPlatformProcess::LaunchURL(*(Url+TEXT("/#invite=")+Invitation),nullptr,nullptr);return FReply::Handled();})]
        ]
      ]];
    GEngine->GameViewport->AddViewportWidgetContent(Window.ToSharedRef(),20);
    if (auto PC=GetWorld()->GetFirstPlayerController()) { PC->bShowMouseCursor=true; PC->SetInputMode(FInputModeGameAndUI()); }
}

void UNmNaoChatComponent::TickComponent(float DeltaTime,ELevelTick TickType,FActorComponentTickFunction* Function)
{
    Super::TickComponent(DeltaTime,TickType,Function); if(Token.IsEmpty())return; MakeWindow();
    if(StatusLabel)StatusLabel->SetText(FText::FromString(Status));
    if(ConversationLabel)ConversationLabel->SetText(FText::FromString(Conversation));
    if(Bubble && FPlatformTime::Seconds()>BubbleUntil)Bubble->SetText(FText::GetEmpty());
    if(!bEncountered && !bBusy) {
        auto PC=GetWorld()->GetFirstPlayerController();auto Speaker=PC?PC->GetPawn():nullptr;
        if(Nearby(Speaker)) {
            bEncountered=true;bBusy=true;CurrentSpeaker=Speaker;Deadline=FPlatformTime::Seconds()+60;
            auto Body=MakeShared<FJsonObject>();Body->SetStringField(TEXT("player"),Principal);
            Body->SetStringField(TEXT("kind"),TEXT("player"));
            Body->SetNumberField(TEXT("sequence"),Sequence++);Body->SetNumberField(TEXT("capture_ns"),FMath::FloorToDouble(FPlatformTime::Seconds()*1e9));
            FVector Relative=(Speaker->GetActorLocation()-GetOwner()->GetActorLocation())/InteractionRangeCm;
            Body->SetArrayField(TEXT("position"),{MakeShared<FJsonValueNumber>(Relative.Y),MakeShared<FJsonValueNumber>(Relative.Z),MakeShared<FJsonValueNumber>(Relative.X)});
            Post(TEXT("/api/encounter"),Body);
        }
    }
    if(!bBusy)return;
    if(!Nearby(CurrentSpeaker.Get()) || FPlatformTime::Seconds()>Deadline) { StopConversation(); return; }
    if(!Pending && !TurnId.IsEmpty() && FPlatformTime::Seconds()>=NextPoll)
    { auto Body=MakeShared<FJsonObject>();Body->SetStringField(TEXT("player"),Principal);Body->SetStringField(TEXT("id"),TurnId);Post(TEXT("/api/poll"),Body); }
}

void UNmNaoChatComponent::EndPlay(const EEndPlayReason::Type Reason)
{
    StopConversation(); Generation++; if(Pending){Pending->CancelRequest();Pending.Reset();}
    if(Window && GEngine && GEngine->GameViewport)GEngine->GameViewport->RemoveViewportWidgetContent(Window.ToSharedRef());
    Window.Reset(); Super::EndPlay(Reason);
}
