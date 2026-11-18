package dev.horizon.android;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Insets;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.util.DisplayMetrics;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.widget.Button;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;
import android.window.SplashScreen;
import android.window.SplashScreenView;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

/**
 * Lightweight copyright-clean entry activity:
 * - If prepared resources already exist in internal storage (or a private local build bundled
 *   assets/resources.zip), immediately hands off to {@link HorizonActivity} with zero UI flash.
 * - Otherwise presents a landscape foolproof setup screen allowing the player to pick their own
 *   horizon-prepare generated {@code resources.zip} via Android's system document picker.
 */
public class HorizonLaunchActivity extends Activity {
    private static final int REQ_PICK_RESOURCES_ZIP = 1001;

    private TextView statusText;
    private ProgressBar progressBar;
    private Button pickButton;
    private volatile boolean importing = false;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        if (Build.VERSION.SDK_INT >= 31) {
            SplashScreen splashScreen = getSplashScreen();
            if (splashScreen != null) {
                splashScreen.setOnExitAnimationListener(new SplashScreen.OnExitAnimationListener() {
                    @Override
                    public void onSplashScreenExit(SplashScreenView splashScreenView) {
                        splashScreenView.remove();
                    }
                });
            }
        }
        super.onCreate(savedInstanceState);
        configureImmersiveWindow();

        if (isResourcesReady() || hasBundledAssetsZip()) {
            launchGameActivity();
            return;
        }

