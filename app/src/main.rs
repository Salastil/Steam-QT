//! Steam-QT: a library-only Steam client for Linux.

mod bridge;
mod worker;

unsafe extern "C" {
    fn steamqt_run(argc: std::ffi::c_int, argv: *mut *mut std::ffi::c_char) -> std::ffi::c_int;
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("STEAMQT_LOG")
                .unwrap_or_else(|_| "warn,steamqt_core=info,steam_qt=info".into()),
        )
        .init();
    cxx_qt::init_qml_module!("org.steamqt");

    let args: Vec<std::ffi::CString> = std::env::args_os()
        .map(|a| std::ffi::CString::new(a.into_encoded_bytes()).unwrap_or_default())
        .collect();
    let mut argv: Vec<*mut std::ffi::c_char> = args.iter().map(|a| a.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    // SAFETY: argv holds NUL-terminated strings that outlive the call (Qt keeps the pointers
    // for the application's lifetime, and `args` lives until main returns).
    let code = unsafe { steamqt_run(args.len() as std::ffi::c_int, argv.as_mut_ptr()) };
    std::process::exit(code);
}
