#[cfg(target_os = "windows")]
use crate::client::translate;
#[cfg(not(debug_assertions))]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::platform::breakdown_callback;
use base::config::keys;
#[cfg(not(debug_assertions))]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use base::platform::register_breakdown_handler;
use hbb_common::{config, log};
#[cfg(windows)]
use tauri_winrt_notification::{Duration, Sound, Toast};

#[macro_export]
macro_rules! my_println{
    ($($arg:tt)*) => {
        #[cfg(not(windows))]
        println!("{}", format_args!($($arg)*));
        #[cfg(windows)]
        crate::platform::message_box(
            &format!("{}", format_args!($($arg)*))
        );
    };
}

pub enum CoreMainAction {
    StartUi(Vec<String>),
    ExitFailure(hbb_common::anyhow::Error),
}

#[cfg(target_os = "macos")]
fn finish_macos_update(result: hbb_common::ResultType<()>) -> Option<CoreMainAction> {
    match result {
        Ok(()) => None,
        Err(err) => Some(CoreMainAction::ExitFailure(err)),
    }
}

/// Shared by the Flutter main function and command-line startup.
///
/// [Note]
/// If it returns [`None`], then the process will terminate, and flutter gui will not be started.
/// If it returns [`Some`], the action determines whether to start the UI or report failure.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn core_main() -> Option<CoreMainAction> {
    // Own name, so config, logs and IPC sockets never collide with a stock SealDesk: the two
    // cannot talk to each other anyway (no rendezvous server here).
    *hbb_common::config::APP_NAME.write().unwrap() = crate::tailcat::APP_NAME.to_owned();
    if !crate::common::global_init() {
        return None;
    }
    #[cfg(windows)]
    if !crate::platform::windows::bootstrap() {
        // return None to terminate the process
        return None;
    }
    let mut args = Vec::new();
    let mut flutter_args = Vec::new();
    let mut i = 0;
    let mut _is_elevate = false;
    let mut _is_run_as_system = false;
    let mut _is_quick_support = false;
    let mut _is_flutter_invoke_new_connection = false;
    let mut no_server = false;
    let mut arg_exe = Default::default();
    for arg in std::env::args() {
        if i == 0 {
            arg_exe = arg;
        } else if i > 0 {
            #[cfg(feature = "flutter")]
            if [
                "--connect",
                "--play",
                "--file-transfer",
                "--view-camera",
                "--port-forward",
                "--terminal",
                "--rdp",
            ]
            .contains(&arg.as_str())
            {
                _is_flutter_invoke_new_connection = true;
            }
            if arg == "--elevate" {
                _is_elevate = true;
            } else if arg == "--run-as-system" {
                _is_run_as_system = true;
            } else if arg == "--quick_support" {
                _is_quick_support = true;
            } else if arg == "--no-server" {
                no_server = true;
            } else {
                args.push(arg);
            }
        }
        i += 1;
    }
    let sealdesk_launch = match crate::tailcat::launch::Launch::parse(&args, no_server) {
        Ok(launch) => launch,
        Err(error) => return Some(CoreMainAction::ExitFailure(error)),
    };
    if sealdesk_launch.is_some() {
        args.clear();
        _is_flutter_invoke_new_connection = false;
        #[cfg(feature = "flutter")]
        if let Some(input) = sealdesk_launch
            .as_ref()
            .and_then(|launch| launch.input.as_ref())
        {
            flutter_args = vec!["--control".to_owned(), input.clone()];
        }
    }
    #[cfg(all(target_os = "linux", feature = "flutter"))]
    if matches!(args.first().map(String::as_str),
        Some("--service" | "--server" | "--tray" | "--install-service" | "--uninstall-service"))
    {
        return Some(CoreMainAction::ExitFailure(hbb_common::anyhow::anyhow!(
            "SealDesk runs as a desktop app; open Share this device or use --host"
        )));
    }
    let startup_mode = crate::tailcat::launch::startup_mode(sealdesk_launch.as_ref())
        .as_str().to_owned();
    no_server = crate::tailcat::launch::controller_only(&startup_mode, no_server);
    #[cfg(target_os = "windows")]
    if args.is_empty() {
        let should_check_start_tray = crate::platform::is_self_service_running()
            && crate::platform::is_cur_exe_the_installed();
        if should_check_start_tray && !crate::check_process("--tray", true) {
            hbb_common::allow_err!(crate::run_me(vec!["--tray"]));
        }
    }
    #[cfg(not(debug_assertions))]
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    register_breakdown_handler(breakdown_callback);
    #[cfg(target_os = "linux")]
    #[cfg(feature = "flutter")]
    {
        let (k, v) = ("LIBGL_ALWAYS_SOFTWARE", "1");
        if config::option2bool(
            "allow-always-software-render",
            &config::Config::get_option("allow-always-software-render"),
        ) {
            std::env::set_var(k, v);
        } else {
            std::env::remove_var(k);
        }
    }
    #[cfg(windows)]
    if args.contains(&"--connect".to_string()) || args.contains(&"--view-camera".to_string()) {
        base::platform::windows::start_cpu_performance_monitor();
    }
    #[cfg(feature = "flutter")]
    if _is_flutter_invoke_new_connection {
        return core_main_invoke_new_connection(std::env::args()).map(CoreMainAction::StartUi);
    }
    let click_setup = cfg!(windows)
        && sealdesk_launch.is_none()
        && args.is_empty()
        && crate::common::is_setup(&arg_exe);
    if click_setup && !config::is_disable_installation() {
        args.push("--install".to_owned());
        flutter_args.push("--install".to_string());
    }
    if args.contains(&"--noinstall".to_string()) {
        args.clear();
    }
    // The portable wrapper injects `--install` when its name ends with `install.exe`,
    // including `no-install.exe`. Drop the argument instead of exiting so disabled
    // clients can continue running as portable applications.
    if config::is_disable_installation() {
        args.retain(|arg| arg != "--install");
        flutter_args.retain(|arg| arg != "--install");
    }
    if args.len() > 0 {
        if args[0] == "--version" {
            println!("{}", crate::VERSION);
            return None;
        } else if args[0] == "--build-date" {
            println!("{}", crate::BUILD_DATE);
            return None;
        }
    }
    #[cfg(windows)]
    {
        _is_quick_support |= !crate::platform::is_installed()
            && args.is_empty()
            && (is_quick_support_exe(&arg_exe)
                || config::LocalConfig::get_option("pre-elevate-service") == "Y"
                || (!click_setup && crate::platform::is_elevated(None).unwrap_or(false)));
        crate::portable_service::client::set_quick_support(_is_quick_support);
    }
    let mut log_name = "".to_owned();
    // Keep portable-service logs under a stable directory name.
    let has_portable_service_shmem_arg = args
        .iter()
        .any(|arg| arg.starts_with("--portable-service-shmem-name="));
    if has_portable_service_shmem_arg {
        log_name = "portable-service".to_owned();
    } else if args.len() > 0 && args[0].starts_with("--") {
        let name = args[0].replace("--", "");
        if !name.is_empty() {
            log_name = name;
        }
    }
    hbb_common::init_log(false, &log_name);

    // linux uni (url) go here.
    #[cfg(all(target_os = "linux", feature = "flutter"))]
    if args.len() > 0 && args[0].starts_with(&crate::get_uri_prefix()) {
        return try_send_by_dbus(args[0].clone()).map(CoreMainAction::StartUi);
    }

    #[cfg(windows)]
    if !crate::platform::is_installed()
        && args.is_empty()
        && !no_server
        && _is_quick_support
        && !_is_elevate
        && !_is_run_as_system
    {
        use crate::portable_service::client;
        if let Err(e) = client::start_portable_service(client::StartPara::Direct) {
            log::error!("Failed to start portable service: {:?}", e);
        }
    }
    #[cfg(windows)]
    if !crate::platform::is_installed() && (_is_elevate || _is_run_as_system) {
        crate::platform::elevate_or_run_as_system(click_setup, _is_elevate, _is_run_as_system);
        return None;
    }
    if args.is_empty() || crate::common::is_empty_uni_link(&args[0]) {
        let requested = if no_server { "control" } else { "host" };
        if let Err(error) = crate::tailcat::host::request_mode(requested) {
            return Some(CoreMainAction::ExitFailure(error));
        }
        config::LocalConfig::set_option(
            keys::OPTION_SEALDESK_MODE.to_owned(),
            requested.to_owned(),
        );
        #[cfg(target_os = "linux")]
        hbb_common::allow_err!(crate::platform::check_autostart_config());
        #[cfg(target_os = "macos")]
        {
            crate::platform::macos::try_remove_temp_update_dir(None);
        }

        #[cfg(windows)]
        {
            crate::platform::try_remove_temp_update_files();
            hbb_common::config::PeerConfig::preload_peers();
        }
        std::thread::spawn(move || crate::start_server(false, no_server));
    } else {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        // Root CLI management commands must talk to the user `--server` main IPC.
        // Example: `sudo rustdesk --option custom-rendezvous-server` should query the
        // user's IPC instead of root's `/tmp/<app>-0/ipc`; `connect()` still limits this
        // routing to empty-postfix main IPC only.
        let _user_main_ipc_scope = if crate::platform::is_installed()
            && is_root()
            && is_user_main_ipc_scope_cli_command(&args)
        {
            Some(crate::ipc::UserMainIpcScope::new())
        } else {
            None
        };

        #[cfg(windows)]
        {
            use crate::platform;
            if args[0] == "--uninstall" {
                if let Err(err) = platform::uninstall_me(true) {
                    log::error!("Failed to uninstall: {}", err);
                }
                return None;
            } else if args[0] == "--update" {
                // The elevated updater cannot delete its running verified copy.
                if let Err(err) = platform::schedule_current_verified_update_file_cleanup() {
                    log::warn!("Failed to schedule verified update file cleanup: {}", err);
                }
                if config::is_disable_installation() {
                    return None;
                }

                let text = match crate::platform::prepare_custom_client_update() {
                    Err(e) => {
                        log::error!("Error preparing custom client update: {}", e);
                        "Update failed!".to_string()
                    }
                    Ok(false) => "Update failed!".to_string(),
                    Ok(true) => match platform::update_me(false) {
                        Ok(_) => "Updated successfully!".to_string(),
                        Err(err) => {
                            log::error!("Failed with error: {err}");
                            "Update failed!".to_string()
                        }
                    },
                };
                Toast::new(Toast::POWERSHELL_APP_ID)
                    .title(&config::APP_NAME.read().unwrap())
                    .text1(&translate(text))
                    .sound(Some(Sound::Default))
                    .duration(Duration::Short)
                    .show()
                    .ok();
                return None;
            } else if args[0] == "--after-install" {
                if let Err(err) = platform::run_after_install() {
                    log::error!("Failed to after-install: {}", err);
                }
                return None;
            } else if args[0] == "--before-uninstall" {
                if let Err(err) = platform::run_before_uninstall() {
                    log::error!("Failed to before-uninstall: {}", err);
                }
                return None;
            } else if args[0] == "--silent-install" {
                if config::is_disable_installation() {
                    return None;
                }
                let (printer_override, debug) = parse_silent_install_args(&args);
                let options = platform::get_silent_install_options(printer_override);
                let res = platform::install_me(options, "".to_owned(), true, debug);
                let text = match res {
                    Ok(_) => translate("Installation Successful!".to_string()),
                    Err(err) => {
                        println!("Failed with error: {err}");
                        translate("Installation failed!".to_string())
                    }
                };
                Toast::new(Toast::POWERSHELL_APP_ID)
                    .title(&config::APP_NAME.read().unwrap())
                    .text1(&text)
                    .sound(Some(Sound::Default))
                    .duration(Duration::Short)
                    .show()
                    .ok();
                return None;
            } else if args[0] == "--uninstall-cert" {
                #[cfg(windows)]
                hbb_common::allow_err!(crate::platform::windows::uninstall_cert());
                return None;
            } else if args[0] == "--install-idd" {
                #[cfg(windows)]
                if crate::virtual_display_manager::is_virtual_display_supported() {
                    hbb_common::allow_err!(
                        crate::virtual_display_manager::rustdesk_idd::install_update_driver()
                    );
                }
                return None;
            } else if args[0] == "--portable-service" {
                crate::platform::elevate_or_run_as_system(
                    click_setup,
                    _is_elevate,
                    _is_run_as_system,
                );
                return None;
            } else if args[0] == "--uninstall-amyuni-idd" {
                #[cfg(windows)]
                hbb_common::allow_err!(
                    crate::virtual_display_manager::amyuni_idd::uninstall_driver()
                );
                return None;
            } else if args[0] == "--install-remote-printer" {
                #[cfg(windows)]
                if crate::platform::is_win_10_or_greater() {
                    match remote_printer::install_update_printer(&crate::get_app_name()) {
                        Ok(_) => {
                            log::info!("Remote printer installed/updated successfully");
                        }
                        Err(e) => {
                            log::error!("Failed to install/update the remote printer: {}", e);
                        }
                    }
                } else {
                    log::error!("Win10 or greater required!");
                }
                return None;
            } else if args[0] == "--uninstall-remote-printer" {
                #[cfg(windows)]
                if crate::platform::is_win_10_or_greater() {
                    remote_printer::uninstall_printer(&crate::get_app_name());
                    log::info!("Remote printer uninstalled");
                }
                return None;
            }
        }
        #[cfg(target_os = "macos")]
        {
            if args[0] == "--update" {
                return finish_macos_update(crate::manual_update::execute_macos_update(&args));
            }
        }
        if args[0] == "--remove" {
            if args.len() == 2 {
                // sleep a while so that process of removed exe exit
                std::thread::sleep(std::time::Duration::from_secs(1));
                std::fs::remove_file(&args[1]).ok();
                return None;
            }
        } else if args[0] == "--tray" {
            if !crate::check_process("--tray", true) {
                crate::tray::start_tray();
            }
            return None;
        } else if args[0] == "--install-service" {
            log::info!("start --install-service");
            crate::platform::install_service();
            return None;
        } else if args[0] == "--uninstall-service" {
            log::info!("start --uninstall-service");
            crate::platform::uninstall_service(false, true);
            return None;
        } else if args[0] == "--service" {
            log::info!("start --service");
            crate::start_os_service();
            return None;
        } else if args[0] == "--server" {
            log::info!("start --server with user {}", crate::username());
            #[cfg(windows)]
            crate::privacy_mode::restore_reg_connectivity(true, false);
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            {
                crate::start_server(true, false);
            }
            #[cfg(target_os = "macos")]
            {
                let handler = std::thread::spawn(move || crate::start_server(true, false));
                crate::tray::start_tray();
                // prevent server exit when encountering errors from tray
                hbb_common::allow_err!(handler.join());
            }
            return None;
        } else if args[0] == "--password" {
            if is_cli_setting_change_disabled() {
                println!("Settings are disabled!");
                return None;
            }
            if config::Config::is_disable_change_permanent_password() {
                println!("Changing permanent password is disabled!");
                return None;
            }
            if args.len() == 2 {
                // A portable (not installed) server runs as this user, so this user may set it.
                if !crate::platform::is_installed() || is_root() {
                    if let Err(err) = crate::ipc::set_permanent_password(args[1].to_owned()) {
                        println!("{err}");
                    } else {
                        println!("Done!");
                    }
                } else {
                    println!("Installation and administrative privileges required!");
                }
            }
            return None;
        } else if args[0] == "--set-unlock-pin" {
            if config::Config::is_disable_unlock_pin() {
                println!("Unlock PIN is disabled!");
                return None;
            }
            #[cfg(feature = "flutter")]
            if args.len() == 2 {
                if crate::platform::is_installed() && is_root() {
                    if let Err(err) = crate::ipc::set_unlock_pin(args[1].to_owned(), false) {
                        println!("{err}");
                    } else {
                        println!("Done!");
                    }
                } else {
                    println!("Installation and administrative privileges required!");
                }
            }
            return None;
        } else if args[0] == "--tc-reset-address" {
            if args.len() != 1 || is_cli_setting_change_disabled() {
                return Some(CoreMainAction::ExitFailure(hbb_common::anyhow::anyhow!(
                    "用法：--tc-reset-address；需要允许修改设置。此操作会断开被控会话并使旧二维码失效"
                )));
            }
            return match crate::tailcat::host::reset_address() {
                Ok(()) => {
                    println!("地址已重置，旧二维码已失效；被控服务会按原运行状态恢复，请重新获取配对码");
                    None
                }
                Err(error) => Some(CoreMainAction::ExitFailure(hbb_common::anyhow::anyhow!(
                    "地址重置未完成，请确认被控服务状态后重试：{error}"
                ))),
            };
        } else if args[0] == "--tc-pair" {
            return match crate::tailcat::print_pairing_code(&args) {
                Ok(()) => None,
                Err(err) => Some(CoreMainAction::ExitFailure(err)),
            };
        } else if args[0] == "--tc-addr" {
            let addr = crate::ui_interface::get_option(crate::tailcat::OPTION_ADDR.to_owned());
            if addr.is_empty() {
                println!("No tailcat address yet; is the SealDesk server running?");
            } else {
                if let Ok(qr) = crate::tailcat::qr_text(&addr) {
                    println!("{qr}");
                }
                println!("{addr}");
            }
            return None;
        } else if args[0] == "--tc-pubkey" {
            match crate::tailcat::client_pubkey() {
                Ok(k) => println!("{k}"),
                Err(err) => println!("{err}"),
            }
            return None;
        } else if args[0] == "--tc-allow" {
            if is_cli_setting_change_disabled() {
                println!("Settings are disabled!");
                return None;
            }
            let keys = args.get(1).cloned().unwrap_or_default();
            crate::ui_interface::set_option(crate::tailcat::OPTION_ALLOW.to_owned(), keys);
            println!("Done! Restart the SealDesk server to apply.");
            return None;
        } else if args[0] == "--get-id" {
            println!("{}", crate::ipc::get_id());
            return None;
        } else if args[0] == "--set-id" {
            if is_cli_setting_change_disabled() {
                println!("Settings are disabled!");
                return None;
            }
            if config::Config::is_disable_change_id() {
                println!("Changing ID is disabled!");
                return None;
            }
            if args.len() == 2 {
                if crate::platform::is_installed() && is_root() {
                    let mut res = crate::ui_interface::change_id_shared(args[1].to_owned());
                    if res.is_empty() {
                        res = "Done!".to_owned();
                    }
                    println!("{}", res);
                } else {
                    println!("Installation and administrative privileges required!");
                }
            }
            return None;
        } else if args[0] == "--option" {
            if is_cli_setting_change_disabled() {
                println!("Settings are disabled!");
                return None;
            }
            if crate::platform::is_installed() && is_root() {
                if args.len() == 2 {
                    let options = crate::ipc::get_options();
                    println!("{}", options.get(&args[1]).unwrap_or(&"".to_owned()));
                } else if args.len() == 3 {
                    crate::ipc::set_option(&args[1], &args[2]);
                }
            } else {
                println!("Installation and administrative privileges required!");
            }
            return None;
        } else if args[0] == "--check-hwcodec-config" {
            #[cfg(feature = "hwcodec")]
            crate::ipc::hwcodec_process();
            return None;
        } else if args[0] == "--terminal-helper" {
            // Terminal helper process - runs as user to create ConPTY
            // This is needed because ConPTY has compatibility issues with CreateProcessAsUserW
            #[cfg(target_os = "windows")]
            {
                let helper_args: Vec<String> = args[1..].to_vec();
                if let Err(e) = crate::server::terminal_helper::run_terminal_helper(&helper_args) {
                    log::error!("Terminal helper failed: {}", e);
                }
            }
            return None;
        } else if args[0] == "--cm" {
            // call connection manager to establish connections
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            crate::ui_interface::start_option_status_sync();
        } else if args[0] == "--whiteboard" {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                crate::whiteboard::run();
            }
            return None;
        } else if args[0] == "-gtk-sudo" {
            // rustdesk service kill `rustdesk --` processes
            #[cfg(target_os = "linux")]
            if args.len() > 2 {
                crate::platform::gtk_sudo::exec();
            }
            return None;
        }
    }
    //_async_logger_holder.map(|x| x.flush());
    #[cfg(not(feature = "flutter"))]
    if let Some(input) = sealdesk_launch.and_then(|launch| launch.input) {
        let pairing = match crate::tailcat::pairing::PairingCode::parse(&input) {
            Ok(pairing) => pairing,
            Err(error) => return Some(CoreMainAction::ExitFailure(error.into())),
        };
        if let Some(name) = pairing.name {
            crate::ui_interface::set_peer_option(
                pairing.address.clone(), "alias".to_owned(), name,
            );
        }
        args = vec![
            "--connect".to_owned(),
            pairing.address,
            pairing.password.unwrap_or_default(),
        ];
    }
    #[cfg(feature = "flutter")]
    return Some(CoreMainAction::StartUi(flutter_args));
    #[cfg(not(feature = "flutter"))]
    return Some(CoreMainAction::StartUi(args));
}

