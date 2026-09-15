package org.neuralmimicry.minecraft.mixin;

import net.minecraft.gametest.framework.GameTestServer;
import net.minecraft.server.MinecraftServer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Test-only storage profile: use buffered region writes, then the ordinary explicit
 * save/flush/close boundary. Avoid thousands of per-sector O_DSYNC writes on laptops.
 * No save, flush, assertion, exit code or production server method is bypassed. */
@Mixin(MinecraftServer.class)
public class TestStorage {
    @Inject(method="forceSynchronousWrites",at=@At("HEAD"),cancellable=true)
    private void bufferedTestWrites(CallbackInfoReturnable<Boolean> callback) {
        if((Object)this instanceof GameTestServer)callback.setReturnValue(false);
    }
}