        buildSetupUi();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            configureImmersiveWindow();
        }
    }

    @SuppressWarnings("deprecation")
    private void configureImmersiveWindow() {
        final Window window = getWindow();
        if (window == null) return;
        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false);
            WindowInsetsController ic = window.getInsetsController();
            if (ic != null) {
                ic.setSystemBarsBehavior(
                    WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                );
                ic.hide(WindowInsets.Type.statusBars() | WindowInsets.Type.navigationBars());
            }
        } else {
            View decor = window.getDecorView();
            if (decor != null) {
                decor.setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                        | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                        | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_FULLSCREEN
                );
            }
        }
    }

    private File targetResourcesDir() {
        return new File(new File(getFilesDir(), "resources"), "26.3");
    }

    private boolean isResourcesReady() {
        File dir = targetResourcesDir();
        File marker = new File(dir, ".extracted_26_3");
        File manifest = new File(dir, "horizon-resources.json");
        return marker.isFile() && manifest.isFile() && manifest.length() > 16;
    }

    private boolean hasBundledAssetsZip() {
        try {
            InputStream is = getAssets().open("resources.zip");
            is.close();
            return true;
        } catch (Throwable ignored) {
            return false;
        }
    }

    private void launchGameActivity() {
        Intent intent = new Intent(this, HorizonActivity.class);
        startActivity(intent);
        overridePendingTransition(0, 0);
        finish();
    }

    private int dp(float v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }

    private GradientDrawable roundedBox(int fill, int stroke, float radiusDp) {
        GradientDrawable gd = new GradientDrawable();
        gd.setColor(fill);
        gd.setCornerRadius(dp(radiusDp));
        if (stroke != 0) {
            gd.setStroke(Math.max(1, dp(1f)), stroke);
        }
        return gd;
    }

    private void buildSetupUi() {
        DisplayMetrics dm = getResources().getDisplayMetrics();
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setGravity(Gravity.CENTER);
        root.setBackgroundColor(0xFF0D0D12);
        int hPad = Math.max(dp(24), Math.round(dm.widthPixels * 0.08f));
        root.setPadding(hPad, dp(16), hPad, dp(16));

        if (Build.VERSION.SDK_INT >= 30) {
            root.setOnApplyWindowInsetsListener(new View.OnApplyWindowInsetsListener() {
                @Override
                public WindowInsets onApplyWindowInsets(View v, WindowInsets insets) {
                    if (insets == null) return null;
                    Insets cutout = insets.getInsets(WindowInsets.Type.displayCutout());
                    v.setPadding(
                        Math.max(dp(24), cutout.left + dp(16)),
                        Math.max(dp(16), cutout.top + dp(8)),
                        Math.max(dp(24), cutout.right + dp(16)),
                        Math.max(dp(16), cutout.bottom + dp(8))
                    );
                    return insets;
                }
            });
        }

        ImageView iconView = new ImageView(this);
        iconView.setScaleType(ImageView.ScaleType.FIT_CENTER);
        try {
            int resId = getResources().getIdentifier("boot_logo", "drawable", getPackageName());
            if (resId == 0) resId = getApplicationInfo().icon;
            BitmapFactory.Options opts = new BitmapFactory.Options();
            opts.inScaled = false;
            Bitmap raw = BitmapFactory.decodeResource(getResources(), resId, opts);
            if (raw != null) {
                Bitmap crisp = Bitmap.createScaledBitmap(raw, dp(52), dp(52), false);
                iconView.setImageBitmap(crisp);
            }
        } catch (Throwable ignored) {
        }
        root.addView(iconView, new LinearLayout.LayoutParams(dp(52), dp(52)));

        TextView title = new TextView(this);
        title.setText("HORIZON");
        title.setTextColor(0xFFF8FAFC);
        title.setTextSize(22f);
        title.setTypeface(Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD));
        if (Build.VERSION.SDK_INT >= 21) {
            title.setLetterSpacing(0.14f);
        }
        title.setGravity(Gravity.CENTER);
        LinearLayout.LayoutParams titleLp = new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.WRAP_CONTENT,
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
        titleLp.topMargin = dp(10);
        root.addView(title, titleLp);

        TextView sub = new TextView(this);
        sub.setText("SGMY EDITION · 26.3（纯引擎无原版资产包）");
        sub.setTextColor(0xFF8A94A6);
        sub.setTextSize(11.5f);
        sub.setTypeface(Typeface.MONOSPACE);
        sub.setGravity(Gravity.CENTER);
        root.addView(sub);

        TextView guide = new TextView(this);
        guide.setText(
            "首次运行需导入你自己的正版 Minecraft: Java Edition 26.3 资源包：\n"
            + "1. 在已安装 26.3 的电脑上运行 cargo run --release -p horizon-prepare -- --prepare --complete\n"
            + "2. 将生成的资源目录压缩为 resources.zip 传至手机，点击下方按钮选择导入"
        );
        guide.setTextColor(0xFFCBD5E1);
        guide.setTextSize(12.5f);
        guide.setLineSpacing(dp(2), 1.05f);
        guide.setGravity(Gravity.CENTER);
        LinearLayout.LayoutParams guideLp = new LinearLayout.LayoutParams(
            Math.min(dm.widthPixels - dp(48), dp(560)),
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
        guideLp.topMargin = dp(14);
        root.addView(guide, guideLp);

        progressBar = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        progressBar.setIndeterminate(true);
        progressBar.setVisibility(View.GONE);
        LinearLayout.LayoutParams barLp = new LinearLayout.LayoutParams(
            Math.min(dm.widthPixels - dp(80), dp(340)),
            dp(8)
        );
        barLp.topMargin = dp(14);
        root.addView(progressBar, barLp);

        statusText = new TextView(this);
        statusText.setText("尚未导入游戏资源 (missing resources/26.3)");
        statusText.setTextColor(0xFF94A3B8);
        statusText.setTextSize(12f);
        statusText.setGravity(Gravity.CENTER);
        LinearLayout.LayoutParams stLp = new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.WRAP_CONTENT,
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
        stLp.topMargin = dp(8);
        root.addView(statusText, stLp);

        pickButton = new Button(this);
        pickButton.setAllCaps(false);
        pickButton.setText("选择手机内的 resources.zip 导入");
        pickButton.setTextColor(0xFFF0FDF4);
        pickButton.setTextSize(14f);
        pickButton.setTypeface(Typeface.DEFAULT_BOLD);
        pickButton.setPadding(dp(22), dp(10), dp(22), dp(10));
        pickButton.setBackground(roundedBox(0xFF166534, 0xFF4ADE80, 8f));
        pickButton.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                if (!importing) {
                    openDocumentPicker();
                }
            }
        });
        LinearLayout.LayoutParams btnLp = new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.WRAP_CONTENT,
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
        btnLp.topMargin = dp(14);
        root.addView(pickButton, btnLp);

        setContentView(root);
    }

    private void openDocumentPicker() {
        try {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            intent.putExtra(
                Intent.EXTRA_MIME_TYPES,
                new String[] {
                    "application/zip",
                    "application/x-zip-compressed",
                    "application/octet-stream"
                }
            );
            startActivityForResult(intent, REQ_PICK_RESOURCES_ZIP);
        } catch (Throwable e) {
            Intent fallback = new Intent(Intent.ACTION_GET_CONTENT);
            fallback.addCategory(Intent.CATEGORY_OPENABLE);
            fallback.setType("*/*");
            startActivityForResult(fallback, REQ_PICK_RESOURCES_ZIP);
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != REQ_PICK_RESOURCES_ZIP) return;
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            return;
        }
        final Uri uri = data.getData();
        startImportFromUri(uri);
    }

    private void startImportFromUri(final Uri uri) {
        importing = true;
        pickButton.setEnabled(false);
        pickButton.setAlpha(0.5f);
        progressBar.setVisibility(View.VISIBLE);
        progressBar.setIndeterminate(true);
        statusText.setTextColor(0xFF86EFAC);
        statusText.setText("正在解压并校验资源包，请勿关闭应用…");

        new Thread(new Runnable() {
            @Override
            public void run() {
                final File resourcesRoot = new File(getFilesDir(), "resources");
                final File stagingDir = new File(resourcesRoot, "26.3_staging");
                final File finalDir = targetResourcesDir();
                deleteRecursively(stagingDir);

                try {
                    if (!stagingDir.mkdirs() && !stagingDir.isDirectory()) {
                        throw new RuntimeException("无法创建内部存储解压目录");
                    }
                    String stagingCanonical = stagingDir.getCanonicalPath() + File.separator;
                    InputStream rawIn = getContentResolver().openInputStream(uri);
                    if (rawIn == null) {
                        throw new RuntimeException("无法读取所选文件");
                    }

                    int filesExtracted = 0;
                    byte[] buffer = new byte[64 * 1024];
                    ZipInputStream zis = new ZipInputStream(new BufferedInputStream(rawIn, 64 * 1024));
                    try {
                        ZipEntry entry;
                        while ((entry = zis.getNextEntry()) != null) {
                            String name = entry.getName();
                            if (name == null || name.isEmpty() || name.startsWith("__MACOSX/")) {
                                zis.closeEntry();
                                continue;
                            }
                            File out = new File(stagingDir, name);
                            String outCanonical = out.getCanonicalPath();
                            if (!outCanonical.startsWith(stagingCanonical)) {
                                throw new SecurityException("拒绝不安全的压缩包路径: " + name);
                            }
                            if (entry.isDirectory()) {
                                out.mkdirs();
                            } else {
                                File parent = out.getParentFile();
                                if (parent != null && !parent.isDirectory()) {
                                    parent.mkdirs();
                                }
                                BufferedOutputStream bos = new BufferedOutputStream(
                                    new FileOutputStream(out),
                                    64 * 1024
                                );
                                try {
                                    int n;
                                    while ((n = zis.read(buffer)) > 0) {
                                        bos.write(buffer, 0, n);
                                    }
                                } finally {
                                    bos.close();
                                }
                                filesExtracted++;
                                if (filesExtracted % 150 == 0) {
                                    final int count = filesExtracted;
                                    runOnUiThread(new Runnable() {
                                        @Override
                                        public void run() {
                                            statusText.setText("正在解压游戏资源 (已解压 " + count + " 个文件)…");
                                        }
                                    });
                                }
                            }
                            zis.closeEntry();
                        }
                    } finally {
                        zis.close();
                    }

                    File manifestDir = findHorizonResourcesRoot(stagingDir, 0);
                    if (manifestDir == null) {
                        deleteRecursively(stagingDir);
                        throw new RuntimeException(
                            "压缩包中未找到 horizon-resources.json，请确认它是由 horizon-prepare 生成的 26.3 资源目录"
                        );
                    }

                    deleteRecursively(finalDir);
                    if (!manifestDir.renameTo(finalDir)) {
                        throw new RuntimeException("移动解压目录失败");
                    }
                    deleteRecursively(stagingDir);

                    FileOutputStream markerOut = new FileOutputStream(new File(finalDir, ".extracted_26_3"));
                    markerOut.write(new byte[] {'o', 'k'});
                    markerOut.close();

                    runOnUiThread(new Runnable() {
                        @Override
                        public void run() {
                            statusText.setText("资源导入完成，正在启动 Horizon…");
                            launchGameActivity();
                        }
                    });
                } catch (final Throwable err) {
                    deleteRecursively(stagingDir);
                    runOnUiThread(new Runnable() {
                        @Override
                        public void run() {
                            importing = false;
                            pickButton.setEnabled(true);
                            pickButton.setAlpha(1.0f);
                            progressBar.setVisibility(View.GONE);
                            statusText.setTextColor(0xFFFCA5A5);
                            statusText.setText("导入失败: " + err.getMessage());
                        }
                    });
                }
            }
        }, "horizon-res-import").start();
    }

    private static File findHorizonResourcesRoot(File dir, int depth) {
        if (dir == null || !dir.isDirectory() || depth > 3) return null;
        File manifest = new File(dir, "horizon-resources.json");
        if (manifest.isFile() && manifest.length() > 16) {
            return dir;
        }
        File[] children = dir.listFiles();
        if (children == null) return null;
        for (File child : children) {
            if (child.isDirectory() && !child.getName().startsWith(".")) {
                File found = findHorizonResourcesRoot(child, depth + 1);
                if (found != null) return found;
            }
        }
        return null;
    }

    private static void deleteRecursively(File file) {
        if (file == null || !file.exists()) return;
        if (file.isDirectory()) {
            File[] children = file.listFiles();
            if (children != null) {
                for (File c : children) {
                    deleteRecursively(c);
                }
            }
        }
        file.delete();
    }
}
