# JNA (UniFFI bindings): classes and members looked up by name from native code.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-dontwarn java.awt.**
# UniFFI bindings of crates/ffi: structures passed by value through JNA.
-keep class fr.skimap.core.** { *; }
