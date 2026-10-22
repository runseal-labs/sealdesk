use crate::client::translate;
#[cfg(windows)]
use crate::ipc::Data;
use base::config::keys;
#[cfg(windows)]
use hbb_common::tokio;
use hbb_common::{allow_err, log};
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::time::Duration;

pub fn start_tray() {
    if crate::ui_interface::get_builtin_option(keys::OPTION_HIDE_TRAY) == "Y" {
        #[cfg(not(target_os = "macos"))]
        {
            return;
        }
    }

    #[cfg(target_os = "linux")]
    crate::server::check_zombie();

    allow_err!(make_tray());
}

fn make_tray() -> hbb_common::ResultType<()> {
    // https://github.com/tauri-apps/tray-icon/blob/dev/examples/tao.rs
    use hbb_common::anyhow::Context;
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem},
        TrayIcon, TrayIconBuilder, TrayIconEvent as TrayEvent,
    };

    // Duplicated tray icons kept piling up through the blind spots of
    // `check_process("--tray", ..)`. https://github.com/rustdesk/rustdesk/issues/15689
    #[cfg(windows)]
    if !crate::platform::windows::try_lock_tray_single_instance() {
        log::info!("Another tray process is already running in this session, exit");
        return Ok(());
    }

    let icon;
    #[cfg(target_os = "macos")]
    {
        icon = include_bytes!("../res/mac-tray-dark-x2.png"); // use as template, so color is not important
    }
    #[cfg(not(target_os = "macos"))]
    {
        icon = include_bytes!("../res/tray-icon.ico");
    }

    let (icon_rgba, icon_width, icon_height) = {
        let image = load_icon_from_asset()
            .unwrap_or(image::load_from_memory(icon).context("Failed to open icon path")?)
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };
    let icon = tray_icon::Icon::from_rgba(icon_rgba, icon_width, icon_height)
        .context("Failed to open icon")?;

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::new().build();

    let tray_menu = Menu::new();
    let open_i = MenuItem::new(translate("Open main window".to_owned()), true, None);
    let id_i = MenuItem::new(translate("Show this device's ID".to_owned()), true, None);
    let quit_i = MenuItem::new(translate("Quit".to_owned()), true, None);
    tray_menu.append_items(&[&open_i, &id_i, &quit_i]).ok();
    let tooltip = |count: usize| {
        if count == 0 {
            format!(
                "{} {}",
                crate::get_app_name(),
                translate("Service is running".to_owned()),
            )
        } else {
            format!(
                "{} - {}\n{}",
                crate::get_app_name(),
                translate("Ready".to_owned()),
                translate("{".to_string() + &format!("{count}") + "} sessions"),
            )
        }
    };
    let mut _tray_icon: Arc<Mutex<Option<TrayIcon>>> = Default::default();

    let menu_channel = MenuEvent::receiver();
    let tray_channel = TrayEvent::receiver();
    #[cfg(windows)]
    let (ipc_sender, ipc_receiver) = std::sync::mpsc::channel::<Data>();

    let open_func = move || {
        if cfg!(not(feature = "flutter")) {
            crate::run_me::<&str>(vec![]).ok();
            return;
        }
        #[cfg(target_os = "macos")]
        crate::platform::macos::handle_application_should_open_untitled_file();
        #[cfg(target_os = "windows")]
        {
            // Do not use "start uni link" way, it may not work on some Windows, and pop out error
            // dialog, I found on one user's desktop, but no idea why, Windows is shit.
            // Use `run_me` instead.
            // `allow_multiple_instances` in `flutter/windows/runner/main.cpp` allows only one instance without args.
            crate::run_me::<&str>(vec![]).ok();
        }
        #[cfg(target_os = "linux")]
        {
            // Do not use "xdg-open", it won't read the config.
            if crate::dbus::invoke_new_connection(crate::get_uri_prefix()).is_err() {
                if let Ok(task) = crate::run_me::<&str>(vec![]) {
                    crate::server::CHILD_PROCESS.lock().unwrap().push(task);
                }
            }
        }
    };

    #[cfg(windows)]
    std::thread::spawn(move || {
        start_query_session_count(ipc_sender.clone());
    });
    #[cfg(windows)]
    let mut last_click = std::time::Instant::now();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::EventLoopExtMacOS;
        event_loop.set_activation_policy(tao::platform::macos::ActivationPolicy::Accessory);
    }
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(100),
        );

        if let tao::event::Event::NewEvents(tao::event::StartCause::Init) = event {
            // for fixing https://github.com/rustdesk/rustdesk/discussions/10210#discussioncomment-14600745
            // so we start tray, but not to show it
            if crate::ui_interface::get_builtin_option(keys::OPTION_HIDE_TRAY) == "Y" {
                return;
            }
            // We create the icon once the event loop is actually running
            // to prevent issues like https://github.com/tauri-apps/tray-icon/issues/90
            #[allow(unused_mut)]
            let mut builder = TrayIconBuilder::new()
                .with_id(crate::get_app_name().to_lowercase())
                .with_menu(Box::new(tray_menu.clone()))
                .with_tooltip(tooltip(0))
                .with_icon(icon.clone());
            #[cfg(target_os = "macos")]
            {
                builder = builder.with_icon_as_template(true);
            }
            #[cfg(target_os = "windows")]
            {
                // Required since tray-icon 0.17
                // Fixes #15215, #15222, #15410
                builder = builder.with_menu_on_left_click(false);
            }
            let tray = builder.build();
            match tray {
                Ok(tray) => _tray_icon = Arc::new(Mutex::new(Some(tray))),
                Err(err) => {
                    log::error!("Failed to create tray icon: {}", err);
                }
            };

            // We have to request a redraw here to have the icon actually show up.
            // Tao only exposes a redraw method on the Window so we use core-foundation directly.
            #[cfg(target_os = "macos")]
            unsafe {
                use core_foundation::runloop::{CFRunLoopGetMain, CFRunLoopWakeUp};

                let rl = CFRunLoopGetMain();
                CFRunLoopWakeUp(rl);
            }
        }

        if let Ok(event) = menu_channel.try_recv() {
            if event.id == quit_i.id() {
                // `process::exit` skips the tray destructor. Hide the icon first
                // or Windows keeps a ghost icon until the pointer passes over it.
                #[cfg(windows)]
                {
                    let _ = _tray_icon
                        .lock()
                        .unwrap()
                        .as_mut()
                        .map(|t| t.set_visible(false));
                    if crate::platform::is_self_service_running()
                        && crate::platform::is_cur_exe_the_installed()
                    {
                        // Upstream tray quit. Writes `stop-service=Y`, stops and
                        // deletes the service, then exits. A failed or cancelled
                        // UAC prompt clears `stop-service` and returns true.
                        if !crate::platform::uninstall_service(false, false) {
                            std::process::exit(0);
                        }
                        let _ = _tray_icon
                            .lock()
                            .unwrap()
                            .as_mut()
                            .map(|t| t.set_visible(true));
                    } else {
                        quit_app();
                    }
                }
                #[cfg(not(windows))]
                quit_app();
            } else if event.id == open_i.id() {
                open_func();
            } else if event.id == id_i.id() {
                show_device_id();
            }
        }

        if let Ok(_event) = tray_channel.try_recv() {
            #[cfg(target_os = "windows")]
            match _event {
                TrayEvent::Click {
                    button,
                    button_state,
                    ..
                } => {
                    if button == tray_icon::MouseButton::Left
                        && button_state == tray_icon::MouseButtonState::Up
                    {
                        if last_click.elapsed() < std::time::Duration::from_secs(1) {
                            return;
                        }
                        open_func();
                        last_click = std::time::Instant::now();
                    }
                }
                _ => {}
            }
        }

        #[cfg(windows)]
        if let Ok(data) = ipc_receiver.try_recv() {
            match data {
                Data::ControlledSessionCount(count) => {
                    _tray_icon
                        .lock()
                        .unwrap()
                        .as_mut()
                        .map(|t| t.set_tooltip(Some(tooltip(count))));
                }
                _ => {}
            }
        }
    });
}

