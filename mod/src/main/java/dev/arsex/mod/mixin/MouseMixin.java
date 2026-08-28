package dev.arsex.mod.mixin;

import dev.arsex.mod.ArsexMod;
import net.minecraft.client.Mouse;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Feeds the CPS counter.
 *
 * HEAD injection with no cancellation: this observes clicks, it never
 * swallows them, so vanilla and every other mod still see the same input.
 */
@Mixin(Mouse.class)
public class MouseMixin {

    @Inject(method = "onMouseButton", at = @At("HEAD"))
    private void arsex$click(long window, int button, int action, int mods, CallbackInfo ci) {
        if (action != 1) return;             // GLFW_PRESS only, ignore release/repeat
        var cps = ArsexMod.cps();
        if (cps == null || !cps.isEnabled()) return;
        if (button == 0) cps.onLeftClick();
        else if (button == 1) cps.onRightClick();
    }
}
