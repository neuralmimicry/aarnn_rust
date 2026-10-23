package org.neuralmimicry.minecraft;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Display;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.level.block.Blocks;
final class PhysicsCompat {
  static boolean tagged(Display.BlockDisplay e, String tag) {
    return e.entityTags().contains(tag);
  }
  static Display.BlockDisplay display(ServerLevel level) {
    var e = new Display.BlockDisplay(EntityTypes.BLOCK_DISPLAY, level);
    e.setBlockState(Blocks.CONCRETE.lightGray().defaultBlockState());
    return e;
  }
}
