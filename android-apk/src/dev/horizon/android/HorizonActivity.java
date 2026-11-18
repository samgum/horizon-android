package dev.horizon.android;

import android.app.NativeActivity;
import android.content.Context;
import android.os.Build;
import android.os.Bundle;
import android.text.InputType;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

public class HorizonActivity extends NativeActivity {
    private InputBridgeView imeView;

    private static native void nativeOnImeCommit(String text);
    private static native void nativeOnImePreedit(String text);
    private static native void nativeOnImeKey(int keyCode);

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        imeView = new InputBridgeView(this);
        imeView.setFocusable(true);
        imeView.setFocusableInTouchMode(true);
        addContentView(imeView, new ViewGroup.LayoutParams(1, 1));
    }

    public void setImeActive(final boolean active) {
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
            outAttrs.imeOptions = EditorInfo.IME_ACTION_DONE | EditorInfo.IME_FLAG_NO_EXTRACT_UI;
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
                public boolean deleteSurroundingText(int beforeLength, int afterLength) {
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
