# UniFFI generated bindings and the Rust bridge are accessed through JNA.
-keep class uniffi.plume_ffi.** { *; }
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }

# ARCore uses reflection for optional features.
-keep class com.google.ar.core.** { *; }
-dontwarn com.google.ar.core.**
