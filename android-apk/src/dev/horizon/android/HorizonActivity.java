package dev.horizon.android;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.DialogInterface;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Insets;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.SystemClock;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.text.InputType;
import android.util.Base64;
import android.util.DisplayMetrics;
import android.view.DisplayCutout;
import android.view.Gravity;
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
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;
import android.window.SplashScreen;
import android.window.SplashScreenView;

import org.json.JSONArray;
import org.json.JSONObject;

import java.nio.charset.StandardCharsets;
import java.security.KeyStore;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

public class HorizonActivity extends NativeActivity {
    static {
        try {
            System.loadLibrary("horizon_android");
        } catch (Throwable ignored) {
        }
    }

    private static final String KEYSTORE_PROVIDER = "AndroidKeyStore";
    private static final String KEYSTORE_ALIAS = "horizon_msa_secrets_v1";
    private static final String SECRETS_PREFS = "horizon_auth_secrets";

    private InputBridgeView imeView;
    private BootSplashView bootSplashView;
    private android.app.Dialog bootSplashDialog;
    private ClipboardManager clipboardManager;
    private SharedPreferences secretPrefs;
    private AlertDialog accountDialog;
    private LinearLayout accountDialogRoot;
    private TextView signInStatusView;
    private TextView signInCodeView;
    private Button openBrowserBtn;
    private Button cancelSignInBtn;
    private volatile String currentVerificationUri = "https://www.microsoft.com/link";
    private volatile String currentUserCode = "";

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
    private static native String nativeGetAccountStateJson();
    private static native String nativeSetOfflineMode(String offlineName);
    private static native void nativeSetCustomClientId(String clientId);
    private static native String nativeSelectMicrosoftAccount(String uuid);
    private static native void nativeRefreshMicrosoftAccount(String uuid);
    private static native void nativeRemoveMicrosoftAccount(String uuid);
    private static native void nativeStartMsaSignIn(String clientIdOverride);
    private static native void nativeCancelMsaSignIn();
    private static native String nativeImportSessionJson(String jsonText);

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
        secretPrefs = getSharedPreferences(SECRETS_PREFS, Context.MODE_PRIVATE);
        imeView = new InputBridgeView(this);
        imeView.setFocusable(true);
        imeView.setFocusableInTouchMode(true);
        addContentView(imeView, new ViewGroup.LayoutParams(1, 1));

        if (!bootDismissed) {
            bootSplashView = new BootSplashView(this);
            bootSplashDialog = new android.app.Dialog(
                this,
                android.R.style.Theme_Black_NoTitleBar_Fullscreen
            );
            bootSplashDialog.setCancelable(false);
            bootSplashDialog.setCanceledOnTouchOutside(false);
            bootSplashDialog.setContentView(bootSplashView);
            Window dlgWin = bootSplashDialog.getWindow();
            if (dlgWin != null) {
                dlgWin.addFlags(
                    WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                        | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN
                        | WindowManager.LayoutParams.FLAG_FULLSCREEN
                );
                if (Build.VERSION.SDK_INT >= 28) {
                    WindowManager.LayoutParams lp = dlgWin.getAttributes();
                    lp.layoutInDisplayCutoutMode =
                        WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
                    dlgWin.setAttributes(lp);
                }
            }
            try {
                bootSplashDialog.show();
            } catch (Throwable ignored) {
            }
        }

