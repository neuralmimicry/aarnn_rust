// SPDX-FileCopyrightText: NEURALMIMICRY LTD
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.

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
