//! Native GPUI configurator for the original MonsGeek M1W.
mod app;
mod board;
mod desktop;
mod device;
mod effects;
mod features;
mod log;
mod pages;
mod settings;
mod ui;

use std::borrow::Cow;

use gpui_kit::{
    App, AppContext, AssetSource, Bounds, SharedString, Size, WindowBounds, WindowDecorations,
    WindowOptions,
    component::{Theme, ThemeMode, TitleBar},
    px, size,
};

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Keyboard,
        Lightbulb,
        ListVideo,
        Gauge,
        SlidersHorizontal,
        Archive,
        Usb,
        Unplug,
        Download,
        Upload,
        Trash,
        RotateCcw,
        Move,
        PaintBucket,
        CircleDot,
        Square,
        Pipette,
        ScrollText,
        Battery,
        Sparkles,
        Sun,
        ArrowLeftRight,
        Image,
        Rainbow,
        Palette,
        Zap,
        Command,
        Timer,
        Bluetooth,
        Radio,
        Layers,
        Lock,
        KeyboardOff,
        Leaf,
        Monitor,
        ShieldCheck,
        BatteryFull,
        BatteryMedium,
        BatteryLow,
        BatteryWarning,
        BellRing,
        PanelTop,
        LogIn,
        Save,
        CircleStop,
        Play,
        Pause,
    ]
);

struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(data) => Ok(Some(data)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut all = ExtraIcons.list(path)?;
        all.extend(gpui_kit::assets::Assets.list(path)?);
        all.sort();
        all.dedup();
        Ok(all)
    }
}

/// Bundle Vercel's Geist (SIL OFL 1.1, see assets/fonts/OFL.txt) so the
/// interface renders the same on systems without it installed.
fn use_geist(cx: &mut App) {
    let fonts = [
        &include_bytes!("../assets/fonts/Geist-Regular.ttf")[..],
        include_bytes!("../assets/fonts/Geist-Medium.ttf"),
        include_bytes!("../assets/fonts/Geist-SemiBold.ttf"),
        include_bytes!("../assets/fonts/Geist-Bold.ttf"),
        include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
        include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    ];
    if let Err(e) = cx
        .text_system()
        .add_fonts(fonts.into_iter().map(Cow::Borrowed).collect())
    {
        log::append(&log::Entry::error("Cannot load Geist", &e));
        return;
    }
    // Set the family on both registered themes so theme switches keep it.
    let theme = Theme::global_mut(cx);
    for config in [&mut theme.light_theme, &mut theme.dark_theme] {
        let mut c = (**config).clone();
        c.font_family = Some("Geist".into());
        c.mono_font_family = Some("Geist Mono".into());
        *config = std::rc::Rc::new(c);
    }
    theme.font_family = "Geist".into();
    theme.mono_font_family = "Geist Mono".into();
}

fn main() {
    log::start_session();
    log::install_panic_hook();
    gpui_kit::application()
        .with_assets(AppAssets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            use_geist(cx);
            let bounds = Bounds::centered(None, size(px(1240.), px(820.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(Size {
                    width: px(960.),
                    height: px(640.),
                }),
                app_id: Some("monsgeek-linux".into()),
                // Compositors that draw no decorations (niri, Sway) leave the
                // window chrome-free; others fall back to the drawn title bar.
                window_decorations: Some(WindowDecorations::Server),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                match settings::load().theme {
                    Some(choice) => Theme::change(
                        match choice {
                            settings::ThemeChoice::Dark => ThemeMode::Dark,
                            settings::ThemeChoice::Light => ThemeMode::Light,
                        },
                        Some(window),
                        cx,
                    ),
                    None => Theme::sync_system_appearance(Some(window), cx),
                }
                cx.new(|cx| app::Workbench::new(window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
