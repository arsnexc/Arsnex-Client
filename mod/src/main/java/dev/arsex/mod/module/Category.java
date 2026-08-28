package dev.arsex.mod.module;

/** Module categories. Declaration order is the tab order in the Click GUI. */
public enum Category {
    COMBAT("Combat", "\u6226"),
    VISUAL("Visual", "\u898b"),
    PERFORMANCE("Performance", "\u901f"),
    HUD("HUD", "\u8868"),
    MOVEMENT("Movement", "\u52d5"),
    UTILITY("Utility", "\u5177");

    public final String label;
    public final String kanji;

    Category(String label, String kanji) {
        this.label = label;
        this.kanji = kanji;
    }
}
