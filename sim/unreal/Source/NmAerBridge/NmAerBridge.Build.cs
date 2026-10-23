// Copyright NeuralMimicry. All Rights Reserved.

using UnrealBuildTool;
using System.IO;

public class NmAerBridge : ModuleRules
{
    public NmAerBridge(ReadOnlyTargetRules Target) : base(Target)
    {
        bEnableExceptions = true;
        PublicIncludePaths.Add(Path.GetFullPath(Path.Combine(ModuleDirectory, "../../../../crates/aarnn-embodiment-ffi/include")));
        PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;

        PublicDependencyModuleNames.AddRange(new string[]
        {
            "Core",
            "PhysicsCore",
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
            "HTTP",
            "Slate",
            "SlateCore",
        });
    }
}
