package dev.horizon.android;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Insets;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.os.Build;
import android.os.Bundle;
import android.os.SystemClock;
import android.text.InputType;
import android.util.DisplayMetrics;
import android.view.DisplayCutout;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;
import android.widget.Toast;
import android.window.SplashScreen;
import android.window.SplashScreenView;

public class HorizonActivity extends NativeActivity {
    private InputBridgeView imeView;
    private BootSplashView bootSplashView;
    private ClipboardManager clipboardManager;
    private volatile boolean imeRequested = false;
    private volatile String latestBootStage = "正在启动 Horizon…";
    private volatile int latestBootPercent = 4;
    private volatile boolean bootDismissed = false;

    private static native void nativeOnImeCommit(String text);
    private static native void nativeOnImePreedit(String text);
    private static native void nativeOnImeKey(int keyCode);
    private static native void nativeOnWindowInsets(
        int cutoutLeft,
        int cutoutTop,
        int cutoutRight,
        int cutoutBottom,
        int gestureLeft,
        int gestureRight,
        int imeBottom
    );

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
        clipboardManager = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        imeView = new InputBridgeView(this);
        imeView.setFocusable(true);
        imeView.setFocusableInTouchMode(true);
        addContentView(imeView, new ViewGroup.LayoutParams(1, 1));

