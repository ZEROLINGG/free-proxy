// client_tauri/src-tauri/src/tray.rs
// 桌面端托盘：常驻系统托盘 + 关闭到托盘。
//   - 菜单：显示主界面 / 启动·停止代理（按运行状态动态切换 label）/ 退出；
//   - 主窗口点 X 只隐藏不退出（进程与本地代理继续运行），退出仅通过托盘菜单；
//   - 代理启停复用 commands::proxy（含失败恢复旧实例等既有逻辑），配置来源与 UI 一致；
//   - 菜单 label 与图标（未运行黑底白描边 / 运行中白底黑描边）通过监听
//     proxy:status 事件实时同步（UI 侧启停同样生效）。

use std::sync::LazyLock;

use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Listener, Manager, WindowEvent, Wry,
};

const TRAY_ID: &str = "main-tray";
const MENU_SHOW: &str = "show";
const MENU_TOGGLE: &str = "toggle";
const MENU_QUIT: &str = "quit";

/// 托盘渲染尺寸（各平台实际显示尺寸约为 16~32px，渲染大图由系统缩放保证 HiDPI 清晰）
const TRAY_ICON_SIZE: u32 = 64;

/// 托盘双态图标（[未运行, 运行中]），进程生命周期内渲染一次。
/// 未运行：黑底白描边；运行中：白底黑描边（见 icons/free-proxy[-on].svg）。
static TRAY_ICONS: LazyLock<[Image<'static>; 2]> = LazyLock::new(|| {
    [
        render_svg(include_str!("../icons/free-proxy.svg")),
        render_svg(include_str!("../icons/free-proxy-on.svg")),
    ]
});

/// 将内嵌 SVG 光栅化为 RGBA 托盘图标（Tauri Image::from_bytes 仅支持 png/ico）
fn render_svg(svg: &str) -> Image<'static> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg, &opt).expect("invalid tray icon svg");
    let size = tree.size();
    let scale = TRAY_ICON_SIZE as f32 / size.width().max(1.0);
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(TRAY_ICON_SIZE, TRAY_ICON_SIZE).expect("alloc pixmap");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Image::new_owned(pixmap.take(), TRAY_ICON_SIZE, TRAY_ICON_SIZE)
}

/// 初始化托盘：构建菜单与图标、注册事件、拦截主窗口关闭、同步代理状态。
pub fn init(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let menu = build_menu(app)?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("free-proxy")
        .menu(&menu)
        // 左键不再弹菜单：Win/macOS 左键直接唤起主窗口（on_tray_icon_event），
        // 右键走菜单；Linux 后端忽略此设置，左键仍显示菜单。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_SHOW => show_main_window(app),
            MENU_TOGGLE => handle_toggle(app),
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Linux 不支持该事件；Win/macOS 左键单击唤起主窗口
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    // 图标与菜单初始状态按当前代理状态同步（兜底，正常情况下启动即未运行）
    refresh_icon(app);
    refresh_menu(app)?;

    // 关闭到托盘：点 X 隐藏窗口，进程与代理保持运行
    if let Some(window) = app.get_webview_window("main") {
        let win = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = win.hide();
            }
        });
    }

    // 代理状态变化（UI 或托盘触发）→ 重建菜单、同步图标
    let listener_app = app.clone();
    app.listen("proxy:status", move |_| {
        let _ = refresh_menu(&listener_app);
        refresh_icon(&listener_app);
    });

    Ok(())
}

/// 按当前代理运行状态切换托盘图标（[未运行, 运行中] 双态）
fn refresh_icon(app: &AppHandle<Wry>) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let icon = TRAY_ICONS[proxy_running() as usize].clone();
        let _ = tray.set_icon(Some(icon));
    }
}

/// 构建托盘菜单：label 按当前代理运行状态动态取"停止代理"/"启动代理"。
fn build_menu(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    let show_i = MenuItem::with_id(app, MENU_SHOW, "显示主界面", true, None::<&str>)?;
    let toggle_label = if proxy_running() {
        "停止代理"
    } else {
        "启动代理"
    };
    let toggle_i = MenuItem::with_id(app, MENU_TOGGLE, toggle_label, true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, MENU_QUIT, "退出", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&show_i, &toggle_i, &sep, &quit_i])
}

/// 重建托盘菜单（Tauri 菜单文本不可原地修改，需整体重建）。
fn refresh_menu(app: &AppHandle<Wry>) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_menu(app)?))?;
    }
    Ok(())
}

/// 当前代理是否运行中
fn proxy_running() -> bool {
    let guard = crate::commands::proxy::PROXY
        .read()
        .unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .map(|s| s.proxy.is_running())
        .unwrap_or(false)
}

/// 托盘"启动/停止代理"：运行中则停止；未运行则用已保存配置启动，
/// 配置缺失/失败时唤出主窗口让用户先配置。
fn handle_toggle(app: &AppHandle<Wry>) {
    if proxy_running() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = crate::commands::proxy::proxy_stop(app).await;
        });
    } else {
        match crate::commands::settings::load_settings(app.clone()) {
            Ok(settings) => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::commands::proxy::proxy_start(app.clone(), settings).await
                    {
                        eprintln!("tray: proxy start failed: {e}");
                        show_main_window(&app);
                    }
                });
            }
            Err(e) => {
                eprintln!("tray: failed to load settings for proxy start: {e}");
                show_main_window(app);
            }
        }
    }
    let _ = refresh_menu(app);
    refresh_icon(app);
}

/// 唤起主窗口（隐藏/最小化时恢复并聚焦）
fn show_main_window(app: &AppHandle<Wry>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}
