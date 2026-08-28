package dev.arsex.mod.mixin;

import dev.arsex.mod.ArsexMod;
import net.minecraft.client.gui.DrawContext;
import net.minecraft.client.gui.hud.InGameHud;
import net.minecraft.client.render.RenderTickCounter;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Draws the Arsex HUD over the vanilla one.
 *
 * Injected at RETURN so our overlay sits above vanilla elements but below any
 * open screen, which is where players expect a client HUD to live.
 */
@Mixin(InGameHud.class)
public class InGameHudMixin {

    @Inject(method = "render", at = @At("RETURN"))
    private void arsex$hud(DrawContext ctx, RenderTickCounter counter, CallbackInfo ci) {
        ArsexMod.renderHud(ctx);
    }
}