/// Ends this user's SealDesk processes. An installed, running Windows service
/// is stopped earlier with upstream `uninstall_service` instead of this kill.
fn quit_app() {
    terminate_other_instances();
    std::process::exit(0);
}

fn terminate_other_instances() {
    use hbb_common::sysinfo::System;
    let Ok(my_exe) = std::env::current_exe() else {
        return;
    };
    let my_exe = std::fs::canonicalize(&my_exe).unwrap_or(my_exe);
    let my_pid = std::process::id();
    let mut sys = System::new();
    sys.refresh_processes();
    for (pid, process) in sys.processes() {
        if pid.as_u32() == my_pid {
            continue;
        }
        let exe = process.exe();
        if exe.as_os_str().is_empty() {
            continue;
        }
        let exe = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
        if !same_exe(&exe, &my_exe) {
            continue;
        }
        process.kill();
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
}

fn same_exe(left: &std::path::Path, right: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

/// Longest id the dialog must show in full.
///
/// Custom ids match `^[a-zA-Z][\w-]{5,15}$` (16 characters). Numeric ids are shorter.
/// A tailcat address (`tc` + unpadded base64url, `is_addr` requires length > 40) is longer:
/// the default relay embeds one DERP region and encodes to 229 characters.
/// [`crate::common::MAX_UNTRUSTED_PEER_ID_LEN`] is the longest peer id accepted before
/// it is stored or written into a command. That cap covers a tailcat address.
const DEVICE_ID_MAX_CHARS: usize = crate::common::MAX_UNTRUSTED_PEER_ID_LEN;

/// Other lines are cut to this many characters so they stay inside the fixed width.
const DEVICE_ID_DIALOG_OTHER_LINE_MAX_CHARS: usize = DEVICE_ID_MAX_CHARS;

/// Pixels of zenity `--width` per id character. 10px is the same ratio as the old
/// 360px dialog and is wider than a base64url line (about 8.5px), so a max-length
/// id does not stretch the window.
const DEVICE_ID_DIALOG_PX_PER_CHAR: usize = 10;

const DEVICE_ID_DIALOG_WIDTH_PX: usize = DEVICE_ID_MAX_CHARS * DEVICE_ID_DIALOG_PX_PER_CHAR;

/// The first line is the device id and is kept whole. Later lines (TCP and the like)
/// are cut to `max_chars` Unicode scalar values. An over-long line ends with `…`.
fn truncate_dialog_lines(text: &str, max_chars: usize) -> String {
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                line.to_owned()
            } else {
                truncate_dialog_line(line, max_chars)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_dialog_line(line: &str, max_chars: usize) -> String {
    if line.chars().count() <= max_chars {
        return line.to_owned();
    }
    let mut out: String = line.chars().take(max_chars.saturating_sub(1)).collect();
    if max_chars > 0 {
        out.push('\u{2026}');
    }
    out
}

/// Dialog body. The id line is complete. Other lines are truncated and do not wrap.
fn device_id_dialog_body(text: &str) -> String {
    truncate_dialog_lines(text, DEVICE_ID_DIALOG_OTHER_LINE_MAX_CHARS)
}

/// Info dialog for the device id. `--width` and `--no-wrap` keep a fixed width
/// that fits the longest legal id.
#[cfg(any(target_os = "linux", test))]
fn linux_device_id_dialog_args(title: &str, id: &str) -> Vec<String> {
    vec![
        "--info".to_owned(),
        format!("--width={DEVICE_ID_DIALOG_WIDTH_PX}"),
        "--no-wrap".to_owned(),
        "--title".to_owned(),
        title.to_owned(),
        "--text".to_owned(),
        device_id_dialog_body(id),
    ]
}

/// `notify-send` has no width flag. The body is the same text as the other dialogs:
/// the id line is complete.
#[cfg(any(target_os = "linux", test))]
fn linux_notify_send_args(title: &str, id: &str) -> Vec<String> {
    vec![title.to_owned(), device_id_dialog_body(id)]
}

fn show_device_id() {
    let id = device_id_text();
    let title = translate("Show this device's ID".to_owned());
    // Same body on every platform: the id line is complete, other lines are cut.
    let body = device_id_dialog_body(&id);
    #[cfg(windows)]
    crate::platform::message_box_ok(&title, &body);
    #[cfg(target_os = "linux")]
    {
        let notified = std::process::Command::new("notify-send")
            .arg(&title)
            .arg(&body)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !notified {
            let _ = std::process::Command::new("zenity")
                .args(linux_device_id_dialog_args(&title, &id))
                .status();
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Pass the text as argv. AppleScript `\"` is not an escape, so
        // interpolating the id could break out of the string.
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(
                "on run argv\n\
                 display dialog (item 1 of argv) with title (item 2 of argv) \
                 buttons {\"OK\"} default button \"OK\"\n\
                 end run",
            )
            .arg("--")
            .arg(&body)
            .arg(&title)
            .spawn();
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    let _ = (&title, &body);
}

fn device_id_text() -> String {
    let options = crate::ipc::get_options();
    if let Some(addr) = options.get(crate::tailcat::OPTION_ADDR) {
        if !addr.is_empty() {
            return addr.clone();
        }
    }
    if let Some(id) = options.get("id") {
        if !id.is_empty() {
            return id.clone();
        }
    }
    translate("Service is not running".to_owned())
}

#[cfg(windows)]
#[tokio::main(flavor = "current_thread")]
async fn start_query_session_count(sender: std::sync::mpsc::Sender<Data>) {
    let mut last_count = 0;
    loop {
        if let Ok(mut c) = crate::ipc::connect(1000, "").await {
            let mut timer = crate::rustdesk_interval(tokio::time::interval(Duration::from_secs(1)));
            loop {
                tokio::select! {
                    res = c.next() => {
                        match res {
                            Err(err) => {
                                log::error!("ipc connection closed: {}", err);
                                break;
                            }

                            Ok(Some(Data::ControlledSessionCount(count))) => {
                                if count != last_count {
                                    last_count = count;
                                    sender.send(Data::ControlledSessionCount(count)).ok();
                                }
                            }
                            _ => {}
                        }
                    }

                    _ = timer.tick() => {
                        c.send(&Data::ControlledSessionCount(0)).await.ok();
                    }
                }
            }
        }
        hbb_common::sleep(1.).await;
    }
}

fn load_icon_from_asset() -> Option<image::DynamicImage> {
    let Some(path) = std::env::current_exe().map_or(None, |x| x.parent().map(|x| x.to_path_buf()))
    else {
        return None;
    };
    #[cfg(target_os = "macos")]
    let path = path.join("../Frameworks/App.framework/Resources/flutter_assets/assets/icon.png");
    #[cfg(windows)]
    let path = path.join(r"data\flutter_assets\assets\icon.png");
    #[cfg(target_os = "linux")]
    let path = path.join(r"data/flutter_assets/assets/icon.png");
    if path.exists() {
        if let Ok(image) = image::open(path) {
            return Some(image);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        device_id_dialog_body, linux_device_id_dialog_args, linux_notify_send_args,
        truncate_dialog_lines, DEVICE_ID_DIALOG_OTHER_LINE_MAX_CHARS, DEVICE_ID_DIALOG_WIDTH_PX,
        DEVICE_ID_MAX_CHARS,
    };

    #[test]
    fn linux_id_dialog_keeps_a_fixed_width_without_wrapping() {
        let args = linux_device_id_dialog_args("Show this device's ID", "tc-addr");
        assert_eq!(
            args,
            vec![
                "--info".to_owned(),
                format!("--width={DEVICE_ID_DIALOG_WIDTH_PX}"),
                "--no-wrap".to_owned(),
                "--title".to_owned(),
                "Show this device's ID".to_owned(),
                "--text".to_owned(),
                "tc-addr".to_owned(),
            ]
        );
        assert!(DEVICE_ID_DIALOG_WIDTH_PX >= DEVICE_ID_MAX_CHARS);
        assert_eq!(DEVICE_ID_MAX_CHARS, 253);
    }

    #[test]
    fn device_id_dialog_keeps_the_id_and_truncates_other_lines() {
        let max = DEVICE_ID_DIALOG_OTHER_LINE_MAX_CHARS;
        let id = format!("tc{}", "A".repeat(DEVICE_ID_MAX_CHARS - 2));
        assert_eq!(id.chars().count(), DEVICE_ID_MAX_CHARS);
        let tcp = format!("TCP: {}", "1".repeat(max));
        assert!(tcp.chars().count() > max);

        let body = device_id_dialog_body(&format!("{id}\n{tcp}"));
        let mut lines = body.lines();
        assert_eq!(lines.next().unwrap(), id);
        let tcp_line = lines.next().unwrap();
        assert_eq!(tcp_line.chars().count(), max);
        assert!(tcp_line.ends_with('\u{2026}'));
        assert_eq!(
            tcp_line.chars().take(max - 1).collect::<String>(),
            tcp.chars().take(max - 1).collect::<String>()
        );
        assert!(lines.next().is_none());

        let args = linux_device_id_dialog_args("Show this device's ID", &format!("{id}\n{tcp}"));
        assert_eq!(args.last().map(String::as_str), Some(body.as_str()));
        assert_eq!(
            linux_notify_send_args("Show this device's ID", &format!("{id}\n{tcp}")),
            vec!["Show this device's ID".to_owned(), body]
        );

        // The id line is not cut, even past the other-line limit.
        let long_id = "abc".repeat(max);
        assert_eq!(truncate_dialog_lines(&long_id, max), long_id);

        // `é` and `你` are multiple bytes and one scalar value each.
        // The first line is the id. Only the following lines are cut.
        assert_eq!(truncate_dialog_lines("ééééé\néééé", 4), "ééééé\néééé");
        assert_eq!(truncate_dialog_lines("éééé\nééééé", 4), "éééé\nééé…");
        assert_eq!(truncate_dialog_lines("id\n你你你你你", 4), "id\n你你你…");

        assert_eq!(
            truncate_dialog_lines("ok\n123456789\nend", 8),
            "ok\n1234567…\nend"
        );
    }
}