        configureImmersiveWindow();
        installInsetsListener();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus && !imeRequested && (accountDialog == null || !accountDialog.isShowing())) {
            configureImmersiveWindow();
        }
    }

    @SuppressWarnings("deprecation")
    private void configureImmersiveWindow() {
        try {
            final Window window = getWindow();
            if (window == null) return;
            final View decor = window.getDecorView();
            if (decor == null) return;
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
                decor.setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                        | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                        | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_FULLSCREEN
                );
            }
        } catch (Throwable ignored) {
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
                    Insets mandatory = insets.getInsets(WindowInsets.Type.mandatorySystemGestures());
                    gLeft = gesture.left;
                    gRight = gesture.right;
                    cBottom = Math.max(cBottom, Math.max(gesture.bottom, mandatory.bottom));
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
                        cBottom = Math.max(cBottom, gesture.bottom);
                    }
                    Rect visible = new Rect();
                    v.getWindowVisibleDisplayFrame(visible);
                    int rootH = v.getRootView().getHeight();
                    if (rootH > 0 && rootH - visible.bottom > rootH / 6) {
                        imeBottom = rootH - visible.bottom;
                    }
                }
                try {
                    nativeOnWindowInsets(cLeft, cTop, cRight, cBottom, gLeft, gRight, imeBottom);
                } catch (UnsatisfiedLinkError ignored) {
                }
                return insets;
            }
        });
    }

    // ----- Android Hardware Keystore Vault (AES-256-GCM) -----------------------------------------

    private synchronized SecretKey getOrCreateVaultKey() throws Exception {
        KeyStore ks = KeyStore.getInstance(KEYSTORE_PROVIDER);
        ks.load(null);
        KeyStore.Entry existing = ks.getEntry(KEYSTORE_ALIAS, null);
        if (existing instanceof KeyStore.SecretKeyEntry) {
            return ((KeyStore.SecretKeyEntry) existing).getSecretKey();
        }
        KeyGenerator kg = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            KEYSTORE_PROVIDER
        );
        KeyGenParameterSpec spec = new KeyGenParameterSpec.Builder(
            KEYSTORE_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .build();
        kg.init(spec);
        return kg.generateKey();
    }

    public synchronized void sealSecret(String key, String plaintext) {
        if (key == null || plaintext == null || secretPrefs == null) return;
        try {
            SecretKey secretKey = getOrCreateVaultKey();
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.ENCRYPT_MODE, secretKey);
            byte[] iv = cipher.getIV();
            byte[] encrypted = cipher.doFinal(plaintext.getBytes(StandardCharsets.UTF_8));
            String blob = Base64.encodeToString(iv, Base64.NO_WRAP)
                + ":"
                + Base64.encodeToString(encrypted, Base64.NO_WRAP);
            secretPrefs.edit().putString(key, blob).apply();
        } catch (Throwable ignored) {
        }
    }

    public synchronized String openSecret(String key) {
        if (key == null || secretPrefs == null) return "";
        try {
            String blob = secretPrefs.getString(key, null);
            if (blob == null || blob.isEmpty()) return "";
            int sep = blob.indexOf(':');
            if (sep <= 0) return "";
            byte[] iv = Base64.decode(blob.substring(0, sep), Base64.NO_WRAP);
            byte[] encrypted = Base64.decode(blob.substring(sep + 1), Base64.NO_WRAP);
            SecretKey secretKey = getOrCreateVaultKey();
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.DECRYPT_MODE, secretKey, new GCMParameterSpec(128, iv));
            byte[] plain = cipher.doFinal(encrypted);
            return new String(plain, StandardCharsets.UTF_8);
        } catch (Throwable ignored) {
            return "";
        }
    }

    public synchronized void deleteSecret(String key) {
        if (key == null || secretPrefs == null) return;
        try {
            secretPrefs.edit().remove(key).apply();
        } catch (Throwable ignored) {
        }
    }

    // ----- Account Center Dialog (Offline Testing + Microsoft Genuine Sign-In) -------------------

    private int dpPx(float dpVal) {
        return Math.round(dpVal * getResources().getDisplayMetrics().density);
    }

    private GradientDrawable roundedBox(int fillArgb, int strokeArgb, float radiusDp) {
        GradientDrawable gd = new GradientDrawable();
        gd.setColor(fillArgb);
        gd.setCornerRadius(dpPx(radiusDp));
        if (strokeArgb != 0) {
            gd.setStroke(Math.max(1, dpPx(1f)), strokeArgb);
        }
        return gd;
    }

    private Button styledButton(String text, int bgArgb, int strokeArgb, int textArgb) {
        Button b = new Button(this);
        b.setAllCaps(false);
        b.setText(text);
        b.setTextColor(textArgb);
        b.setTextSize(13f);
        b.setPadding(dpPx(12), dpPx(6), dpPx(12), dpPx(6));
        b.setMinimumHeight(dpPx(36));
        b.setMinHeight(dpPx(36));
        b.setBackground(roundedBox(bgArgb, strokeArgb, 6f));
        return b;
    }

    public void openAccountCenterDialog() {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                showOrRefreshAccountDialog();
            }
        });
    }

    public void onMsaSignInUpdate(
        final String status,
        final String userCode,
        final String verificationUri,
        final boolean inProgress,
        final boolean completed
    ) {
        if (verificationUri != null && !verificationUri.isEmpty()) {
            currentVerificationUri = verificationUri;
        }
        if (userCode != null) {
            currentUserCode = userCode;
        }
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                if (completed) {
                    if (accountDialog != null && accountDialog.isShowing()) {
                        rebuildAccountDialogContents();
                    }
                }
                if (signInStatusView != null && status != null && !status.isEmpty()) {
                    signInStatusView.setVisibility(View.VISIBLE);
                    signInStatusView.setText(status);
                }
                if (signInCodeView != null) {
                    if (userCode != null && !userCode.isEmpty()) {
                        signInCodeView.setVisibility(View.VISIBLE);
                        signInCodeView.setText("设备验证码:  " + userCode);
                    } else if (!inProgress) {
                        signInCodeView.setVisibility(View.GONE);
                    }
                }
                if (openBrowserBtn != null) {
                    openBrowserBtn.setVisibility(
                        (inProgress && userCode != null && !userCode.isEmpty())
                            ? View.VISIBLE
                            : View.GONE
                    );
                }
                if (cancelSignInBtn != null) {
                    cancelSignInBtn.setVisibility(inProgress ? View.VISIBLE : View.GONE);
                }
            }
        });
    }

    private void showOrRefreshAccountDialog() {
        if (isFinishing()) return;
        if (accountDialog != null && accountDialog.isShowing()) {
            rebuildAccountDialogContents();
            return;
        }
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackground(roundedBox(0xFF11151E, 0xFF2B3548, 10f));

        accountDialogRoot = new LinearLayout(this);
        accountDialogRoot.setOrientation(LinearLayout.VERTICAL);
        int pad = dpPx(16);
        accountDialogRoot.setPadding(pad, dpPx(14), pad, dpPx(14));
        scroll.addView(accountDialogRoot);

        rebuildAccountDialogContents();

        AlertDialog.Builder builder = new AlertDialog.Builder(this);
        builder.setView(scroll);
        accountDialog = builder.create();
        accountDialog.setOnDismissListener(new DialogInterface.OnDismissListener() {
            @Override
            public void onDismiss(DialogInterface dialog) {
                configureImmersiveWindow();
            }
        });
        if (accountDialog.getWindow() != null) {
            accountDialog.getWindow().setBackgroundDrawable(roundedBox(0x00000000, 0, 10f));
        }
        accountDialog.show();
        if (accountDialog.getWindow() != null) {
            int maxW = Math.min(getResources().getDisplayMetrics().widthPixels - dpPx(32), dpPx(540));
            accountDialog.getWindow().setLayout(maxW, ViewGroup.LayoutParams.WRAP_CONTENT);
        }
    }

    private void rebuildAccountDialogContents() {
        if (accountDialogRoot == null) return;
        accountDialogRoot.removeAllViews();

        String mode = "offline";
        String offlineName = "Player";
        String customClientId = "";
        boolean hasBuiltinCid = false;
        JSONArray accounts = new JSONArray();
        try {
            JSONObject st = new JSONObject(nativeGetAccountStateJson());
            mode = st.optString("mode", "offline");
            offlineName = st.optString("offline_name", "Player");
            customClientId = st.optString("custom_client_id", "");
            hasBuiltinCid = st.optBoolean("has_builtin_client_id", false);
            JSONArray arr = st.optJSONArray("accounts");
            if (arr != null) accounts = arr;
        } catch (Throwable ignored) {
        }

        // Header row
        LinearLayout header = new LinearLayout(this);
        header.setOrientation(LinearLayout.HORIZONTAL);
        header.setGravity(Gravity.CENTER_VERTICAL);

        TextView title = new TextView(this);
        title.setText("账号模式管理（离线直连 / 微软正版）");
        title.setTextColor(0xFFF8FAFC);
        title.setTextSize(16f);
        title.setTypeface(Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD));
        LinearLayout.LayoutParams titleLp = new LinearLayout.LayoutParams(
            0,
            ViewGroup.LayoutParams.WRAP_CONTENT,
            1f
        );
        header.addView(title, titleLp);

        Button closeBtn = styledButton("完成", 0xFF1E293B, 0xFF475569, 0xFFE2E8F0);
        closeBtn.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                if (accountDialog != null) accountDialog.dismiss();
            }
        });
        header.addView(closeBtn);
        accountDialogRoot.addView(header);

        // Card 1: Offline Mode (preserved & instant for local testing!)
        LinearLayout offlineCard = new LinearLayout(this);
        offlineCard.setOrientation(LinearLayout.VERTICAL);
        int isOff = "offline".equals(mode) ? 1 : 0;
        offlineCard.setBackground(
            roundedBox(0xFF18202F, isOff == 1 ? 0xFFFACC15 : 0xFF2B3548, 8f)
        );
        offlineCard.setPadding(dpPx(12), dpPx(10), dpPx(12), dpPx(10));
        LinearLayout.LayoutParams cardLp = new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
        cardLp.topMargin = dpPx(10);
        accountDialogRoot.addView(offlineCard, cardLp);

        TextView offTitle = new TextView(this);
        offTitle.setText(
            isOff == 1
                ? "● 离线测试模式（当前使用中 · 免登录直接进游戏）"
                : "○ 离线测试模式（免登录直接测试单机与离线服）"
        );
        offTitle.setTextColor(isOff == 1 ? 0xFFFDE047 : 0xFFCBD5E1);
        offTitle.setTextSize(13.5f);
        offTitle.setTypeface(Typeface.DEFAULT_BOLD);
        offlineCard.addView(offTitle);

        LinearLayout offRow = new LinearLayout(this);
        offRow.setOrientation(LinearLayout.HORIZONTAL);
        offRow.setGravity(Gravity.CENTER_VERTICAL);
        LinearLayout.LayoutParams offRowLp = new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
        offRowLp.topMargin = dpPx(8);

        final EditText offInput = new EditText(this);
        offInput.setSingleLine(true);
        offInput.setText(offlineName);
        offInput.setHint("离线玩家名 (1~16位字母/数字/_)");
        offInput.setHintTextColor(0xFF64748B);
        offInput.setTextColor(0xFFF8FAFC);
        offInput.setTextSize(13.5f);
        offInput.setPadding(dpPx(10), dpPx(6), dpPx(10), dpPx(6));
        offInput.setBackground(roundedBox(0xFF0D121B, 0xFF334155, 6f));
        offInput.setImeOptions(EditorInfo.IME_FLAG_NO_EXTRACT_UI | EditorInfo.IME_ACTION_DONE);
        LinearLayout.LayoutParams offInLp = new LinearLayout.LayoutParams(
            0,
            ViewGroup.LayoutParams.WRAP_CONTENT,
            1f
        );
        offInLp.rightMargin = dpPx(8);
        offRow.addView(offInput, offInLp);

        Button applyOffBtn = styledButton(
            isOff == 1 ? "保存离线名" : "切换为离线模式",
            0xFF334155,
            0xFFFACC15,
            0xFFFEF08A
        );
        applyOffBtn.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                String err = nativeSetOfflineMode(offInput.getText().toString());
                if (err != null && !err.isEmpty()) {
                    Toast.makeText(HorizonActivity.this, err, Toast.LENGTH_SHORT).show();
                } else {
                    Toast.makeText(
                        HorizonActivity.this,
                        "已启用离线模式: " + offInput.getText().toString().trim(),
                        Toast.LENGTH_SHORT
                    ).show();
                    rebuildAccountDialogContents();
                }
            }
        });
        offRow.addView(applyOffBtn);
        offlineCard.addView(offRow, offRowLp);

        // Card 2: Microsoft Genuine Accounts & Device-Code Sign-In
        LinearLayout msaCard = new LinearLayout(this);
        msaCard.setOrientation(LinearLayout.VERTICAL);
        msaCard.setBackground(
            roundedBox(0xFF18202F, isOff == 0 ? 0xFF4ADE80 : 0xFF2B3548, 8f)
        );
        msaCard.setPadding(dpPx(12), dpPx(10), dpPx(12), dpPx(10));
        accountDialogRoot.addView(msaCard, cardLp);

        TextView msaTitle = new TextView(this);
        msaTitle.setText("微软正版账号（支持正版验证服务器、正版皮肤与自动续期）");
        msaTitle.setTextColor(0xFF86EFAC);
        msaTitle.setTextSize(13.5f);
        msaTitle.setTypeface(Typeface.DEFAULT_BOLD);
        msaCard.addView(msaTitle);

        for (int i = 0; i < accounts.length(); i++) {
            JSONObject acc = accounts.optJSONObject(i);
            if (acc == null) continue;
            final String uuid = acc.optString("uuid", "");
            final String name = acc.optString("name", "Player");
            final boolean active = acc.optBoolean("active", false);
            final boolean onlineReady = acc.optBoolean("online_ready", false);

            LinearLayout row = new LinearLayout(this);
            row.setOrientation(LinearLayout.HORIZONTAL);
            row.setGravity(Gravity.CENTER_VERTICAL);
            row.setPadding(dpPx(10), dpPx(6), dpPx(10), dpPx(6));
            row.setBackground(roundedBox(0xFF0F1623, active ? 0xFF4ADE80 : 0xFF263145, 6f));
            LinearLayout.LayoutParams rlp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            );
            rlp.topMargin = dpPx(6);

            TextView label = new TextView(this);
            String badge = active
                ? (onlineReady ? "✓ 当前正版在线: " : "● 当前账号 (待刷新): ")
                : "正版账号: ";
            label.setText(badge + name);
            label.setTextColor(active ? 0xFF4ADE80 : 0xFFE2E8F0);
            label.setTextSize(13f);
            row.addView(
                label,
                new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
            );

            if (!active) {
                Button useBtn = styledButton("切换", 0xFF14532D, 0xFF4ADE80, 0xFFDCFCE7);
                useBtn.setOnClickListener(new View.OnClickListener() {
                    @Override
                    public void onClick(View v) {
                        String note = nativeSelectMicrosoftAccount(uuid);
                        if (note != null && !note.isEmpty()) {
                            Toast.makeText(HorizonActivity.this, note, Toast.LENGTH_LONG).show();
                        } else {
                            Toast.makeText(
                                HorizonActivity.this,
                                "已切换为正版账号: " + name,
                                Toast.LENGTH_SHORT
                            ).show();
                        }
                        rebuildAccountDialogContents();
                    }
                });
                row.addView(useBtn);
            } else {
                Button refreshBtn = styledButton("刷新令牌", 0xFF1E3A8A, 0xFF60A5FA, 0xFFDBEAFE);
                refreshBtn.setOnClickListener(new View.OnClickListener() {
                    @Override
                    public void onClick(View v) {
                        Toast.makeText(HorizonActivity.this, "正在后台刷新令牌…", Toast.LENGTH_SHORT).show();
                        nativeRefreshMicrosoftAccount(uuid);
                    }
                });
                row.addView(refreshBtn);
            }

            Button removeBtn = styledButton("退出", 0xFF3F1D24, 0xFFF87171, 0xFFFECACA);
            LinearLayout.LayoutParams rmLp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            );
            rmLp.leftMargin = dpPx(6);
            removeBtn.setOnClickListener(new View.OnClickListener() {
                @Override
                public void onClick(View v) {
                    nativeRemoveMicrosoftAccount(uuid);
                    rebuildAccountDialogContents();
                }
            });
            row.addView(removeBtn, rmLp);
            msaCard.addView(row, rlp);
        }

        // Device-code sign-in controls & live status
        final EditText cidInput = new EditText(this);
        cidInput.setSingleLine(true);
        cidInput.setText(customClientId);
        cidInput.setHint(
            hasBuiltinCid
                ? "可选：自定义 Azure Client ID（留空使用内置配置）"
                : "填入 Azure 应用 Client ID (开启 Allow public client flows)"
        );
        cidInput.setHintTextColor(0xFF64748B);
        cidInput.setTextColor(0xFFE2E8F0);
        cidInput.setTextSize(12f);
        cidInput.setPadding(dpPx(10), dpPx(6), dpPx(10), dpPx(6));
        cidInput.setBackground(roundedBox(0xFF0D121B, 0xFF334155, 6f));
        cidInput.setImeOptions(EditorInfo.IME_FLAG_NO_EXTRACT_UI | EditorInfo.IME_ACTION_DONE);

        LinearLayout signInBtnRow = new LinearLayout(this);
        signInBtnRow.setOrientation(LinearLayout.HORIZONTAL);
        signInBtnRow.setGravity(Gravity.CENTER_VERTICAL);
        LinearLayout.LayoutParams sbrLp = new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
        sbrLp.topMargin = dpPx(8);

        Button addMsaBtn = styledButton(
            "+ 微软设备码正版登录",
            0xFF166534,
            0xFF4ADE80,
            0xFFF0FDF4
        );
        addMsaBtn.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                nativeStartMsaSignIn(cidInput.getText().toString());
            }
        });
        signInBtnRow.addView(addMsaBtn);

        openBrowserBtn = styledButton(
            "复制验证码并打开浏览器",
            0xFF1D4ED8,
            0xFF60A5FA,
            0xFFEFF6FF
        );
        openBrowserBtn.setVisibility(View.GONE);
        LinearLayout.LayoutParams obLp = new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.WRAP_CONTENT,
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
        obLp.leftMargin = dpPx(8);
        openBrowserBtn.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                if (currentUserCode != null && !currentUserCode.isEmpty()) {
                    setClipboardText(currentUserCode);
                }
                openBrowserUrl(currentVerificationUri);
            }
        });
        signInBtnRow.addView(openBrowserBtn, obLp);

        cancelSignInBtn = styledButton("取消", 0xFF334155, 0xFF94A3B8, 0xFFE2E8F0);
        cancelSignInBtn.setVisibility(View.GONE);
        cancelSignInBtn.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                nativeCancelMsaSignIn();
                if (signInStatusView != null) signInStatusView.setText("已取消登录。");
                if (signInCodeView != null) signInCodeView.setVisibility(View.GONE);
                openBrowserBtn.setVisibility(View.GONE);
                cancelSignInBtn.setVisibility(View.GONE);
            }
        });
        signInBtnRow.addView(cancelSignInBtn, obLp);
        msaCard.addView(signInBtnRow, sbrLp);

        signInCodeView = new TextView(this);
        signInCodeView.setVisibility(View.GONE);
        signInCodeView.setTextColor(0xFF4ADE80);
        signInCodeView.setTextSize(17f);
        signInCodeView.setTypeface(Typeface.MONOSPACE, Typeface.BOLD);
        signInCodeView.setPadding(0, dpPx(8), 0, dpPx(2));
        msaCard.addView(signInCodeView);

        signInStatusView = new TextView(this);
        signInStatusView.setVisibility(View.GONE);
        signInStatusView.setTextColor(0xFFCBD5E1);
        signInStatusView.setTextSize(12.5f);
        signInStatusView.setPadding(0, dpPx(4), 0, dpPx(2));
        msaCard.addView(signInStatusView);

        // Card 3: Advanced Client ID & Debug Session JSON Import
        LinearLayout advCard = new LinearLayout(this);
        advCard.setOrientation(LinearLayout.VERTICAL);
        advCard.setBackground(roundedBox(0xFF141B27, 0xFF263145, 8f));
        advCard.setPadding(dpPx(12), dpPx(8), dpPx(12), dpPx(10));
        accountDialogRoot.addView(advCard, cardLp);

        TextView advTitle = new TextView(this);
        advTitle.setText("高级 / 开发者配置（Azure Client ID & 调试会话导入）");
        advTitle.setTextColor(0xFF94A3B8);
        advTitle.setTextSize(12f);
        advCard.addView(advTitle);

        LinearLayout cidRow = new LinearLayout(this);
        cidRow.setOrientation(LinearLayout.HORIZONTAL);
        cidRow.setGravity(Gravity.CENTER_VERTICAL);
        LinearLayout.LayoutParams cidRowLp = new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
        cidRowLp.topMargin = dpPx(6);
        cidRow.addView(cidInput, offInLp);

        Button saveCidBtn = styledButton("保存 ID", 0xFF1E293B, 0xFF475569, 0xFFCBD5E1);
        saveCidBtn.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                nativeSetCustomClientId(cidInput.getText().toString());
                Toast.makeText(HorizonActivity.this, "已保存 Client ID 配置", Toast.LENGTH_SHORT).show();
            }
        });
        cidRow.addView(saveCidBtn);

        Button pasteSessionBtn = styledButton("从剪贴板导入 Session", 0xFF1E293B, 0xFF475569, 0xFFCBD5E1);
        pasteSessionBtn.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                String clip = getClipboardText();
                if (clip == null || clip.trim().isEmpty()) {
                    Toast.makeText(HorizonActivity.this, "剪贴板为空", Toast.LENGTH_SHORT).show();
                    return;
                }
                String err = nativeImportSessionJson(clip);
                if (err != null && !err.isEmpty()) {
                    Toast.makeText(HorizonActivity.this, err, Toast.LENGTH_LONG).show();
                } else {
                    rebuildAccountDialogContents();
                }
            }
        });
        cidRow.addView(pasteSessionBtn, obLp);

        Button reimportResBtn = styledButton("重新导入资源包", 0xFF1E293B, 0xFF475569, 0xFFCBD5E1);
        reimportResBtn.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                try {
                    java.io.File marker = new java.io.File(
                        new java.io.File(new java.io.File(getFilesDir(), "resources"), "26.3"),
                        ".extracted_26_3"
                    );
                    if (marker.exists()) {
                        marker.delete();
                    }
                    Intent intent = new Intent(HorizonActivity.this, HorizonLaunchActivity.class);
                    intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK);
                    startActivity(intent);
                    Runtime.getRuntime().exit(0);
                } catch (Throwable ignored) {
                }
            }
        });
        cidRow.addView(reimportResBtn, obLp);
        advCard.addView(cidRow, cidRowLp);
    }

    public void openBrowserUrl(final String url) {
        if (url == null || url.isEmpty()) return;
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    Intent intent = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
                    intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                    startActivity(intent);
                } catch (Throwable e) {
                    Toast.makeText(
                        HorizonActivity.this,
                        "无法拉起浏览器，请手动访问: " + url,
                        Toast.LENGTH_LONG
                    ).show();
                }
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
                final android.app.Dialog dlg = bootSplashDialog;
                bootSplashDialog = null;
                final BootSplashView view = bootSplashView;
                bootSplashView = null;
                if (view != null) {
                    view.setClickable(false);
                    view.setFocusable(false);
                }
                if (dlg != null) {
                    try {
                        if (dlg.isShowing()) {
                            dlg.dismiss();
                        }
                    } catch (Throwable ignored) {
                    }
                }
            }
        });
    }

    @Override
    protected void onDestroy() {
        if (bootSplashDialog != null) {
            try {
                if (bootSplashDialog.isShowing()) {
                    bootSplashDialog.dismiss();
                }
            } catch (Throwable ignored) {
            }
            bootSplashDialog = null;
        }
        super.onDestroy();
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
                displayedPercent = Math.min(
                    target,
                    displayedPercent + Math.max(0.6f, (target - displayedPercent) * 0.22f)
                );
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
