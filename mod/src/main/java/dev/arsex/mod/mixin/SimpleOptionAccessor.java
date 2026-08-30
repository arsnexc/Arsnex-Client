package dev.arsex.mod.mixin;

import net.minecraft.client.option.SimpleOption;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Accessor;

/**
 * Direct access to a {@link SimpleOption}'s backing value.
 *
 * Why this exists: since 1.19.4 every option is a SimpleOption, and
 * {@code setValue} routes through the option's callbacks:
 *
 * <pre>{@code
 * this.value = callbacks.validate(value).orElseGet(() -> this.value);
 * }</pre>
 *
 * Gamma's callbacks are {@code DoubleSliderCallbacks}, whose validate()
 * returns Optional.empty() for anything outside [0.0, 1.0] — verified against
 * the 1.20.4 jar. So {@code options.getGamma().setValue(10.0)} is *silently
 * ignored*, which is exactly why a naive fullbright does nothing on 1.20.4.
 *
 * Fullbright writes the field directly through this accessor when the target
 * is above the slider cap, and restores the original through it on disable.
 */
@Mixin(SimpleOption.class)
public interface SimpleOptionAccessor<T> {

    @Accessor("value")
    void arsex$setValueDirect(T value);

    @Accessor("value")
    T arsex$getValueDirect();
}
