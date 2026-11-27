package com.opengdroid.core;

/**
 * Kotlin/Java side of the native bridge.
 *
 * <p>Every method maps to a {@code Java_com_opengdroid_core_Core_*} symbol exported by
 * {@code libopengdroid.so}. Strings are UTF-8 on the Rust side; byte arrays are used for file
 * payloads so no encoding round-trip happens in Java.
 */
public final class Core {
    static {
        System.loadLibrary("opengdroid");
    }

    private Core() {}

    /** Crate version, for diagnostics. */
    public static native String version();

    /** Resolves the game-content root beneath external storage. */
    public static native String sharedRoot(String externalRoot);

    /**
     * Builds the engine command line.
     *
     * @param rendererId 1 for Vulkan, anything else for OpenGL ES
     */
    public static native String buildArgs(
            String gameDir,
            int width,
            int height,
            boolean fullscreen,
            int heapMb,
            int rendererId,
            String extras);

    /** Number of entries in a localization table, or -1 if it cannot be decoded. */
    public static native long localizationCount(byte[] payload);

    /** Value for a localization key, or null when absent or undecodable. */
    public static native String localizationLookup(byte[] payload, String key);

    /**
     * Renderer decision for this launch as JSON: {@code {"vulkan":bool,"notice":string|null}}.
     *
     * @param deviceVersion value of {@code android.hardware.vulkan.version}; 4198400 is Vulkan 1.1
     */
    public static native String rendererLaunch(String filesDir, boolean requested, int deviceVersion);

    /** Clears the renderer fallback marker: 0 ok, -1 bad argument, -2 I/O error. */
    public static native int rendererClear(String filesDir);

    /** Number of files in a .gma archive, or -1 when the payload is not a plain archive. */
    public static native long gmaCount(byte[] payload);

    /** Archive metadata as JSON, or null when the payload cannot be parsed. */
    public static native String gmaInfo(byte[] payload);

    /** 1 when the addon path is allowed, 0 when rejected, -1 on a bad argument. */
    public static native int gmaPathAllowed(String path);

    /**
     * Full launch plan as JSON: command line, environment and readiness.
     *
     * <p>Shape: {@code {"ready":bool,"game_installed":bool,"blockers":[string],
     * "missing_entry_libraries":[string],"missing_engine_libraries":[string],
     * "command":string,"args":[string],"env":[string]}}.
     *
     * @param rendererId 1 for Vulkan, anything else for OpenGL ES
     */
    public static native String launchPlan(
            String sharedRoot,
            String gameId,
            String nativeLibDir,
            String userDir,
            String engineContent,
            String runtimeVpk,
            String cacheDir,
            int width,
            int height,
            boolean fullscreen,
            int heapMb,
            int rendererId,
            String extras);

    /**
     * Loads the engine entry libraries with {@code dlopen} and reports the outcome.
     *
     * <p>Shape: {@code {"all_loaded":bool,"entry_symbol":bool,"failed":[string],
     * "errors":[string],"libraries":[{"name":string,"loaded":bool,
     * "error":string|null,"entry_symbol":bool}]}}.
     *
     * <p>Libraries are unloaded again, so the call has no lasting effect.
     */
    public static native String engineProbe(String libraryDir);
}
