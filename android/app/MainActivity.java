package com.opengdroid.app;

import android.Manifest;
import android.app.Activity;
import android.content.pm.FeatureInfo;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import com.opengdroid.core.Core;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

/**
 * Minimal launcher screen.
 *
 * <p>Shows what the Rust core resolves for this device: content root, engine command line and the
 * renderer decision. This is the diagnostic half of the launcher; starting the engine needs the
 * game content present, which is checked and reported rather than assumed.
 */
public final class MainActivity extends Activity {
    private static final String TAG = "OpenGDroid";
    private static final int REQ_STORAGE = 1;

    private TextView output;
    private final List<String> lines = new ArrayList<>();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        int pad = (int) (16 * getResources().getDisplayMetrics().density);
        root.setPadding(pad, pad, pad, pad);

        Button refresh = new Button(this);
        refresh.setText("Обновить");
        refresh.setOnClickListener(v -> runChecks());
        root.addView(refresh);

        Button saveCfg = new Button(this);
        saveCfg.setText("Записать android_settings.cfg");
        saveCfg.setOnClickListener(v -> writeConfig());
        root.addView(saveCfg);

        ScrollView scroll = new ScrollView(this);
        output = new TextView(this);
        output.setTextSize(12);
        output.setTypeface(android.graphics.Typeface.MONOSPACE);
        scroll.addView(output);
        root.addView(scroll, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f));

        setContentView(root);

        if (Build.VERSION.SDK_INT >= 23
                && checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE)
                        != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[] {Manifest.permission.READ_EXTERNAL_STORAGE}, REQ_STORAGE);
        }

        runChecks();
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(code, permissions, results);
        runChecks();
    }

    /** Resolves everything the launcher needs and shows it. */
    private void runChecks() {
        lines.clear();

        say("Ядро: " + Core.version());
        say("Устройство: " + Build.MANUFACTURER + " " + Build.MODEL
                + " / Android " + Build.VERSION.RELEASE + " (API " + Build.VERSION.SDK_INT + ")");

        String external = Environment.getExternalStorageDirectory().getAbsolutePath();
        String sharedRoot = Core.sharedRoot(external);
        say("Корень контента: " + sharedRoot);
        File gameDir = new File(sharedRoot, "garrysmod");
        say("Каталог игры: " + gameDir.getAbsolutePath()
                + (gameDir.isDirectory() ? " (найден)" : " (НЕ найден)"));

        DisplayMetrics dm = getResources().getDisplayMetrics();
        int width = Math.max(dm.widthPixels, dm.heightPixels);
        int height = Math.min(dm.widthPixels, dm.heightPixels);

        int vulkanVersion = vulkanVersion();
        say("Vulkan: " + (vulkanVersion >= 4198400
                ? "1.1+ (" + vulkanVersion + ")" : "нет/старый (" + vulkanVersion + ")"));

        String rendererJson = Core.rendererLaunch(getFilesDir().getAbsolutePath(), false, vulkanVersion);
        say("Рендерер: " + rendererJson);

        String cmdline = Core.buildArgs(gameDir.getAbsolutePath(), width, height,
                true, 1024, 0, "");
        say("Команда: " + cmdline);

        File addonDir = new File(sharedRoot, "garrysmod/addons");
        File[] entries = addonDir.listFiles();
        int gmaCount = 0;
        int workshopCount = 0;
        if (entries != null) {
            for (File entry : entries) {
                if (entry.isFile() && entry.getName().endsWith(".gma")) {
                    gmaCount++;
                } else if (entry.isDirectory()) {
                    // Steam Workshop items land as numeric directories.
                    workshopCount++;
                }
            }
        }
        say("Аддоны: .gma " + gmaCount + ", workshop-каталогов " + workshopCount);

        render();
    }

    /** Reads android.hardware.vulkan.version from the platform feature list. */
    private int vulkanVersion() {
        PackageManager pm = getPackageManager();
        for (FeatureInfo feature : pm.getSystemAvailableFeatures()) {
            if ("android.hardware.vulkan.version".equals(feature.name)) {
                return feature.version;
            }
        }
        return 0;
    }

    /** Writes the generated config where the engine expects it. */
    private void writeConfig() {
        String sharedRoot = Core.sharedRoot(Environment.getExternalStorageDirectory().getAbsolutePath());
        File cfgDir = new File(sharedRoot, "garrysmod/cfg");
        File cfg = new File(cfgDir, "android_settings.cfg");

        String body = "// Generated by OpenGDroid.\n"
                + "fps_max 60\n"
                + "touch_enable 1\n";

        try {
            if (!cfgDir.isDirectory() && !cfgDir.mkdirs()) {
                say("Не удалось создать " + cfgDir);
                render();
                return;
            }
            try (FileOutputStream out = new FileOutputStream(cfg)) {
                out.write(body.getBytes(StandardCharsets.UTF_8));
            }
            say("Записано: " + cfg.getAbsolutePath() + " (" + cfg.length() + " байт)");
        } catch (IOException e) {
            say("Ошибка записи: " + e.getMessage());
        }
        render();
    }

    private void say(String line) {
        lines.add(line);
        Log.i(TAG, line);
    }

    private void render() {
        StringBuilder sb = new StringBuilder();
        for (String line : lines) {
            sb.append(line).append('\n');
        }
        output.setText(sb.toString());
    }
}
