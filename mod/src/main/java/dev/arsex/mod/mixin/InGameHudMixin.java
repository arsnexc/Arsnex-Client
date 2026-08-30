package dev.arsex.mod.mixin;

import dev.arsex.mod.ArsexMod;
import net.minecraft.client.gui.DrawContext;
import net.minecraft.client.gui.hud.InGameHud;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Draws the Arsex HUD over the vanilla one.
 *
 * Injected at RETURN so our overlay sits above vanilla elements but below any
 * open screen, which is where players expect a client HUD to live.
 *
 * Signature note: on 1.20.4, {@code InGameHud#render} takes
 * {@code (DrawContext context, float tickDelta)}. The RenderTickCounter
 * overload only exists from 1.20.5 onwards; the refmap confirms the 1.20.4
 * target is {@code method_1753(Lnet/minecraft/class_332;F)V}.
 */
@Mixin(InGameHud.class)
public class InGameHudMixin {

    @Inject(method = "render", at = @At("RETURN"))
    private void arsex$hud(DrawContext ctx, float tickDelta, CallbackInfo ci) {
        ArsexMod.renderHud(ctx);
    }
}