/// invoke a new connection
///
/// [Note]
/// this is for invoke new connection from dbus.
/// If it returns [`None`], then the process will terminate, and flutter gui will not be started.
/// If it returns [`Some`], then the process will continue, and flutter gui will be started.
#[cfg(feature = "flutter")]
fn core_main_invoke_new_connection(mut args: std::env::Args) -> Option<Vec<String>> {
    let mut authority = None;
    let mut id = None;
    let mut param_array = vec![];
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--connect" | "--play" | "--file-transfer" | "--view-camera" | "--port-forward"
            | "--terminal" | "--rdp" => {
                authority = Some((&arg.to_string()[2..]).to_owned());
                id = args.next();
            }
            "--password" => {
                if let Some(password) = args.next() {
                    param_array.push(format!("password={password}"));
                }
            }
            // inner
            "--switch_uuid" => {
                if let Some(switch_uuid) = args.next() {
                    param_array.push(format!("switch_uuid={switch_uuid}"));
                }
            }
            _ => {}
        }
    }
    let mut uni_links = Default::default();
    if let Some(authority) = authority {
        if let Some(mut id) = id {
            let app_name = crate::get_app_name();
            let ext = format!(".{}", app_name.to_lowercase());
            if id.ends_with(&ext) {
                id = id.replace(&ext, "");
            }
            let params = param_array.join("&");
            let params_flag = if params.is_empty() { "" } else { "?" };
            uni_links = format!(
                "{}{}/{}{}{}",
                crate::get_uri_prefix(),
                authority,
                id,
                params_flag,
                params
            );
        }
    }
    if uni_links.is_empty() {
        return None;
    }

    #[cfg(target_os = "linux")]
    return try_send_by_dbus(uni_links);

    #[cfg(windows)]
    {
        use winapi::um::winuser::WM_USER;
        let res = crate::platform::send_message_to_hnwd(
            &crate::platform::FLUTTER_RUNNER_WIN32_WINDOW_CLASS,
            &crate::get_app_name(),
            (WM_USER + 2) as _, // referred from unilinks desktop pub
            uni_links.as_str(),
            false,
        );
        return if res { None } else { Some(Vec::new()) };
    }
    #[cfg(target_os = "macos")]
    {
        return if let Err(_) = crate::ipc::send_url_scheme(uni_links) {
            Some(Vec::new())
        } else {
            None
        };
    }
}