        if (!bootDismissed) {
            bootSplashView = new BootSplashView(this);
            addContentView(
                bootSplashView,
                new ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT
                )
            );
        }

        configureImmersiveWindow();
        installInsetsListener();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus && !imeRequested) {
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

    @SuppressWarnings("deprecation")
    private void installInsetsListener() {
        final Window window = getWindow();
        if (window == null) return;
        final View decor = window.getDecorView();
        if (decor == null) return;
        decor.setOnApplyWindowInsetsListener(new View.OnApplyWindowInsetsListener() {
            @Override
            public WindowInsets onApplyWindowInsets(View v, WindowInsets insets) {
                if (insets == null) return null;
                int cLeft = 0, cTop = 0, cRight = 0, cBottom = 0;
                int gLeft = 0, gRight = 0;
                int imeBottom = 0;
                if (Build.VERSION.SDK_INT >= 30) {
                    Insets cutout = insets.getInsets(WindowInsets.Type.displayCutout());
                    cLeft = cutout.left;
                    cTop = cutout.top;
                    cRight = cutout.right;
                    cBottom = cutout.bottom;
                    Insets gesture = insets.getInsets(WindowInsets.Type.systemGestures());
                    gLeft = gesture.left;
                    gRight = gesture.right;
                    Insets ime = insets.getInsets(WindowInsets.Type.ime());
                    imeBottom = ime.bottom;
                } else {
                    DisplayCutout dc = insets.getDisplayCutout();
                    if (dc != null) {
                        cLeft = dc.getSafeInsetLeft();
                        cTop = dc.getSafeInsetTop();
                        cRight = dc.getSafeInsetRight();
                        cBottom = dc.getSafeInsetBottom();
                    }
                    if (Build.VERSION.SDK_INT >= 29) {
                        Insets gesture = insets.getSystemGestureInsets();
                        gLeft = gesture.left;
                        gRight = gesture.right;
                    }
                    Rect visible = new Rect();
                    v.getWindowVisibleDisplayFrame(visible);
                    int rootH = v.getRootView().getHeight();
                    if (rootH > 0 && rootH - visible.bottom > rootH / 6) {
                        imeBottom = rootH - visible.bottom;
                    }
                }
                nativeOnWindowInsets(cLeft, cTop, cRight, cBottom, gLeft, gRight, imeBottom);
                return insets;
            }
        });
    }

    public void updateBootProgress(final String stage, final int percent) {
        if (stage != null && !stage.isEmpty()) {
            latestBootStage = stage;
        }
        int clamped = Math.max(0, Math.min(100, percent));
        if (clamped > latestBootPercent || percent == 0) {
            latestBootPercent = clamped;
        }
        if (bootDismissed) return;
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                if (bootSplashView != null && !bootDismissed) {
                    bootSplashView.invalidate();
                }
            }
        });
    }

    public void dismissBootSplash() {
        if (bootDismissed) return;
        bootDismissed = true;
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                final BootSplashView view = bootSplashView;
                bootSplashView = null;
                if (view == null) return;
                view.setClickable(false);
                view.setFocusable(false);
                view.animate()
                    .alpha(0f)
                    .setDuration(180L)
                    .withEndAction(new Runnable() {
                        @Override
                        public void run() {
                            view.setVisibility(View.GONE);
                            ViewParent parent = view.getParent();
                            if (parent instanceof ViewGroup) {
                                ((ViewGroup) parent).removeView(view);
                            }
                        }
                    })
                    .start();
            }
        });
    }

    public void setImeActive(final boolean active) {
        imeRequested = active;
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                if (imeView == null) return;
                final InputMethodManager imm =
                    (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
                if (active) {
                    imeView.requestFocus();
                    imeView.post(new Runnable() {
                        @Override
                        public void run() {
                            if (imm != null) {
                                imm.restartInput(imeView);
                                imm.showSoftInput(imeView, InputMethodManager.SHOW_IMPLICIT);
                            }
                            if (Build.VERSION.SDK_INT >= 30) {
                                WindowInsetsController ic = imeView.getWindowInsetsController();
                                if (ic != null) {
                                    ic.show(WindowInsets.Type.ime());
                                }
                            }
                        }
                    });
                } else {
                    if (imm != null) {
                        imm.hideSoftInputFromWindow(imeView.getWindowToken(), 0);
                    }
                    if (Build.VERSION.SDK_INT >= 30) {
                        WindowInsetsController ic = imeView.getWindowInsetsController();
                        if (ic != null) {
                            ic.hide(WindowInsets.Type.ime());
                        }
                    }
                    configureImmersiveWindow();
                }
            }
        });
    }

    public void setKeepScreenOn(final boolean keepOn) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                if (getWindow() == null) return;
                if (keepOn) {
                    getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
                } else {
                    getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
                }
            }
        });
    }

    public String getClipboardText() {
        try {
            if (clipboardManager == null || !clipboardManager.hasPrimaryClip()) {
                return "";
            }
            ClipData clip = clipboardManager.getPrimaryClip();
            if (clip == null || clip.getItemCount() == 0) {
                return "";
            }
            CharSequence text = clip.getItemAt(0).coerceToText(this);
            return text != null ? text.toString() : "";
        } catch (Throwable ignored) {
            return "";
        }
    }

    public void setClipboardText(final String text) {
        if (text == null) return;
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    if (clipboardManager != null) {
                        clipboardManager.setPrimaryClip(ClipData.newPlainText("Horizon", text));
                    }
                } catch (Throwable ignored) {
                }
            }
        });
    }

    public void showToast(final String message) {
        if (message == null || message.isEmpty()) return;
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                Toast.makeText(HorizonActivity.this, message, Toast.LENGTH_SHORT).show();
            }
        });
    }

    private final class BootSplashView extends View {
        private final Paint bgPaint = new Paint();
        private final Paint iconPaint = new Paint();
        private final Paint titlePaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint subPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint stagePaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint hintPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint barBgPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint barFillPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint barPulsePaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final RectF trackRect = new RectF();
        private final RectF fillRect = new RectF();
        private final RectF pulseRect = new RectF();
        private final Rect iconSrc = new Rect();
        private final RectF iconDst = new RectF();
        private final float dp;
        private final float sp;
        private Bitmap logoBitmap;
        private float displayedPercent = 4f;

        BootSplashView(Context context) {
            super(context);
            DisplayMetrics dm = context.getResources().getDisplayMetrics();
            dp = dm.density;
            sp = dm.scaledDensity;
            setClickable(true);
            setFocusable(true);

            bgPaint.setColor(0xFF0D0D12);
            iconPaint.setFilterBitmap(false);
            iconPaint.setAntiAlias(false);

            titlePaint.setColor(0xFFF8FAFC);
            titlePaint.setTypeface(Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD));
            titlePaint.setTextAlign(Paint.Align.CENTER);
            titlePaint.setTextSize(24f * sp);
            if (Build.VERSION.SDK_INT >= 21) {
                titlePaint.setLetterSpacing(0.14f);
            }

            subPaint.setColor(0xFF8A94A6);
            subPaint.setTypeface(Typeface.MONOSPACE);
            subPaint.setTextAlign(Paint.Align.CENTER);
            subPaint.setTextSize(11.5f * sp);

            stagePaint.setColor(0xFFD6DEE8);
            stagePaint.setTextAlign(Paint.Align.CENTER);
            stagePaint.setTextSize(13f * sp);

            hintPaint.setColor(0xFF64748B);
            hintPaint.setTextAlign(Paint.Align.CENTER);
            hintPaint.setTextSize(11f * sp);

            barBgPaint.setColor(0xFF1E2633);
            barFillPaint.setColor(0xFF4ADE80);
            barPulsePaint.setColor(0x55FFFFFF);

            try {
                int resId = context.getResources().getIdentifier(
                    "boot_logo",
                    "drawable",
                    context.getPackageName()
                );
                if (resId == 0) {
                    resId = context.getApplicationInfo().icon;
                }
                if (resId != 0) {
                    BitmapFactory.Options opts = new BitmapFactory.Options();
                    opts.inScaled = false;
                    logoBitmap = BitmapFactory.decodeResource(context.getResources(), resId, opts);
                    if (logoBitmap != null) {
                        iconSrc.set(0, 0, logoBitmap.getWidth(), logoBitmap.getHeight());
                    }
                }
            } catch (Throwable ignored) {
            }
        }

        @Override
        public boolean onTouchEvent(MotionEvent event) {
            // Foolproof guard: swallow stray touches while unpacking resources or compiling shaders.
            return !bootDismissed;
        }

        @Override
        protected void onDraw(Canvas canvas) {
            int w = getWidth();
            int h = getHeight();
            if (w <= 0 || h <= 0) return;

            canvas.drawRect(0f, 0f, w, h, bgPaint);

            float cx = w * 0.5f;
            float cy = h * 0.5f;
            float iconSize = Math.min(Math.min(w, h) * 0.16f, 56f * dp);
            float blockTop = cy - 68f * dp;

            if (logoBitmap != null && !logoBitmap.isRecycled()) {
                iconDst.set(
                    cx - iconSize * 0.5f,
                    blockTop,
                    cx + iconSize * 0.5f,
                    blockTop + iconSize
                );
                canvas.drawBitmap(logoBitmap, iconSrc, iconDst, iconPaint);
            }

            float titleY = blockTop + iconSize + 30f * dp;
            canvas.drawText("HORIZON", cx, titleY, titlePaint);
            canvas.drawText("SGMY EDITION · 26.3", cx, titleY + 18f * dp, subPaint);

            float target = Math.max(0f, Math.min(100f, (float) latestBootPercent));
            if (displayedPercent < target) {
                displayedPercent = Math.min(target, displayedPercent + Math.max(0.6f, (target - displayedPercent) * 0.22f));
            } else {
                displayedPercent = target;
            }

            float barW = Math.min(w * 0.46f, 320f * dp);
            float barH = 6f * dp;
            float barR = barH * 0.5f;
            float barLeft = cx - barW * 0.5f;
            float barTop = titleY + 38f * dp;
            trackRect.set(barLeft, barTop, barLeft + barW, barTop + barH);
            canvas.drawRoundRect(trackRect, barR, barR, barBgPaint);

            float fillW = Math.max(barH, barW * (displayedPercent / 100f));
            fillRect.set(barLeft, barTop, Math.min(barLeft + barW, barLeft + fillW), barTop + barH);
            canvas.drawRoundRect(fillRect, barR, barR, barFillPaint);

            // Subtle moving shimmer so long shader compilation never looks frozen.
            float phase = (SystemClock.uptimeMillis() % 1400L) / 1400f;
            float pulseW = Math.min(fillRect.width() * 0.35f, 48f * dp);
            if (pulseW > 4f * dp) {
                float pLeft = fillRect.left + (fillRect.width() - pulseW) * phase;
                pulseRect.set(pLeft, barTop, pLeft + pulseW, barTop + barH);
                canvas.drawRoundRect(pulseRect, barR, barR, barPulsePaint);
            }

            canvas.drawText(latestBootStage, cx, barTop + barH + 22f * dp, stagePaint);
            canvas.drawText("正在准备引擎与着色器，请稍候…", cx, barTop + barH + 40f * dp, hintPaint);

            if (!bootDismissed) {
                postInvalidateDelayed(33L);
            }
        }
    }

    private static final class InputBridgeView extends View {
        InputBridgeView(Context context) {
            super(context);
        }

        @Override
        public boolean onCheckIsTextEditor() {
            return true;
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
            outAttrs.inputType = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
            outAttrs.imeOptions = EditorInfo.IME_ACTION_SEND | EditorInfo.IME_FLAG_NO_EXTRACT_UI;
            return new BaseInputConnection(this, false) {
                @Override
                public boolean commitText(CharSequence text, int newCursorPosition) {
                    if (text != null && text.length() > 0) {
                        nativeOnImeCommit(text.toString());
                    }
                    return true;
                }

                @Override
                public boolean setComposingText(CharSequence text, int newCursorPosition) {
                    nativeOnImePreedit(text != null ? text.toString() : "");
                    return true;
                }

                @Override
                public boolean finishComposingText() {
                    nativeOnImePreedit("");
                    return true;
                }

                @Override
                public boolean performEditorAction(int actionCode) {
                    nativeOnImePreedit("");
                    nativeOnImeKey(KeyEvent.KEYCODE_ENTER);
                    return true;
                }

                @Override
                public boolean deleteSurroundingText(int beforeLength, int afterLength) {
                    for (int i = 0; i < beforeLength; i++) {
                        nativeOnImeKey(KeyEvent.KEYCODE_DEL);
                    }
                    return true;
                }

                @Override
                public boolean deleteSurroundingTextInCodePoints(int beforeLength, int afterLength) {
                    for (int i = 0; i < beforeLength; i++) {
                        nativeOnImeKey(KeyEvent.KEYCODE_DEL);
                    }
                    return true;
                }

                @Override
                public boolean sendKeyEvent(KeyEvent event) {
                    if (event != null && event.getAction() == KeyEvent.ACTION_DOWN) {
                        int code = event.getKeyCode();
                        if (code == KeyEvent.KEYCODE_DEL
                                || code == KeyEvent.KEYCODE_ENTER
                                || code == KeyEvent.KEYCODE_NUMPAD_ENTER) {
                            nativeOnImeKey(code);
                            return true;
                        }
                        int ch = event.getUnicodeChar();
                        if (ch > 0 && !Character.isISOControl(ch)) {
                            nativeOnImeCommit(new String(Character.toChars(ch)));
                            return true;
                        }
                    }
                    return super.sendKeyEvent(event);
                }
            };
        }
    }
}
