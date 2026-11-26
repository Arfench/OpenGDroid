package com.opengdroid.core;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.Arrays;

/** On-device verification of the JNI bridge. Prints PASS/FAIL per assertion. */
public final class CoreTest {
    private static int failures = 0;

    private static void check(boolean cond, String what) {
        System.out.println("  [" + (cond ? "PASS" : "FAIL") + "] " + what);
        if (!cond) failures++;
    }

    /** Builds a minimal RAW GMA7 archive in memory. */
    private static byte[] buildGma(boolean withBadPath) {
        java.io.ByteArrayOutputStream b = new java.io.ByteArrayOutputStream();

        putU32(b, 1145130311); // "GMA7"
        b.write(1);            // version
        putI64(b, 76561197960265728L);
        putI64(b, 1700000000L);
        putStr(b, "Test Addon");
        putStr(b, "desc");
        putStr(b, "tester");
        putU32(b, 7);

        putU32(b, 1);
        putStr(b, "lua/autorun/init.lua");
        putI64(b, 4);
        putU32(b, 0xDEADBEEF);

        if (withBadPath) {
            putU32(b, 2);
            putStr(b, "bin/linux64/libengine.so");
            putI64(b, 3);
            putU32(b, 0x12345678);
        }
        putU32(b, 0);

        b.writeBytes("abcd".getBytes(StandardCharsets.US_ASCII));
        if (withBadPath) {
            b.writeBytes("xyz".getBytes(StandardCharsets.US_ASCII));
        }
        putU32(b, 0xCAFEF00D);
        return b.toByteArray();
    }

    private static void putU32(java.io.ByteArrayOutputStream b, long v) {
        b.write((int) ((v >> 24) & 0xFF));
        b.write((int) ((v >> 16) & 0xFF));
        b.write((int) ((v >> 8) & 0xFF));
        b.write((int) (v & 0xFF));
    }

    private static void putI64(java.io.ByteArrayOutputStream b, long v) {
        for (int i = 7; i >= 0; i--) b.write((int) ((v >> (i * 8)) & 0xFF));
    }

    private static void putStr(java.io.ByteArrayOutputStream b, String s) {
        b.writeBytes(s.getBytes(StandardCharsets.UTF_8));
        b.write(0);
    }

    public static void main(String[] args) throws Exception {
        String dir = "/data/local/tmp/jdir";
        new java.io.File(dir).mkdirs();

        System.out.println("== version ==");
        String v = Core.version();
        System.out.println("  " + v);
        check(v != null && !v.isEmpty(), "version non-empty");

        System.out.println("== sharedRoot ==");
        String root = Core.sharedRoot("/storage/emulated/0");
        System.out.println("  " + root);
        check(root != null && root.contains("srceng"), "shared root resolved");

        System.out.println("== buildArgs ==");
        String cmdline = Core.buildArgs("garrysmod", 1520, 720, true, 1024, 1, "+map gm_construct");
        System.out.println("  " + cmdline);
        check(cmdline != null && cmdline.contains("-game garrysmod"), "game dir present");
        check(cmdline != null && cmdline.contains("-heapsize 1024"), "heap size present");
        check(cmdline != null && cmdline.contains("-w 1520"), "width present");
        check(cmdline != null && cmdline.contains("-sourcevr_native_vulkan"), "vulkan token present");
        check(cmdline != null && cmdline.endsWith("+map gm_construct"), "extras appended last");

        String glesArgs = Core.buildArgs("garrysmod", 800, 600, false, 512, 0, "");
        check(!glesArgs.contains("vulkan"), "GLES path omits vulkan token");
        check(!glesArgs.contains("-fullscreen"), "windowed omits fullscreen");

        System.out.println("== localization ==");
        byte[] utf8 = "\"A\" \"Alpha\"\n\"B\" \"Beta\"\n".getBytes(StandardCharsets.UTF_8);
        check(Core.localizationCount(utf8) == 2, "UTF-8 table has 2 entries");
        check("Alpha".equals(Core.localizationLookup(utf8, "A")), "UTF-8 lookup works");
        check(Core.localizationLookup(utf8, "ZZZ") == null, "missing key returns null");

        // UTF-16LE with BOM: the case the original launcher reads as zero entries.
        byte[] text = "\"A\" \"Alpha\"\n".getBytes(StandardCharsets.UTF_16LE);
        byte[] utf16 = new byte[text.length + 2];
        utf16[0] = (byte) 0xFF;
        utf16[1] = (byte) 0xFE;
        System.arraycopy(text, 0, utf16, 2, text.length);
        check(Core.localizationCount(utf16) == 1, "UTF-16LE table decoded");
        check("Alpha".equals(Core.localizationLookup(utf16, "A")), "UTF-16LE lookup works");

        check(Core.localizationCount(new byte[] {(byte) 0xFF, (byte) 0xFE, 0x41}) == -1,
                "truncated UTF-16 rejected");

        System.out.println("== renderer ==");
        String j = Core.rendererLaunch(dir, false, 4198400);
        System.out.println("  " + j);
        check(j != null && j.contains("\"vulkan\":false"), "GLES when not requested");

        j = Core.rendererLaunch(dir, true, 4198400);
        System.out.println("  " + j);
        check(j != null && j.contains("\"vulkan\":true"), "Vulkan accepted when capable");

        j = Core.rendererLaunch(dir, true, 4198400);
        System.out.println("  " + j);
        check(j != null && j.contains("\"vulkan\":false"), "pending marker forces GLES");
        check(j != null && j.contains("closed before its first frame"), "crash notice present");

        check(Core.rendererClear(dir) == 0, "marker cleared");
        j = Core.rendererLaunch(dir, true, 4198400);
        check(j != null && j.contains("\"vulkan\":true"), "Vulkan retried after clear");
        Core.rendererClear(dir);

        j = Core.rendererLaunch(dir, true, 4000000);
        check(j != null && j.contains("\"vulkan\":false"), "old device declined");
        check(j != null && j.contains("does not report Vulkan 1.1"), "reason recorded");
        Core.rendererClear(dir);

        System.out.println("== gma ==");
        byte[] gma = buildGma(false);
        check(Core.gmaCount(gma) == 1, "one file counted");
        String info = Core.gmaInfo(gma);
        System.out.println("  " + info);
        check(info != null && info.contains("\"title\":\"Test Addon\""), "title parsed");
        check(info != null && info.contains("\"allowed\":1"), "allowed count reported");

        byte[] gmaBad = buildGma(true);
        check(Core.gmaCount(gmaBad) == 2, "two files counted");
        info = Core.gmaInfo(gmaBad);
        System.out.println("  " + info);
        check(info != null && info.contains("\"rejected\":1"), "engine library rejected");

        check(Core.gmaPathAllowed("lua/autorun/x.lua") == 1, "lua allowed");
        check(Core.gmaPathAllowed("maps/gm_construct.bsp") == 1, "map allowed");
        check(Core.gmaPathAllowed("bin/libengine.so") == 0, "engine library rejected");
        check(Core.gmaPathAllowed("../../etc/passwd") == 0, "traversal rejected");
        check(Core.gmaCount("not an addon".getBytes(StandardCharsets.US_ASCII)) == -1,
                "garbage rejected");
        check(Core.gmaInfo("not an addon".getBytes(StandardCharsets.US_ASCII)) == null,
                "garbage info is null");

        System.out.println();
        System.out.println(failures == 0 ? "RESULT: ALL PASS" : "RESULT: FAILURES " + failures);
        System.exit(failures == 0 ? 0 : 1);
    }
}
