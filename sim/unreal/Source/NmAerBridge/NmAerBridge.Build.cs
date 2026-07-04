// Copyright NeuralMimicry. All Rights Reserved.

using UnrealBuildTool;

public class NmAerBridge : ModuleRules
{
    public NmAerBridge(ReadOnlyTargetRules Target) : base(Target)
    {
        PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;

        PublicDependencyModuleNames.AddRange(new string[]
        {
            "Core",
            "CoreUObject",
            "Engine",
            "Sockets",
            "Networking",
            "ProceduralMeshComponent",
        });

        PrivateDependencyModuleNames.AddRange(new string[]
        {
            "Json",
            "JsonUtilities",
        });
    }
}
