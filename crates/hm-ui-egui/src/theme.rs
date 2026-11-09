//! Tema propio «plano técnico»: única fuente de colores, tipografía y espaciado.
//! Acero (neutros), azul petróleo (acción y selección) y cobre (acento, valores
//! editados a mano y elemento seleccionado).

use eframe::egui::{self, Color32};

/// Colores semánticos de la interfaz y de la máquina; iguales en todas las vistas.
pub(crate) struct Palette {
    pub background: Color32,
    pub paper: Color32,
    pub grid: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub copper: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub error: Color32,
    /// Tapa trasera.
    pub hub: Color32,
    /// Tapa delantera.
    pub shroud: Color32,
    pub leading_edge: Color32,
    pub trailing_edge: Color32,
    pub cut: Color32,
    pub blade: Color32,
    pub volute: Color32,
    pub tongue: Color32,
    pub reference: Color32,
    pub construction: Color32,
    pub dimension: Color32,
    pub dimension_text: Color32,
    pub marker: Color32,
    pub preferred_region: Color32,
    pub head: Color32,
    pub efficiency: Color32,
    pub power: Color32,
    pub npsh_r: Color32,
    pub npsh_a: Color32,
    pub recirculation: Color32,
    pub motor_limit: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub(crate) const LIGHT: Palette = Palette {
    background: rgb(244, 245, 246),
    paper: rgb(255, 255, 255),
    grid: rgb(228, 231, 234),
    text: rgb(30, 35, 40),
    text_weak: rgb(92, 100, 108),
    accent: rgb(15, 94, 107),
    accent_soft: rgb(200, 226, 230),
    copper: rgb(166, 90, 36),
    ok: rgb(30, 120, 70),
    warn: rgb(145, 96, 0),
    error: rgb(176, 42, 32),
    hub: rgb(37, 82, 150),
    shroud: rgb(46, 120, 50),
    leading_edge: rgb(30, 35, 40),
    trailing_edge: rgb(92, 100, 108),
    cut: rgb(190, 85, 10),
    blade: rgb(15, 120, 135),
    volute: rgb(150, 95, 20),
    tongue: rgb(92, 100, 108),
    reference: rgb(140, 147, 153),
    construction: rgb(170, 176, 181),
    dimension: rgb(105, 112, 118),
    dimension_text: rgb(45, 50, 55),
    marker: rgb(176, 42, 32),
    preferred_region: Color32::from_rgba_premultiplied(3, 13, 8, 28),
    head: rgb(37, 82, 150),
    efficiency: rgb(46, 120, 50),
    power: rgb(150, 95, 20),
    npsh_r: rgb(190, 85, 10),
    npsh_a: rgb(30, 120, 70),
    recirculation: rgb(120, 70, 160),
    motor_limit: rgb(176, 42, 32),
};

pub(crate) const DARK: Palette = Palette {
    background: rgb(27, 31, 35),
    paper: rgb(21, 25, 29),
    grid: rgb(40, 46, 52),
    text: rgb(230, 233, 236),
    text_weak: rgb(160, 168, 175),
    accent: rgb(70, 175, 190),
    accent_soft: rgb(14, 56, 64),
    copper: rgb(222, 145, 90),
    ok: rgb(95, 195, 130),
    warn: rgb(230, 180, 70),
    error: rgb(235, 110, 95),
    hub: rgb(110, 160, 220),
    shroud: rgb(120, 195, 120),
    leading_edge: rgb(230, 233, 236),
    trailing_edge: rgb(160, 168, 175),
    cut: rgb(235, 140, 60),
    blade: rgb(80, 190, 205),
    volute: rgb(220, 175, 90),
    tongue: rgb(160, 168, 175),
    reference: rgb(120, 128, 135),
    construction: rgb(95, 102, 110),
    dimension: rgb(150, 158, 165),
    dimension_text: rgb(215, 220, 224),
    marker: rgb(235, 110, 95),
    preferred_region: Color32::from_rgba_premultiplied(10, 21, 14, 28),
    head: rgb(110, 160, 220),
    efficiency: rgb(120, 195, 120),
    power: rgb(220, 175, 90),
    npsh_r: rgb(235, 140, 60),
    npsh_a: rgb(95, 195, 130),
    recirculation: rgb(185, 140, 225),
    motor_limit: rgb(235, 110, 95),
};

/// Paleta del tema activo de egui.
pub(crate) fn palette(ctx: &egui::Context) -> &'static Palette {
    if ctx.theme() == egui::Theme::Dark {
        &DARK
    } else {
        &LIGHT
    }
}

