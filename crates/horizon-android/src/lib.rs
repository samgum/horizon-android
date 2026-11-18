use android_activity::AndroidApp;
use horizon_client::entry::{
    AndroidImeEvent, AndroidWindowInsets, push_android_ime, set_android_boot_callbacks,
    set_android_clipboard_callbacks, set_android_ime_callback, set_android_keep_screen_on_callback,
    set_android_toast_callback, set_android_window_insets,
};
use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString, JValue};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static ANDROID_APP_FOR_IME: OnceLock<AndroidApp> = OnceLock::new();

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
            ];
            if let Err(e) = env.register_native_methods(cls, &methods) {
                eprintln!("Horizon Android register_native_methods failed: {e}");
            }
        }
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
        let _ = env.call_method(&activity, "dismissBootSplash", "()V", &[]);
        std::mem::forget(activity);
    }
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

    register_ime_natives(&app);
    let _ = ANDROID_APP_FOR_IME.set(app.clone());
    set_android_ime_callback(notify_java_ime);
    set_android_keep_screen_on_callback(notify_java_keep_screen_on);
    set_android_toast_callback(notify_java_toast);
    set_android_boot_callbacks(notify_java_boot_progress, notify_java_boot_ready);
    set_android_clipboard_callbacks(jni_get_clipboard, jni_set_clipboard);

    if let Err(err) = extract_bundled_resources(&app, &resources_dir) {
        let _ = fs::write(base_dir.join("extract_error.log"), &err);
        eprintln!("Horizon Android resource extraction failed: {err}");
        notify_java_boot_progress(&format!("资源准备异常: {err}"), 45);
    }

    notify_java_boot_progress("正在初始化核心运行环境…", 48);
    horizon_client::entry::set_android_app(app);
    let args = vec![
        "--resources".to_owned(),
        resources_dir.to_string_lossy().into_owned(),
        "--game-dir".to_owned(),
        game_dir.to_string_lossy().into_owned(),
        "--fullscreen".to_owned(),
        "--name".to_owned(),
        "Player".to_owned(),
    ];
    let startup = horizon_client::entry::prepare_with_args(&args);
    horizon_mod_host::main_with(Vec::new(), &[], &[], startup);
}
