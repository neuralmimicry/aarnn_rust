// SPDX-FileCopyrightText: 2026 AARNN contributors
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See sim/minecraft/LICENSE in the project root.
package org.neuralmimicry.minecraft;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Display;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.level.block.Blocks;
final class PhysicsCompat {
  static boolean tagged(Display.BlockDisplay e, String tag) {
    return e.getTags().contains(tag);
  }
  static Display.BlockDisplay display(ServerLevel level) {
    var e = new Display.BlockDisplay(EntityType.BLOCK_DISPLAY, level);
    e.setBlockState(Blocks.LIGHT_GRAY_CONCRETE.defaultBlockState());
    return e;
  }
}