/// `Visuals` de un tema construidos con su paleta.
pub(crate) fn visuals(theme: egui::Theme) -> egui::Visuals {
    let (mut v, p) = match theme {
        egui::Theme::Dark => (egui::Visuals::dark(), &DARK),
        egui::Theme::Light => (egui::Visuals::light(), &LIGHT),
    };
    v.panel_fill = p.background;
    v.window_fill = p.paper;
    v.extreme_bg_color = p.paper;
    v.faint_bg_color = p.grid;
    v.widgets.noninteractive.fg_stroke.color = p.text;
    v.weak_text_color = Some(p.text_weak);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = egui::Stroke::new(1.0, p.accent);
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.error;
    let corner = egui::CornerRadius::same(3);
    v.widgets.noninteractive.corner_radius = corner;
    v.widgets.inactive.corner_radius = corner;
    v.widgets.hovered.corner_radius = corner;
    v.widgets.active.corner_radius = corner;
    v.widgets.open.corner_radius = corner;
    v
}

fn style(theme: egui::Theme) -> egui::Style {
    use egui::{FontFamily, FontId, TextStyle};
    let mut style = egui::Style {
        visuals: visuals(theme),
        ..egui::Style::default()
    };
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(18.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(12.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(12.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        ),
        (
            TextStyle::Small,
            FontId::new(10.0, FontFamily::Proportional),
        ),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 5.0);
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
    style
}

/// Registra los dos temas y deja que el sistema elija el activo.
pub(crate) fn install(ctx: &egui::Context) {
    ctx.set_style_of(egui::Theme::Light, style(egui::Theme::Light));
    ctx.set_style_of(egui::Theme::Dark, style(egui::Theme::Dark));
    ctx.set_theme(egui::ThemePreference::System);
}

/// Tema pedido por `HMF_THEME` (solo para capturas). Un valor desconocido
/// avisa por `stderr` y cae en claro.
pub(crate) fn theme_from_env(value: Option<&str>) -> Option<egui::ThemePreference> {
    match value? {
        "light" => Some(egui::ThemePreference::Light),
        "dark" => Some(egui::ThemePreference::Dark),
        other => {
            eprintln!("HMF_THEME: no entiendo `{other}`; uso claro");
            Some(egui::ThemePreference::Light)
        }
    }
}

#[cfg(test)]
fn luminance(c: Color32) -> f32 {
    let channel = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// Razón de contraste WCAG 2.1 entre dos colores opacos.
#[cfg(test)]
pub(crate) fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: [&Palette; 2] = [&LIGHT, &DARK];

    #[test]
    fn contrast_ratio_matches_wcag() {
        let black_on_white = contrast_ratio(Color32::BLACK, Color32::WHITE);
        assert!((black_on_white - 21.0).abs() < 0.01, "{black_on_white}");
        let c = Color32::from_rgb(15, 94, 107);
        assert_eq!(contrast_ratio(c, c), 1.0);
        let d = Color32::from_rgb(244, 245, 246);
        assert_eq!(contrast_ratio(c, d), contrast_ratio(d, c));
    }

    #[test]
    fn text_and_states_reach_aa() {
        for p in BOTH {
            for (name, c) in [
                ("text", p.text),
                ("text_weak", p.text_weak),
                ("accent", p.accent),
                ("copper", p.copper),
                ("ok", p.ok),
                ("warn", p.warn),
                ("error", p.error),
                ("dimension_text", p.dimension_text),
            ] {
                for bg in [p.paper, p.background] {
                    let r = contrast_ratio(c, bg);
                    assert!(r >= 4.5, "{name} sobre {bg:?}: {r}");
                }
            }
        }
    }

    #[test]
    fn strokes_reach_three_to_one() {
        for p in BOTH {
            for (name, c) in [
                ("hub", p.hub),
                ("shroud", p.shroud),
                ("leading_edge", p.leading_edge),
                ("trailing_edge", p.trailing_edge),
                ("cut", p.cut),
                ("blade", p.blade),
                ("volute", p.volute),
                ("tongue", p.tongue),
                ("dimension", p.dimension),
                ("marker", p.marker),
                ("head", p.head),
                ("efficiency", p.efficiency),
                ("power", p.power),
                ("npsh_r", p.npsh_r),
                ("npsh_a", p.npsh_a),
                ("recirculation", p.recirculation),
                ("motor_limit", p.motor_limit),
            ] {
                let r = contrast_ratio(c, p.paper);
                assert!(r >= 3.0, "{name}: {r}");
            }
        }
    }

    #[test]
    fn soft_strokes_stay_visible() {
        for p in BOTH {
            for (name, c) in [("reference", p.reference), ("construction", p.construction)] {
                let r = contrast_ratio(c, p.paper);
                assert!(r >= 1.8, "{name}: {r}");
            }
        }
    }

    #[test]
    fn hub_and_shroud_differ() {
        for p in BOTH {
            assert_ne!(p.hub, p.shroud);
            assert_ne!(p.blade, p.volute);
            assert_ne!(p.copper, p.accent);
        }
    }

    #[test]
    fn visuals_use_palette() {
        for (theme, p, dark) in [
            (egui::Theme::Light, &LIGHT, false),
            (egui::Theme::Dark, &DARK, true),
        ] {
            let v = visuals(theme);
            assert_eq!(v.dark_mode, dark);
            assert_eq!(v.panel_fill, p.background);
            assert_eq!(v.extreme_bg_color, p.paper);
            assert_eq!(v.selection.bg_fill, p.accent_soft);
        }
    }

    #[test]
    fn palette_follows_context_theme() {
        let ctx = egui::Context::default();
        install(&ctx);
        ctx.set_theme(egui::Theme::Dark);
        assert_eq!(palette(&ctx).paper, DARK.paper);
        ctx.set_theme(egui::Theme::Light);
        assert_eq!(palette(&ctx).paper, LIGHT.paper);
    }

    #[test]
    fn env_theme_parses() {
        assert_eq!(
            theme_from_env(Some("dark")),
            Some(egui::ThemePreference::Dark)
        );
        assert_eq!(
            theme_from_env(Some("light")),
            Some(egui::ThemePreference::Light)
        );
        assert_eq!(theme_from_env(None), None);
    }

    #[test]
    fn unknown_env_theme_falls_back_to_light() {
        assert_eq!(
            theme_from_env(Some("sepia")),
            Some(egui::ThemePreference::Light)
        );
    }

    #[test]
    fn no_loose_colours_outside_theme() {
        const FORBIDDEN: [&str; 10] = [
            "Color32::from_",
            "Color32::RED",
            "Color32::GREEN",
            "Color32::BLUE",
            "Color32::GRAY",
            "Color32::WHITE",
            "Color32::BLACK",
            "Color32::YELLOW",
            "Color32::LIGHT_",
            "Color32::DARK_",
        ];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !name.ends_with(".rs") || name == "theme.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (index, line) in text.lines().enumerate() {
                if let Some(found) = FORBIDDEN.iter().find(|f| line.contains(**f)) {
                    panic!(
                        "{name}:{}: color suelto `{found}`; usa un token de theme.rs",
                        index + 1
                    );
                }
            }
        }
    }

    #[test]
    fn selected_widget_text_reaches_aa() {
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let v = visuals(theme);
            let r = contrast_ratio(v.selection.stroke.color, v.selection.bg_fill);
            assert!(r >= 4.5, "{theme:?}: {r}");
        }
    }
}
