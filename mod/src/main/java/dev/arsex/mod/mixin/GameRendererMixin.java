package dev.arsex.mod.mixin;

import dev.arsex.mod.ArsexMod;
import net.minecraft.client.render.Camera;
import net.minecraft.client.render.GameRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * Applies the Zoom module's FOV multiplier.
 *
 * Hooking getFov (rather than the options FOV value) is what makes zoom
 * compose correctly with sprint FOV, speed effects and shaders: we scale the
 * final computed value instead of fighting the game for the setting.
 */
@Mixin(GameRenderer.class)
public class GameRendererMixin {

    @Inject(method = "getFov", at = @At("RETURN"), cancellable = true)
    private void arsex$zoom(Camera camera, float tickDelta, boolean changingFov,
                            CallbackInfoReturnable<Double> cir) {
        var zoom = ArsexMod.zoom();
        if (zoom == null) return;
        zoom.advance();
        if (!zoom.isZooming()) return;
        cir.setReturnValue(cir.getReturnValue() * zoom.fovMultiplier());
    }
}
