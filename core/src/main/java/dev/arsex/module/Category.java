package dev.arsex.module;

/** Module categories. Order here is the order shown in the Click GUI. */
public enum Category {
    COMBAT("Combat", "\u6226"),
    VISUAL("Visual", "\u898b"),
    PERFORMANCE("Performance", "\u901f"),
    HUD("HUD", "\u8868"),
    MOVEMENT("Movement", "\u52d5"),
    SOCIAL("Social", "\u4ea4"),
    UTILITY("Utility", "\u5177"),
    SYSTEM("System", "\u7cfb");

    public final String label;
    public final String kanji;

    Category(String label, String kanji) {
        this.label = label;
        this.kanji = kanji;
    }
}
