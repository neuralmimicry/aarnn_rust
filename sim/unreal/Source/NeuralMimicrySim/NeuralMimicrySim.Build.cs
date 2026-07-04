// Copyright NeuralMimicry. All Rights Reserved.

using UnrealBuildTool;

public class NeuralMimicrySim : ModuleRules
{
    public NeuralMimicrySim(ReadOnlyTargetRules Target) : base(Target)
    {
        PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;

        PublicDependencyModuleNames.AddRange(new string[]
        {
            "Core",
            "CoreUObject",
            "Engine",
            "InputCore",
            // The AARNN bridge module (clients, robot components, sim manager).
            "NmAerBridge",
        });
    }
}
