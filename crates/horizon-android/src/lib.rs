use android_activity::AndroidApp;
use horizon_client::auth::msa;
use horizon_client::entry::{
    AndroidImeEvent, AndroidWindowInsets, push_android_ime, push_android_player_name,
    set_android_account_dialog_callback, set_android_boot_callbacks,
    set_android_clipboard_callbacks, set_android_ime_callback, set_android_keep_screen_on_callback,
    set_android_toast_callback, set_android_window_insets,
};
use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString, JValue};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

static ANDROID_APP_FOR_IME: OnceLock<AndroidApp> = OnceLock::new();
static ANDROID_GAME_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Incremented on every sign-in start or cancel so superseded pollers stop immediately.
static MSA_SIGNIN_GENERATION: AtomicU64 = AtomicU64::new(0);

fn game_dir_path() -> PathBuf {
    ANDROID_GAME_DIR
        .get()
        .cloned()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/horizon/game"))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeOnImeCommit(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
) {
    if let Ok(s) = env.get_string(&text) {
        let s: String = s.into();
        if !s.is_empty() {
            push_android_ime(AndroidImeEvent::Commit(s));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeOnImePreedit(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
) {
    if let Ok(s) = env.get_string(&text) {
        push_android_ime(AndroidImeEvent::Preedit(s.into()));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeOnImeKey(
    _env: JNIEnv,
    _class: JClass,
    key_code: jni::sys::jint,
) {
    match key_code {
        67 => push_android_ime(AndroidImeEvent::Backspace),
        66 | 160 => push_android_ime(AndroidImeEvent::Enter),
        _ => {}
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeOnWindowInsets(
    _env: JNIEnv,
    _class: JClass,
    cutout_left: jni::sys::jint,
    cutout_top: jni::sys::jint,
    cutout_right: jni::sys::jint,
    cutout_bottom: jni::sys::jint,
    gesture_left: jni::sys::jint,
    gesture_right: jni::sys::jint,
    ime_bottom: jni::sys::jint,
) {
    set_android_window_insets(AndroidWindowInsets {
        cutout: [
            cutout_left.max(0) as u32,
            cutout_top.max(0) as u32,
            cutout_right.max(0) as u32,
            cutout_bottom.max(0) as u32,
        ],
        gesture_lr: [gesture_left.max(0) as u32, gesture_right.max(0) as u32],
        ime_bottom: ime_bottom.max(0) as u32,
    });
}

fn new_jstring_or_null<'a>(env: &mut JNIEnv<'a>, text: &str) -> jni::sys::jstring {
    env.new_string(text)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeGetAccountStateJson(
    mut env: JNIEnv,
    _class: JClass,
) -> jni::sys::jstring {
    let json = msa::ui_state_json(&game_dir_path());
    new_jstring_or_null(&mut env, &json)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeSetOfflineMode(
    mut env: JNIEnv,
    _class: JClass,
    offline_name: JString,
) -> jni::sys::jstring {
    let raw: String = env
        .get_string(&offline_name)
        .map(Into::into)
        .unwrap_or_default();
    let Some(clean) = msa::sanitize_offline_name(&raw) else {
        return new_jstring_or_null(
            &mut env,
            "离线玩家名需为 1~16 位英文字母、数字或下划线 (_)。",
        );
    };
    let dir = game_dir_path();
    let mut store = msa::AccountStore::load(&dir);
    store.offline_name = clean.clone();
    store.mode = msa::AccountMode::Offline;
    if let Err(e) = store.save(&dir) {
        return new_jstring_or_null(&mut env, &format!("保存账号配置失败: {e}"));
    }
    let resolved = msa::resolve_and_activate(&dir, &msa::Endpoints::live(), false);
    push_android_player_name(resolved.player_name);
    new_jstring_or_null(&mut env, "")
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeSetCustomClientId(
    mut env: JNIEnv,
    _class: JClass,
    client_id: JString,
) {
    let cid: String = env
        .get_string(&client_id)
        .map(Into::into)
        .unwrap_or_default();
    let dir = game_dir_path();
    let mut store = msa::AccountStore::load(&dir);
    store.custom_client_id = cid.trim().to_owned();
    let _ = store.save(&dir);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeSelectMicrosoftAccount(
    mut env: JNIEnv,
    _class: JClass,
    uuid: JString,
) -> jni::sys::jstring {
    let uid: String = env.get_string(&uuid).map(Into::into).unwrap_or_default();
    let norm = uid.replace('-', "").to_lowercase();
    let dir = game_dir_path();
    let mut store = msa::AccountStore::load(&dir);
    if !store.accounts.iter().any(|a| a.uuid == norm) {
        return new_jstring_or_null(&mut env, "未找到该微软账号记录。");
    }
    store.mode = msa::AccountMode::Microsoft { uuid: norm };
    if let Err(e) = store.save(&dir) {
        return new_jstring_or_null(&mut env, &format!("保存账号状态失败: {e}"));
    }
    let resolved = msa::resolve_and_activate(&dir, &msa::Endpoints::live(), false);
    push_android_player_name(resolved.player_name);
    new_jstring_or_null(&mut env, resolved.notice.as_deref().unwrap_or(""))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeRefreshMicrosoftAccount(
    mut env: JNIEnv,
    _class: JClass,
    uuid: JString,
) {
    let uid: String = env.get_string(&uuid).map(Into::into).unwrap_or_default();
    let norm = uid.replace('-', "").to_lowercase();
    let dir = game_dir_path();
    let _ = std::thread::Builder::new()
        .name("horizon-msa-refresh".into())
        .spawn(move || {
            let mut store = msa::AccountStore::load(&dir);
            if store.accounts.iter().any(|a| a.uuid == norm) {
                store.mode = msa::AccountMode::Microsoft { uuid: norm };
                let _ = store.save(&dir);
            }
            let resolved = msa::resolve_and_activate(&dir, &msa::Endpoints::live(), true);
            push_android_player_name(resolved.player_name.clone());
            let msg = match resolved.notice {
                Some(note) => note,
                None if resolved.session.is_some() => {
                    format!("已刷新正版会话: {}", resolved.player_name)
                }
                None => format!("当前账号: {}", resolved.player_name),
            };
            notify_java_msa_update(&msg, "", "", false, true);
            notify_java_toast(&msg);
        });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeRemoveMicrosoftAccount(
    mut env: JNIEnv,
    _class: JClass,
    uuid: JString,
) {
    let uid: String = env.get_string(&uuid).map(Into::into).unwrap_or_default();
    let norm = uid.replace('-', "").to_lowercase();
    let dir = game_dir_path();
    msa::delete_secrets(&norm);
    let mut store = msa::AccountStore::load(&dir);
    store.remove_account(&norm);
    let _ = store.save(&dir);
    let resolved = msa::resolve_and_activate(&dir, &msa::Endpoints::live(), false);
    push_android_player_name(resolved.player_name);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeStartMsaSignIn(
    mut env: JNIEnv,
    _class: JClass,
    client_id_override: JString,
) {
    let cid_override: String = env
        .get_string(&client_id_override)
        .map(Into::into)
        .unwrap_or_default();
    let dir = game_dir_path();
    let mut store = msa::AccountStore::load(&dir);
    if !cid_override.trim().is_empty() {
        store.custom_client_id = cid_override.trim().to_owned();
        let _ = store.save(&dir);
    }
    let Some(cid) = store.effective_client_id() else {
        notify_java_msa_update(
            "请先在下方「高级 / Client ID 配置」填入 Azure 应用 Client ID（开启 Allow public client flows）。",
            "",
            "",
            false,
            false,
        );
        return;
    };

    let generation = MSA_SIGNIN_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    notify_java_msa_update("正在向 Microsoft 请求设备登录码…", "", "", true, false);

    let _ = std::thread::Builder::new()
        .name("horizon-msa-signin".into())
        .spawn(move || {
            let ep = msa::Endpoints::live();
            let dc = match msa::request_device_code(&ep, &cid) {
                Ok(code) => code,
                Err(e) => {
                    if MSA_SIGNIN_GENERATION.load(Ordering::Acquire) == generation {
                        notify_java_msa_update(&e, "", "", false, false);
                    }
                    return;
                }
            };
            if MSA_SIGNIN_GENERATION.load(Ordering::Acquire) != generation {
                return;
            }
            jni_set_clipboard(&dc.user_code);
            notify_java_msa_update(
                "验证码已自动复制到剪贴板！请点击下方按钮打开浏览器粘贴授权：",
                &dc.user_code,
                &dc.verification_uri,
                true,
                false,
            );

            let ms_tokens = match msa::wait_for_device_tokens(&ep, &cid, &dc, &|| {
                MSA_SIGNIN_GENERATION.load(Ordering::Acquire) != generation
            }) {
                Ok(tokens) => tokens,
                Err(e) => {
                    if MSA_SIGNIN_GENERATION.load(Ordering::Acquire) == generation {
                        notify_java_msa_update(&e, "", "", false, false);
                    }
                    return;
                }
            };
            if MSA_SIGNIN_GENERATION.load(Ordering::Acquire) != generation {
                return;
            }

            let finished = msa::finish_sign_in(&ep, ms_tokens, &|phase| {
                if MSA_SIGNIN_GENERATION.load(Ordering::Acquire) == generation {
                    notify_java_msa_update(phase, &dc.user_code, &dc.verification_uri, true, false);
                }
            });

            if MSA_SIGNIN_GENERATION.load(Ordering::Acquire) != generation {
                return;
            }

            match finished {
                Ok((account, secrets)) => {
                    msa::save_secrets(&account.uuid, &secrets);
                    let mut current_store = msa::AccountStore::load(&dir);
                    current_store.upsert_account(account.clone());
                    current_store.mode = msa::AccountMode::Microsoft {
                        uuid: account.uuid.clone(),
                    };
                    let _ = current_store.save(&dir);
                    let resolved = msa::resolve_and_activate(&dir, &ep, false);
                    push_android_player_name(resolved.player_name.clone());
                    let ok_msg = format!("正版登录成功！当前玩家: {}", resolved.player_name);
                    notify_java_msa_update(&ok_msg, "", "", false, true);
                    notify_java_toast(&ok_msg);
                }
                Err(e) => {
                    notify_java_msa_update(&e, "", "", false, false);
                }
            }
        });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeCancelMsaSignIn(
    _env: JNIEnv,
    _class: JClass,
) {
    MSA_SIGNIN_GENERATION.fetch_add(1, Ordering::AcqRel);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_horizon_android_HorizonActivity_nativeImportSessionJson(
    mut env: JNIEnv,
    _class: JClass,
    json_text: JString,
) -> jni::sys::jstring {
    let raw: String = env
        .get_string(&json_text)
        .map(Into::into)
        .unwrap_or_default();
    let dir = game_dir_path();
    match msa::import_session_json(&dir, &raw) {
        Ok(resolved) => {
            push_android_player_name(resolved.player_name.clone());
            notify_java_toast(&format!("已导入正版会话: {}", resolved.player_name));
            new_jstring_or_null(&mut env, "")
        }
        Err(e) => new_jstring_or_null(&mut env, &format!("导入失败: {e}")),
    }
}

fn register_ime_natives(app: &AndroidApp) {
    unsafe {
        let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
        let act_ptr = app.activity_as_ptr() as jni::sys::jobject;
        if vm_ptr.is_null() || act_ptr.is_null() {
            return;
        }
        let Ok(vm) = jni::JavaVM::from_raw(vm_ptr) else {
            return;
        };
        let Ok(mut env) = vm.get_env().or_else(|_| vm.attach_current_thread_permanently()) else {
            return;
        };
        let activity = JObject::from_raw(act_ptr);
        if let Ok(cls) = env.get_object_class(&activity) {
            let methods = [
                jni::NativeMethod {
                    name: "nativeOnImeCommit".into(),
                    sig: "(Ljava/lang/String;)V".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeOnImeCommit
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeOnImePreedit".into(),
                    sig: "(Ljava/lang/String;)V".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeOnImePreedit
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeOnImeKey".into(),
                    sig: "(I)V".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeOnImeKey
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeOnWindowInsets".into(),
                    sig: "(IIIIIII)V".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeOnWindowInsets
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeGetAccountStateJson".into(),
                    sig: "()Ljava/lang/String;".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeGetAccountStateJson
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeSetOfflineMode".into(),
                    sig: "(Ljava/lang/String;)Ljava/lang/String;".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeSetOfflineMode
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeSetCustomClientId".into(),
                    sig: "(Ljava/lang/String;)V".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeSetCustomClientId
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeSelectMicrosoftAccount".into(),
                    sig: "(Ljava/lang/String;)Ljava/lang/String;".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeSelectMicrosoftAccount
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeRefreshMicrosoftAccount".into(),
                    sig: "(Ljava/lang/String;)V".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeRefreshMicrosoftAccount
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeRemoveMicrosoftAccount".into(),
                    sig: "(Ljava/lang/String;)V".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeRemoveMicrosoftAccount
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeStartMsaSignIn".into(),
                    sig: "(Ljava/lang/String;)V".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeStartMsaSignIn
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeCancelMsaSignIn".into(),
                    sig: "()V".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeCancelMsaSignIn
                        as *mut std::ffi::c_void,
                },
                jni::NativeMethod {
                    name: "nativeImportSessionJson".into(),
                    sig: "(Ljava/lang/String;)Ljava/lang/String;".into(),
                    fn_ptr: Java_dev_horizon_android_HorizonActivity_nativeImportSessionJson
                        as *mut std::ffi::c_void,
                },
            ];
            if let Err(e) = env.register_native_methods(cls, &methods) {
                eprintln!("Horizon Android register_native_methods failed: {e}");
            }
        }
        std::mem::forget(activity);
    }
}

fn call_java_void_method(method: &str) {
    let Some(app) = ANDROID_APP_FOR_IME.get() else {
        return;
    };
    unsafe {
        let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
        let act_ptr = app.activity_as_ptr() as jni::sys::jobject;
        if vm_ptr.is_null() || act_ptr.is_null() {
            return;
        }
        let Ok(vm) = jni::JavaVM::from_raw(vm_ptr) else {
            return;
        };
        let Ok(mut env) = vm.get_env().or_else(|_| vm.attach_current_thread_permanently()) else {
            return;
        };
        let activity = JObject::from_raw(act_ptr);
        let _ = env.call_method(&activity, method, "()V", &[]);
        std::mem::forget(activity);
    }
}

fn call_java_bool_method(method: &str, value: bool) {
    let Some(app) = ANDROID_APP_FOR_IME.get() else {
        return;
    };
    unsafe {
        let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
        let act_ptr = app.activity_as_ptr() as jni::sys::jobject;
        if vm_ptr.is_null() || act_ptr.is_null() {
            return;
        }
        let Ok(vm) = jni::JavaVM::from_raw(vm_ptr) else {
            return;
        };
        let Ok(mut env) = vm.get_env().or_else(|_| vm.attach_current_thread_permanently()) else {
            return;
        };
        let activity = JObject::from_raw(act_ptr);
        let _ = env.call_method(
            &activity,
            method,
            "(Z)V",
            &[JValue::Bool(u8::from(value))],
        );
        std::mem::forget(activity);
    }
}

fn call_java_string_method(method: &str, text: &str) {
    let Some(app) = ANDROID_APP_FOR_IME.get() else {
        return;
    };
    unsafe {
        let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
        let act_ptr = app.activity_as_ptr() as jni::sys::jobject;
        if vm_ptr.is_null() || act_ptr.is_null() {
            return;
        }
        let Ok(vm) = jni::JavaVM::from_raw(vm_ptr) else {
            return;
        };
        let Ok(mut env) = vm.get_env().or_else(|_| vm.attach_current_thread_permanently()) else {
            return;
        };
        let activity = JObject::from_raw(act_ptr);
        if let Ok(jstr) = env.new_string(text) {
            let _ = env.call_method(
                &activity,
                method,
                "(Ljava/lang/String;)V",
                &[JValue::Object(&jstr)],
            );
        }
        std::mem::forget(activity);
    }
}

fn jni_open_secret(key: &str) -> Option<String> {
    let app = ANDROID_APP_FOR_IME.get()?;
    unsafe {
        let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
        let act_ptr = app.activity_as_ptr() as jni::sys::jobject;
        if vm_ptr.is_null() || act_ptr.is_null() {
            return None;
        }
        let vm = jni::JavaVM::from_raw(vm_ptr).ok()?;
        let mut env = vm
            .get_env()
            .or_else(|_| vm.attach_current_thread_permanently())
            .ok()?;
        let activity = JObject::from_raw(act_ptr);
        let jkey = env.new_string(key).ok()?;
        let res = env
            .call_method(
                &activity,
                "openSecret",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&jkey)],
            )
            .ok()
            .and_then(|v| v.l().ok())
            .and_then(|obj| {
                if obj.is_null() {
                    None
                } else {
                    env.get_string(&JString::from(obj)).ok().map(String::from)
                }
            })
            .filter(|s| !s.is_empty());
        std::mem::forget(activity);
        res
    }
}

fn jni_seal_secret(key: &str, plaintext: &str) {
    let Some(app) = ANDROID_APP_FOR_IME.get() else {
        return;
    };
    unsafe {
        let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
        let act_ptr = app.activity_as_ptr() as jni::sys::jobject;
        if vm_ptr.is_null() || act_ptr.is_null() {
            return;
        }
        let Ok(vm) = jni::JavaVM::from_raw(vm_ptr) else {
            return;
        };
        let Ok(mut env) = vm.get_env().or_else(|_| vm.attach_current_thread_permanently()) else {
            return;
        };
        let activity = JObject::from_raw(act_ptr);
        if let (Ok(jk), Ok(jv)) = (env.new_string(key), env.new_string(plaintext)) {
            let _ = env.call_method(
                &activity,
                "sealSecret",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&jk), JValue::Object(&jv)],
            );
        }
        std::mem::forget(activity);
    }
}

fn jni_delete_secret(key: &str) {
    call_java_string_method("deleteSecret", key);
}

fn notify_java_msa_update(
    status: &str,
    user_code: &str,
    verification_uri: &str,
    in_progress: bool,
    completed: bool,
) {
    let Some(app) = ANDROID_APP_FOR_IME.get() else {
        return;
    };
    unsafe {
        let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
        let act_ptr = app.activity_as_ptr() as jni::sys::jobject;
        if vm_ptr.is_null() || act_ptr.is_null() {
            return;
        }
        let Ok(vm) = jni::JavaVM::from_raw(vm_ptr) else {
            return;
        };
        let Ok(mut env) = vm.get_env().or_else(|_| vm.attach_current_thread_permanently()) else {
            return;
        };
        let activity = JObject::from_raw(act_ptr);
        if let (Ok(js), Ok(jc), Ok(ju)) = (
            env.new_string(status),
            env.new_string(user_code),
            env.new_string(verification_uri),
        ) {
            let _ = env.call_method(
                &activity,
                "onMsaSignInUpdate",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ZZ)V",
                &[
                    JValue::Object(&js),
                    JValue::Object(&jc),
                    JValue::Object(&ju),
                    JValue::Bool(u8::from(in_progress)),
                    JValue::Bool(u8::from(completed)),
                ],
            );
        }
        std::mem::forget(activity);
    }
}

fn jni_get_clipboard() -> Option<String> {
    let app = ANDROID_APP_FOR_IME.get()?;
    unsafe {
        let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
        let act_ptr = app.activity_as_ptr() as jni::sys::jobject;
        if vm_ptr.is_null() || act_ptr.is_null() {
            return None;
        }
        let vm = jni::JavaVM::from_raw(vm_ptr).ok()?;
        let mut env = vm
            .get_env()
            .or_else(|_| vm.attach_current_thread_permanently())
            .ok()?;
        let activity = JObject::from_raw(act_ptr);
        let res = env
            .call_method(&activity, "getClipboardText", "()Ljava/lang/String;", &[])
            .ok()
            .and_then(|v| v.l().ok())
            .and_then(|obj| {
                if obj.is_null() {
                    None
                } else {
                    env.get_string(&JString::from(obj))
                        .ok()
                        .map(|s| String::from(s))
                }
            });
        std::mem::forget(activity);
        res
    }
}

fn jni_set_clipboard(text: &str) {
    call_java_string_method("setClipboardText", text);
}

fn notify_java_ime(active: bool) {
    call_java_bool_method("setImeActive", active);
}

fn notify_java_keep_screen_on(keep_on: bool) {
    call_java_bool_method("setKeepScreenOn", keep_on);
}

fn notify_java_toast(message: &str) {
    call_java_string_method("showToast", message);
}

fn notify_java_open_account_dialog() {
    call_java_void_method("openAccountCenterDialog");
}

fn notify_java_boot_progress(stage: &str, percent: i32) {
    let Some(app) = ANDROID_APP_FOR_IME.get() else {
        return;
    };
    unsafe {
        let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
        let act_ptr = app.activity_as_ptr() as jni::sys::jobject;
        if vm_ptr.is_null() || act_ptr.is_null() {
            return;
        }
        let Ok(vm) = jni::JavaVM::from_raw(vm_ptr) else {
            return;
        };
        let Ok(mut env) = vm.get_env().or_else(|_| vm.attach_current_thread_permanently()) else {
            return;
        };
        let activity = JObject::from_raw(act_ptr);
        if let Ok(jstr) = env.new_string(stage) {
            let _ = env.call_method(
                &activity,
                "updateBootProgress",
                "(Ljava/lang/String;I)V",
                &[JValue::Object(&jstr), JValue::Int(percent)],
            );
        }
        std::mem::forget(activity);
    }
}

fn notify_java_boot_ready() {
    call_java_void_method("dismissBootSplash");
}

#[used]
#[unsafe(link_section = ".init_array")]
static INIT_RUST_MIN_STACK: extern "C" fn() = {
    extern "C" fn set_min_stack() {
        unsafe {
            libc::setenv(c"RUST_MIN_STACK".as_ptr(), c"16777216".as_ptr(), 1);
        }
    }
    set_min_stack
};

fn extract_bundled_resources(app: &AndroidApp, target_dir: &Path) -> Result<(), String> {
    let marker = target_dir.join(".extracted_26_3");
    if marker.is_file() && target_dir.join("horizon-resources.json").is_file() {
        notify_java_boot_progress("正在启动引擎…", 18);
        return Ok(());
    }
    let _ = fs::remove_file(&marker);
    notify_java_boot_progress("首次启动：正在解压游戏资源 (0%)…", 2);
    fs::create_dir_all(target_dir).map_err(|e| format!("create resources dir: {e}"))?;
    let assets = app.asset_manager();
    let asset = assets
        .open(c"resources.zip")
        .ok_or_else(|| "missing assets/resources.zip in APK".to_owned())?;
    let mut archive =
        zip::ZipArchive::new(asset).map_err(|e| format!("open resources.zip: {e}"))?;

    let total = archive.len().max(1);
    let mut last_pct = 0usize;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;
        let Some(rel) = file.enclosed_name() else {
            continue;
        };
        let out_path = target_dir.join(rel);
        if file.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|e| format!("mkdir {}: {e}", out_path.display()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            let out_file = fs::File::create(&out_path)
                .map_err(|e| format!("create {}: {e}", out_path.display()))?;
            let mut writer = io::BufWriter::with_capacity(64 * 1024, out_file);
            io::copy(&mut file, &mut writer)
                .map_err(|e| format!("write {}: {e}", out_path.display()))?;
        }
        let pct = ((i + 1) * 100) / total;
        if pct >= last_pct + 2 || i + 1 == total {
            last_pct = pct;
            let bar_pct = 2 + ((pct as i32) * 43) / 100;
            notify_java_boot_progress(&format!("首次启动：正在解压游戏资源 ({pct}%)…"), bar_pct);
        }
    }
    if !target_dir.join("horizon-resources.json").is_file() {
        return Err("extracted archive did not contain horizon-resources.json".to_owned());
    }
    fs::write(&marker, b"ok").map_err(|e| format!("write marker: {e}"))?;
    Ok(())
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    run_android(app);
}

fn run_android(app: AndroidApp) {
    let base_dir = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/horizon"));
    let crash_path = base_dir.join("crash_android.log");
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("Horizon Android panic: {info}\n");
        let _ = fs::write(&crash_path, &msg);
        eprintln!("{msg}");
    }));

    horizon_guest_stack::prepare_main();
    horizon_mod_host::exit_if_compile_child();

    let resources_dir = base_dir.join("resources").join("26.3");
    let game_dir = base_dir.join("game");
    let _ = fs::create_dir_all(&game_dir);
    let _ = std::env::set_current_dir(&game_dir);
    let _ = ANDROID_GAME_DIR.set(game_dir.clone());

    register_ime_natives(&app);
    let _ = ANDROID_APP_FOR_IME.set(app.clone());
    set_android_ime_callback(notify_java_ime);
    set_android_keep_screen_on_callback(notify_java_keep_screen_on);
    set_android_toast_callback(notify_java_toast);
    set_android_boot_callbacks(notify_java_boot_progress, notify_java_boot_ready);
    set_android_account_dialog_callback(notify_java_open_account_dialog);
    set_android_clipboard_callbacks(jni_get_clipboard, jni_set_clipboard);
    msa::set_secret_store_callbacks(jni_open_secret, jni_seal_secret, jni_delete_secret);

    if let Err(err) = extract_bundled_resources(&app, &resources_dir) {
        let _ = fs::write(base_dir.join("extract_error.log"), &err);
        eprintln!("Horizon Android resource extraction failed: {err}");
        notify_java_boot_progress(&format!("资源准备异常: {err}"), 45);
    }

    // Resolve saved account mode: defaults to instant Offline Mode ("Player") with zero login gate!
    // Only touches the network if the player explicitly left a Microsoft account selected and its
    // token has < 1 hour remaining.
    let saved_store = msa::AccountStore::load(&game_dir);
    if matches!(saved_store.mode, msa::AccountMode::Microsoft { .. }) {
        notify_java_boot_progress("正在恢复正版账号会话…", 47);
    }
    let identity = msa::resolve_and_activate(&game_dir, &msa::Endpoints::live(), false);
    if let Some(note) = &identity.notice {
        notify_java_toast(note);
    }

    notify_java_boot_progress("正在初始化核心运行环境…", 49);
    horizon_client::entry::set_android_app(app);
    let args = vec![
        "--resources".to_owned(),
        resources_dir.to_string_lossy().into_owned(),
        "--game-dir".to_owned(),
        game_dir.to_string_lossy().into_owned(),
        "--fullscreen".to_owned(),
        "--name".to_owned(),
        identity.player_name,
    ];
    let startup = horizon_client::entry::prepare_with_args(&args);
    horizon_mod_host::main_with(Vec::new(), &[], &[], startup);
}