#[cfg(all(target_os = "linux", feature = "flutter"))]
fn try_send_by_dbus(uni_links: String) -> Option<Vec<String>> {
    use crate::dbus::invoke_new_connection;

    match invoke_new_connection(uni_links) {
        Ok(()) => {
            return None;
        }
        Err(err) => {
            log::error!("{}", err.as_ref());
            // return Some to invoke this url by self
            return Some(Vec::new());
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn is_root() -> bool {
    #[cfg(windows)]
    {
        return crate::platform::is_elevated(None).unwrap_or_default()
            || crate::platform::is_root();
    }
    #[allow(unreachable_code)]
    crate::platform::is_root()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn is_user_main_ipc_scope_cli_command(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        Some("--password")
            | Some("--set-unlock-pin")
            | Some("--get-id")
            | Some("--tc-addr")
            | Some("--tc-pair")
            | Some("--tc-reset-address")
            | Some("--tc-allow")
            | Some("--set-id")
            | Some("--option")
    )
}

#[inline]
fn is_cli_setting_change_disabled() -> bool {
    let option = keys::OPTION_ALLOW_COMMAND_LINE_SETTINGS_WHEN_SETTINGS_DISABLED;
    let allow_command_line_settings =
        config::option2bool(option, &crate::get_builtin_option(option));
    config::is_disable_settings() && !allow_command_line_settings
}

#[cfg(windows)]
fn parse_silent_install_args(args: &[String]) -> (Option<bool>, bool) {
    let mut printer_override = None;
    let mut debug = false;

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "printer=1" => printer_override = Some(true),
            "printer=0" => printer_override = Some(false),
            "debug" => debug = true,
            _ => {}
        }
    }

    (printer_override, debug)
}

/// Check if the executable is a Quick Support version.
/// Note: This function must be kept in sync with `libs/portable/src/main.rs`.
#[cfg(windows)]
#[inline]
fn is_quick_support_exe(exe: &str) -> bool {
    let exe = exe.to_lowercase();
    exe.contains("-qs-") || exe.contains("-qs.exe") || exe.contains("_qs.exe")
}
