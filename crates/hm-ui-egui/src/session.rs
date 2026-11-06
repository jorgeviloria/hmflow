//! Sesión de la ventana: archivo abierto, cajas y qué avisos mostrar.
//!
//! Sin egui. No calcula: abrir es `open_file`, y el primer cambio de una caja
//! devuelve el panel al resultado de `compute`. Nuevo y Abrir, con cambios,
//! piden repetir la orden antes de descartar.

use std::path::{Path, PathBuf};

use hm_core::Diagnostic;
use hm_project::Project;

use crate::model::{Basis, Inputs, build_project, export_report, open_file, open_loaded};

/// Orden del menú Archivo que puede tirar el trabajo en curso.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    New,
    Open,
    RemoveCasing,
}

/// Si la orden actúa ya, o si quedó armada y hay que repetirla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// No hay cambios, o es la misma orden repetida.
    Proceed,
    /// Hay cambios y esta es la primera vez: no actúa.
    Armed,
}

/// Aviso cuando Nuevo o Abrir no actúan porque hay cambios.
pub const DISCARD_NOTICE: &str = "Hay cambios sin guardar. Repite la orden para descartarlos.";

/// Quitar la carcasa de un archivo no puede ocurrir por un clic suelto.
pub const CASING_NOTICE: &str =
    "Esto quita la carcasa del archivo al guardar. Repite para confirmarlo.";

/// Lo que la ventana recuerda entre un fotograma y el siguiente.
#[derive(Default)]
pub struct Session {
    pub inputs: Inputs,
    /// Selecciones y densidad del archivo abierto, para `compute`.
    pub basis: Basis,
    pub file: Option<PathBuf>,
    pub project: Option<Project>,
    pub dirty: bool,
    pub notice: Option<String>,
    /// Avisos de `open_project`. Sustituyen al panel de `compute` hasta el
    /// primer cambio de una caja.
    pub opened_diagnostics: Option<Vec<Diagnostic>>,
    /// Orden armada para descartar. La misma, repetida, sí actúa.
    armed: Option<Command>,
    /// El último título enviado a la ventana.
    shown_title: Option<String>,
}

impl Session {
    /// Nombre del archivo para el título, sin la carpeta.
    pub fn file_name(&self) -> Option<String> {
        self.file.as_ref().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
    }

    /// Título de la ventana.
    pub fn title(&self) -> String {
        match self.file_name() {
            Some(name) => format!("HidroMac Flow — {name}"),
            None => "HidroMac Flow".to_string(),
        }
    }

    /// Nombre en la barra: el archivo, o «sin título» si hay cambios y no hay
    /// archivo. Con cambios, lleva `●` al final.
    pub fn bar_label(&self) -> Option<String> {
        let name = self
            .file_name()
            .or_else(|| self.dirty.then(|| "sin título".to_string()))?;
        if self.dirty {
            Some(format!("{name} ●"))
        } else {
            Some(name)
        }
    }

    /// El aviso de descarte, si es el que está puesto.
    pub fn discard_notice(&self) -> Option<&str> {
        self.notice
            .as_deref()
            .filter(|notice| *notice == DISCARD_NOTICE)
    }

    /// El aviso del panel de diagnósticos: todos menos el de descarte, que va
    /// en la barra.
    pub fn panel_notice(&self) -> Option<&str> {
        self.notice
            .as_deref()
            .filter(|notice| *notice != DISCARD_NOTICE)
    }

    /// El título, solo si no es el que ya tiene la ventana.
    ///
    /// Cada orden a la ventana pide otro fotograma: enviarlo siempre la
    /// dejaría repintándose sin parar.
    pub fn title_change(&mut self) -> Option<String> {
        let title = self.title();
        if self.shown_title.as_ref() == Some(&title) {
            return None;
        }
        self.shown_title = Some(title.clone());
        Some(title)
    }

    /// Abre `path`.
    ///
    /// Si sale bien, las cajas, el archivo y el proyecto quedan en lo abierto,
    /// sin cambios pendientes, y el panel muestra esos diagnósticos. Si falla,
    /// solo queda `notice`: lo anterior sigue igual.
    pub fn open_path(&mut self, path: &Path) -> bool {
        match open_file(path) {
            Ok(opened) => {
                self.inputs = opened.inputs;
                self.basis = opened.basis;
                self.file = Some(path.to_path_buf());
                self.project = Some(opened.project);
                self.dirty = false;
                self.notice = None;
                self.armed = None;
                self.opened_diagnostics = Some(opened.diagnostics);
                true
            }
            Err(message) => {
                self.notice = Some(message);
                false
            }
        }
    }

    /// Una caja cambió: hay trabajo sin guardar, se desarma el descarte y el
    /// panel vuelve a los avisos del cálculo.
    pub fn edited(&mut self) {
        self.opened_diagnostics = None;
        self.dirty = true;
        self.armed = None;
        if self.discard_notice().is_some() || self.notice.as_deref() == Some(CASING_NOTICE) {
            self.notice = None;
        }
    }

    /// Guardar desarma el descarte: no tira el trabajo, lo escribe.
    pub fn disarm(&mut self) {
        self.armed = None;
        if self.discard_notice().is_some() {
            self.notice = None;
        }
    }

    /// Pide quitar la carcasa. Si el archivo la tiene, la primera vez solo avisa;
    /// repetida, actúa. Con una que aún no está en el archivo no hay nada que perder.
    pub fn request_remove_casing(&mut self) -> Gate {
        let saved = self
            .project
            .as_ref()
            .is_some_and(|project| project.casing.is_some());
        if !saved || self.armed == Some(Command::RemoveCasing) {
            self.armed = None;
            if self.notice.as_deref() == Some(CASING_NOTICE) {
                self.notice = None;
            }
            return Gate::Proceed;
        }
        self.armed = Some(Command::RemoveCasing);
        self.notice = Some(CASING_NOTICE.to_string());
        Gate::Armed
    }

    /// Pide Nuevo o Abrir.
    ///
    /// Sin cambios, actúa. Con cambios, la primera vez deja el aviso y no
    /// actúa; la misma orden, repetida, sí. Otra orden, o una edición, desarma
    /// la anterior.
    pub fn request(&mut self, command: Command) -> Gate {
        if !self.dirty {
            self.armed = None;
            return Gate::Proceed;
        }
        if self.armed == Some(command) {
            self.armed = None;
            return Gate::Proceed;
        }
        self.armed = Some(command);
        self.notice = Some(DISCARD_NOTICE.to_string());
        Gate::Armed
    }

    /// Escribe el proyecto en `path`.
    ///
    /// Si falta, añade `.hmf`. El nombre del proyecto nuevo es el del archivo,
    /// sin extensión. Solo si sale bien quedan `project`, `file` y las cajas
    /// de lo escrito, sin `●`. Si falla, `notice`: el disco y el `●` siguen.
    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        self.disarm();
        let chosen = path;
        let path = with_extension(chosen, "hmf");
        if let Some(message) = unasked_collision(chosen, &path) {
            self.notice = Some(message.clone());
            return Err(message);
        }
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "sin título".to_string());
        let had_project = self.project.is_some();
        let built = match build_project(&self.inputs, &self.basis, self.project.as_ref(), &name) {
            Ok(built) => built,
            Err(message) => {
                self.notice = Some(message.clone());
                return Err(message);
            }
        };
        if let Err(error) = built.project.save(&path) {
            let message = error.to_string();
            self.notice = Some(message.clone());
            return Err(message);
        }
        let reopened = match open_loaded(built.project.clone()) {
            Ok(reopened) => reopened,
            Err(message) => {
                // El archivo ya está escrito: la sesión apunta a él.
                self.notice = Some(message.clone());
                self.file = Some(path);
                self.project = Some(built.project);
                self.dirty = false;
                self.opened_diagnostics = had_project.then_some(built.diagnostics);
                return Err(message);
            }
        };
        self.inputs = reopened.inputs;
        self.basis = reopened.basis;
        self.project = Some(built.project);
        self.file = Some(path);
        self.dirty = false;
        self.notice = None;
        self.opened_diagnostics = had_project.then_some(built.diagnostics);
        Ok(())
    }

    /// El nombre del proyecto: el del archivo, sin extensión, o «sin título».
    pub fn project_name(&self) -> String {
        self.file
            .as_ref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "sin título".to_string())
    }

    /// El JSON del informe, antes de abrir el diálogo. Si no se puede armar,
    /// queda en `notice`. No toca el `.hmf`, el proyecto ni el `●`, pero es otra
    /// orden: desarma el descarte, aunque luego se cancele el diálogo.
    pub fn export_json(&mut self) -> Result<String, String> {
        self.disarm();
        let name = self.project_name();
        let exported = export_report(&self.inputs, &self.basis, self.project.as_ref(), &name);
        if let Err(message) = &exported {
            self.notice = Some(message.clone());
        }
        exported
    }

    /// Escribe el informe en `path`. Si falta, añade `.json`. Si falla, `notice`.
    pub fn export_to(&mut self, path: &Path, json: &str) -> Result<(), String> {
        let chosen = path;
        let path = with_extension(chosen, "json");
        if let Some(message) = unasked_collision(chosen, &path) {
            self.notice = Some(message.clone());
            return Err(message);
        }
        if let Err(error) = std::fs::write(&path, json) {
            let message = error.to_string();
            self.notice = Some(message.clone());
            return Err(message);
        }
        self.notice = None;
        Ok(())
    }

    /// Nuevo: las entradas por defecto, sin archivo, sin proyecto y sin `●`.
    pub fn new_design(&mut self) {
        let shown_title = self.shown_title.take();
        *self = Self {
            shown_title,
            ..Self::default()
        };
    }
}

/// `extension` si el nombre no la trae. No sustituye otra extensión: la añade.
fn with_extension(path: &Path, extension: &str) -> PathBuf {
    let has_it = path
        .extension()
        .and_then(|found| found.to_str())
        .is_some_and(|found| found.eq_ignore_ascii_case(extension));
    if has_it {
        return path.to_path_buf();
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}

/// El diálogo preguntó por el nombre elegido, no por el que sale de añadir la
/// extensión: ese archivo no se pisa.
fn unasked_collision(chosen: &Path, path: &Path) -> Option<String> {
    (path != chosen && path.exists()).then(|| {
        format!(
            "ya existe {}: elige otro nombre",
            path.file_name().unwrap_or_default().to_string_lossy()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../hm-project/tests/fixtures")
            .join(name)
    }

    #[test]
    fn opening_a_fixture_replaces_the_session() {
        let mut session = Session::default();
        session.inputs.q = "1 m3/h".into();
        session.dirty = true;
        let path = fixture("end_suction_v0.9.hmf");

        assert!(session.open_path(&path));

        assert_eq!(session.inputs.q, "120 m3/h");
        assert_eq!(session.file.as_deref(), Some(path.as_path()));
        assert!(session.project.is_some());
        assert!(!session.dirty);
        assert!(session.notice.is_none());
        assert!(session.opened_diagnostics.is_some());
        assert_eq!(session.title(), "HidroMac Flow — end_suction_v0.9.hmf");
    }

    #[test]
    fn the_title_is_sent_only_when_it_changes() {
        let mut session = Session::default();
        assert_eq!(session.title_change().as_deref(), Some("HidroMac Flow"));
        assert_eq!(session.title_change(), None);

        assert!(session.open_path(&fixture("end_suction_v0.9.hmf")));
        assert_eq!(
            session.title_change().as_deref(),
            Some("HidroMac Flow — end_suction_v0.9.hmf")
        );
        assert_eq!(session.title_change(), None);
    }

    #[test]
    fn a_failed_open_keeps_the_previous_boxes() {
        let mut session = Session::default();
        session.inputs.q = "100 m3/h".into();
        let path = fixture("end_suction_v0.9.hmf");
        assert!(session.open_path(&path));
        let previous = session.inputs.q.clone();

        let mut project = session.project.clone().unwrap();
        project.operating.fluid = "aceite".into();
        let rejected = std::env::temp_dir().join("hmflow-sesion-aceite.hmf");
        project.save(&rejected).unwrap();
        let opened = session.open_path(&rejected);
        let _ = std::fs::remove_file(&rejected);

        assert!(!opened);
        assert_eq!(session.inputs.q, previous);
        assert_eq!(session.file.as_deref(), Some(path.as_path()));
        assert_eq!(
            session.notice.as_deref(),
            Some("esta interfaz solo edita proyectos de agua")
        );
        assert!(session.opened_diagnostics.is_some());
    }

    #[test]
    fn an_edit_returns_the_panel_to_the_computation() {
        let mut session = Session::default();
        assert!(session.open_path(&fixture("end_suction_v0.8.hmf")));
        assert_eq!(
            session.opened_diagnostics.as_ref().unwrap()[0].code,
            "HM-PROJ-025"
        );

        session.edited();

        assert!(session.opened_diagnostics.is_none());
        assert!(session.project.is_some());
    }

    #[test]
    fn a_clean_session_proceeds_at_once() {
        let mut session = Session::default();

        assert_eq!(session.request(Command::New), Gate::Proceed);
        assert_eq!(session.request(Command::Open), Gate::Proceed);
        assert_eq!(session.request(Command::New), Gate::Proceed);

        assert!(!session.dirty);
        assert!(session.notice.is_none());
        assert!(session.bar_label().is_none());
    }

    #[test]
    fn a_dirty_session_arms_the_first_time() {
        let mut session = Session::default();
        let inputs = session.inputs.clone();
        session.edited();

        assert_eq!(session.request(Command::New), Gate::Armed);

        assert!(session.dirty);
        assert_eq!(session.notice.as_deref(), Some(DISCARD_NOTICE));
        assert_eq!(session.inputs, inputs);
        assert!(session.file.is_none());
        assert!(session.project.is_none());
        assert_eq!(session.bar_label().as_deref(), Some("sin título ●"));
    }

    #[test]
    fn the_discard_notice_goes_to_the_bar_and_a_rejection_to_the_panel() {
        let mut session = Session::default();
        session.edited();
        assert_eq!(session.request(Command::New), Gate::Armed);
        assert_eq!(session.discard_notice(), Some(DISCARD_NOTICE));
        assert_eq!(session.panel_notice(), None);

        session.notice = Some("esta interfaz solo edita proyectos de agua".into());
        assert_eq!(session.discard_notice(), None);
        assert_eq!(
            session.panel_notice(),
            Some("esta interfaz solo edita proyectos de agua")
        );
    }

    #[test]
    fn the_same_command_twice_proceeds() {
        let mut session = Session::default();
        session.edited();

        assert_eq!(session.request(Command::Open), Gate::Armed);
        assert_eq!(session.request(Command::Open), Gate::Proceed);
        // Sigue habiendo cambios: la siguiente vez vuelve a pedir confirmación.
        assert_eq!(session.request(Command::Open), Gate::Armed);
    }

    #[test]
    fn a_different_command_between_disarms() {
        let mut session = Session::default();
        session.edited();

        assert_eq!(session.request(Command::New), Gate::Armed);
        assert_eq!(session.request(Command::Open), Gate::Armed);
        assert_eq!(session.request(Command::New), Gate::Armed);
        assert_eq!(session.request(Command::New), Gate::Proceed);
    }

    #[test]
    fn an_edit_between_disarms() {
        let mut session = Session::default();
        session.edited();
        assert_eq!(session.request(Command::New), Gate::Armed);

        session.edited();

        assert!(session.notice.is_none());
        assert_eq!(session.request(Command::New), Gate::Armed);
        assert_eq!(session.notice.as_deref(), Some(DISCARD_NOTICE));
    }

    #[test]
    fn new_resets_to_the_defaults() {
        let mut session = Session::default();
        let path = fixture("end_suction_v0.9.hmf");
        assert!(session.open_path(&path));
        assert!(session.basis.design.psi.is_some());
        assert!(session.basis.density.is_some());
        assert_eq!(session.bar_label().as_deref(), Some("end_suction_v0.9.hmf"));
        session.inputs.q = "1 m3/h".into();
        session.edited();
        assert_eq!(
            session.bar_label().as_deref(),
            Some("end_suction_v0.9.hmf ●")
        );
        assert_eq!(session.request(Command::Open), Gate::Armed);
        assert_eq!(
            session.title_change().as_deref(),
            Some("HidroMac Flow — end_suction_v0.9.hmf")
        );

        session.new_design();

        assert_eq!(session.inputs, Inputs::default());
        assert!(session.file.is_none());
        assert!(session.project.is_none());
        assert!(session.basis.density.is_none());
        assert!(session.basis.design.psi.is_none());
        assert!(!session.dirty);
        assert!(session.notice.is_none());
        assert!(session.opened_diagnostics.is_none());
        assert!(session.bar_label().is_none());
        assert_eq!(session.title(), "HidroMac Flow");
        assert_eq!(session.title_change().as_deref(), Some("HidroMac Flow"));
        assert_eq!(session.request(Command::New), Gate::Proceed);
    }

    use hm_project::{Param, Project};

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("hmflow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn copy_fixture(dir: &std::path::Path, name: &str) -> PathBuf {
        let dest = dir.join(name);
        std::fs::copy(fixture(name), &dest).unwrap();
        dest
    }

    fn sample_boxes() -> crate::model::CasingBoxes {
        let mut boxes = crate::model::CasingBoxes::blank();
        for (name, text) in [
            ("d2", "210 mm"),
            ("neck_diameter", "90 mm"),
            ("eye_diameter_max", "75 mm"),
            ("bore_diameter", "20 mm"),
            ("hub_diameter", "30 mm"),
            ("b3", "30 mm"),
            ("cutwater_radius", "111 mm"),
        ] {
            *boxes.text_mut(name).unwrap() = text.to_string();
        }
        boxes
    }

    #[test]
    fn a_new_design_with_a_casing_is_saved_and_reopened_with_its_boxes() {
        let dir = scratch("carcasa-nueva");
        let path = dir.join("rodete.hmf");
        let mut session = Session::default();
        session.inputs.casing = Some(sample_boxes());
        session.edited();
        session.save_to(&path).unwrap();

        let saved = Project::load(&path).unwrap();
        assert_eq!(saved.casing.as_ref().unwrap().b3, "30 mm");
        let mut again = Session::default();
        assert!(again.open_path(&path), "{:?}", again.notice);
        assert_eq!(again.inputs.casing, session.inputs.casing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_an_edited_box_writes_it_and_reselects_what_depends_on_it() {
        let dir = scratch("carcasa-editada");
        let path = copy_fixture(&dir, "casing_v3.hmf");
        let mut session = Session::default();
        assert!(session.open_path(&path), "{:?}", session.notice);
        let boxes = session.inputs.casing.as_mut().unwrap();
        *boxes.text_mut("b3").unwrap() = "28 mm".to_string();
        session.edited();
        session.save_to(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap().casing.unwrap().b3, "28 mm");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_without_touching_the_casing_leaves_the_file_as_it_was() {
        let dir = scratch("carcasa-igual");
        let path = copy_fixture(&dir, "casing_v3.hmf");
        let recalculated = hm_engine::recalc(Project::load(&path).unwrap(), false).unwrap();
        recalculated.project.save(&path).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let mut session = Session::default();
        assert!(session.open_path(&path), "{:?}", session.notice);
        session.save_to(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removing_a_saved_casing_needs_a_second_click() {
        let dir = scratch("carcasa-quitar");
        let path = copy_fixture(&dir, "casing_v3.hmf");
        let mut session = Session::default();
        assert!(session.open_path(&path), "{:?}", session.notice);

        assert_eq!(session.request_remove_casing(), Gate::Armed);
        assert_eq!(session.notice.as_deref(), Some(CASING_NOTICE));
        session.edited();
        assert_eq!(session.notice, None);
        assert_eq!(session.request_remove_casing(), Gate::Armed);
        assert_eq!(session.request_remove_casing(), Gate::Proceed);
        assert_eq!(session.notice, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_casing_not_yet_saved_goes_away_at_once() {
        let mut session = Session::default();
        session.inputs.casing = Some(sample_boxes());
        assert_eq!(session.request_remove_casing(), Gate::Proceed);
    }

    #[test]
    fn saving_without_the_casing_removes_it_from_the_file() {
        let dir = scratch("carcasa-fuera");
        let path = copy_fixture(&dir, "casing_v3.hmf");
        let mut session = Session::default();
        assert!(session.open_path(&path), "{:?}", session.notice);
        session.inputs.casing = None;
        session.edited();
        session.save_to(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap().casing, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_window_keeps_the_casing_through_edit_and_save() {
        let dir = scratch("carcasa");
        let path = copy_fixture(&dir, "casing_v3.hmf");
        let mut project = Project::load(&path).unwrap();
        let casing = hm_project::Casing {
            seal_clearance: "0.3 mm".into(),
            seal_length: "15 mm".into(),
            rear_seal_diameter: "80 mm".into(),
            rear_seal_length: "15 mm".into(),
            balance_hole_count: 6,
            balance_hole_diameter: "9 mm".into(),
            balance_hole_circle: "50 mm".into(),
            ..project.casing.clone().unwrap()
        };
        project.casing = Some(casing.clone());
        hm_engine::recalc(project, false)
            .unwrap()
            .project
            .save(&path)
            .unwrap();

        let mut session = Session::default();
        assert!(session.open_path(&path), "{:?}", session.notice);
        session.inputs.h = "46 m".into();
        session.edited();
        session.save_to(&path).unwrap();

        let saved = Project::load(&path).unwrap();
        assert_eq!(saved.casing, Some(casing));
        assert_eq!(saved.operating.h, "46 m");
        let mut again = Session::default();
        assert!(again.open_path(&path), "{:?}", again.notice);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn param_text(project: &Project, name: &str) -> String {
        match &project.params[name] {
            Param::Manual { value } | Param::Locked { value } => value.clone(),
            Param::Auto { value, .. } => value.clone().unwrap_or_default(),
            Param::Override { .. } => String::new(),
        }
    }

    fn codes(session: &Session) -> Vec<&str> {
        session
            .opened_diagnostics
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect()
    }

    #[test]
    fn saving_a_new_design_and_reopening_gives_the_same_boxes() {
        let dir = scratch("nuevo");
        let mut session = Session::default();
        session.edited();

        session.save_to(&dir.join("bomba")).unwrap();

        assert!(!session.dirty);
        assert!(session.notice.is_none());
        assert!(
            session
                .bar_label()
                .as_deref()
                .unwrap()
                .ends_with("bomba.hmf")
        );
        assert!(!session.bar_label().unwrap().contains('●'));
        let saved = session.inputs.clone();
        let path = session.file.clone().unwrap();

        let mut again = Session::default();
        assert!(again.open_path(&path));
        assert_eq!(again.inputs, saved);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn eta_m_empty_becomes_the_resolved_number_after_saving() {
        let dir = scratch("eta");
        let mut session = Session::default();
        assert!(session.inputs.eta_m.is_empty());

        session.save_to(&dir.join("bomba.hmf")).unwrap();

        let eta_m = session.inputs.eta_m.parse::<f64>().unwrap();
        assert!(eta_m > 0.0 && eta_m < 1.0, "{eta_m}");
        assert_eq!(
            param_text(session.project.as_ref().unwrap(), "eta_m"),
            session.inputs.eta_m
        );

        let mut again = Session::default();
        assert!(again.open_path(session.file.as_ref().unwrap()));
        assert_eq!(again.inputs.eta_m, session.inputs.eta_m);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_locked_eta_m_comes_back_to_the_box() {
        let dir = scratch("eta-locked");
        let path = copy_fixture(&dir, "end_suction_v0.9.hmf");
        let mut project = Project::load(&path).unwrap();
        let eta_m = param_text(&project, "eta_m");
        project.params.insert(
            "eta_m".into(),
            Param::Locked {
                value: eta_m.clone(),
            },
        );
        project.save(&path).unwrap();
        let mut session = Session::default();
        assert!(session.open_path(&path));
        session.inputs.eta_m = "0.9".into();

        session.save_to(&path).unwrap();

        assert_eq!(session.inputs.eta_m, eta_m);
        assert!(matches!(
            &session.project.as_ref().unwrap().params["eta_m"],
            Param::Locked { value } if value == &eta_m
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manual_rho_follows_the_temperature_on_save() {
        let dir = scratch("rho-manual");
        let path = copy_fixture(&dir, "end_suction_v0.9.hmf");
        let mut session = Session::default();
        assert!(session.open_path(&path));
        assert_eq!(
            param_text(session.project.as_ref().unwrap(), "rho"),
            "998.204 kg/m3"
        );
        session.inputs.temperature_c = "60".into();

        session.save_to(&path).unwrap();

        let rho = param_text(session.project.as_ref().unwrap(), "rho");
        let expected = format!(
            "{:.3} kg/m3",
            hm_design::water_density(60.0 + 273.15).unwrap()
        );
        assert_eq!(rho, expected);
        assert!(matches!(
            session.project.as_ref().unwrap().params.get("rho"),
            Some(Param::Manual { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_locked_rho_stays_and_shows_hm_des_009() {
        let dir = scratch("rho-locked");
        let path = copy_fixture(&dir, "end_suction_v0.9.hmf");
        let mut project = Project::load(&path).unwrap();
        let rho = param_text(&project, "rho");
        project
            .params
            .insert("rho".into(), Param::Locked { value: rho.clone() });
        project.save(&path).unwrap();

        let mut session = Session::default();
        assert!(session.open_path(&path));
        session.inputs.temperature_c = "60".into();
        session.dirty = true;

        session.save_to(&path).unwrap();

        assert!(matches!(
            &session.project.as_ref().unwrap().params["rho"],
            Param::Locked { value } if value == &rho
        ));
        assert_eq!(
            codes(&session)
                .iter()
                .filter(|code| **code == "HM-DES-009")
                .count(),
            1
        );
        assert!(!session.dirty);
        assert_eq!(session.inputs.temperature_c, "60");

        // Lo guardado se puede volver a abrir, con la misma densidad.
        let mut again = Session::default();
        assert!(again.open_path(&path), "{:?}", again.notice);
        assert_eq!(again.inputs, session.inputs);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_manual_selection_shows_hm_des_013_once() {
        let dir = scratch("seleccion");
        let path = copy_fixture(&dir, "end_suction_v0.9.hmf");
        let mut session = Session::default();
        assert!(session.open_path(&path));
        let psi = param_text(session.project.as_ref().unwrap(), "psi");
        session.inputs.q = "250 m3/h".into();

        session.save_to(&path).unwrap();

        assert_eq!(
            codes(&session)
                .iter()
                .filter(|code| **code == "HM-DES-013")
                .count(),
            1
        );
        assert_eq!(param_text(session.project.as_ref().unwrap(), "psi"), psi);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_input_error_does_not_create_or_modify_the_file() {
        let dir = scratch("caja-mala");
        let created = dir.join("nueva.hmf");
        let mut session = Session::default();
        session.inputs.q = "mucho".into();
        session.edited();
        assert_eq!(session.request(Command::New), Gate::Armed);

        assert!(session.save_to(&created).is_err());

        assert!(!created.exists());
        assert!(session.dirty);
        assert!(session.notice.is_some());
        assert_eq!(session.inputs.q, "mucho");
        assert!(session.file.is_none());
        // Guardar desarmó: la siguiente orden vuelve a pedir confirmación.
        assert_eq!(session.request(Command::New), Gate::Armed);

        let path = copy_fixture(&dir, "end_suction_v0.9.hmf");
        let bytes = std::fs::read(&path).unwrap();
        let mut session = Session::default();
        assert!(session.open_path(&path));
        session.inputs.q = "mucho".into();
        session.dirty = true;
        assert!(session.save_to(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(session.dirty);
        assert_eq!(session.inputs.q, "mucho");
        assert_eq!(session.file.as_deref(), Some(path.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_stage_does_not_create_or_modify_the_file() {
        let dir = scratch("etapa");
        let path = dir.join("etapa.hmf");
        let mut session = Session::default();
        session.save_to(&path).unwrap();
        // Solo la razón de garganta, como el caso del motor que tumba la geometría.
        // El resto de selecciones del diseño nuevo forzarían otro punto.
        let mut project = Project::new("etapa", hm_engine::PumpType::EndSuction);
        project.design.method = hm_engine::METHOD.to_string();
        project.params.insert(
            "volute_ratio".into(),
            Param::Manual {
                value: "0.5".into(),
            },
        );
        session.project = Some(project);
        session.basis = Basis {
            design: hm_engine::DesignOptions {
                volute_ratio: Some(0.5),
                ..Default::default()
            },
            ..Basis::default()
        };
        session.inputs.q = "4 m3/s".into();
        session.inputs.h = "30 m".into();
        session.inputs.n = "500 rpm".into();
        session.dirty = true;
        let bytes = std::fs::read(&path).unwrap();

        let error = session.save_to(&path).unwrap_err();

        assert!(
            error.contains("difusor") || error.contains("tubuladura"),
            "{error}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(session.dirty);
        assert!(session.notice.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_as_adds_the_hmf_extension_and_accepts_spaces_and_accents() {
        let dir = scratch("nombre");
        let mut session = Session::default();

        session.save_to(&dir.join("Bomba de río")).unwrap();

        let saved = dir.join("Bomba de río.hmf");
        assert!(saved.is_file());
        assert_eq!(session.file.as_deref(), Some(saved.as_path()));
        assert_eq!(
            session.project.as_ref().unwrap().project.name,
            "Bomba de río"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adding_the_extension_never_overwrites_another_file() {
        let dir = scratch("colision");
        let existing = dir.join("bomba.hmf");
        std::fs::write(&existing, "mío").unwrap();
        let mut session = Session::default();
        session.edited();

        let error = session.save_to(&dir.join("bomba")).unwrap_err();

        assert!(error.contains("bomba.hmf"), "{error}");
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "mío");
        assert!(session.dirty);
        assert!(session.file.is_none());
        assert_eq!(session.notice.as_deref(), Some(error.as_str()));

        // Escribir el nombre completo es elegirlo: ahí el diálogo ya preguntó.
        session.save_to(&existing).unwrap();
        assert_ne!(std::fs::read_to_string(&existing).unwrap(), "mío");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_legacy_project_is_migrated_only_when_saved() {
        let dir = scratch("legado");
        let path = copy_fixture(&dir, "end_suction_v0.8.hmf");
        let bytes = std::fs::read(&path).unwrap();
        let mut session = Session::default();
        assert!(session.open_path(&path));

        assert_eq!(
            session.project.as_ref().unwrap().design.method,
            "end_suction_v0.8"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);

        session.save_to(&path).unwrap();

        assert_eq!(
            Project::load(&path).unwrap().design.method,
            hm_engine::METHOD
        );
        assert_ne!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::read(fixture("end_suction_v0.8.hmf")).unwrap(),
            bytes
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Cajas, archivo, `●`, avisos y el proyecto en memoria.
    fn session_stamp(session: &Session) -> String {
        let codes = session.opened_diagnostics.as_ref().map(|diagnostics| {
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.clone())
                .collect::<Vec<_>>()
        });
        let project = session
            .project
            .as_ref()
            .map(|project| project.to_toml().unwrap())
            .unwrap_or_default();
        format!(
            "{:?}\n{:?}\n{:?}\n{}\n{:?}\n{codes:?}\n{project}",
            session.inputs, session.basis, session.file, session.dirty, session.notice,
        )
    }

    fn dir_listing(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut names = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn export_does_not_touch_the_session() {
        let dir = scratch("export-sesion");
        let path = copy_fixture(&dir, "end_suction_v0.9.hmf");
        let bytes = std::fs::read(&path).unwrap();
        let mut session = Session::default();
        assert!(session.open_path(&path));
        session.inputs.q = "100 m3/h".into();
        session.edited();
        let before = session_stamp(&session);

        let json = session.export_json().unwrap();

        assert!(json.ends_with('\n'));
        assert_eq!(session_stamp(&session), before);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(session.dirty);
        assert!(session.bar_label().unwrap().contains('●'));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_of_a_legacy_project_does_not_modify_the_file() {
        let dir = scratch("export-legado");
        let path = copy_fixture(&dir, "end_suction_v0.8.hmf");
        let bytes = std::fs::read(&path).unwrap();
        let mut session = Session::default();
        assert!(session.open_path(&path));
        assert!(!session.dirty);
        let before = session_stamp(&session);

        let json = session.export_json().unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            Project::load(&path).unwrap().design.method,
            "end_suction_v0.8"
        );
        assert_eq!(
            session.project.as_ref().unwrap().design.method,
            "end_suction_v0.8"
        );
        assert_eq!(session_stamp(&session), before);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["method"], hm_engine::METHOD);
        assert_eq!(value["format"], hm_engine::REPORT_FORMAT);
        assert_eq!(value["curve"].as_array().unwrap().len(), 11);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_fails_with_an_input_error_and_writes_nothing() {
        let dir = scratch("export-caja");
        let path = copy_fixture(&dir, "end_suction_v0.9.hmf");
        let bytes = std::fs::read(&path).unwrap();
        let listing = dir_listing(&dir);
        let mut session = Session::default();
        assert!(session.open_path(&path));
        session.inputs.q = "mucho".into();
        session.edited();
        let before = session_stamp(&session);

        let error = session.export_json().unwrap_err();

        assert!(!error.is_empty(), "{error}");
        assert_eq!(session.notice.take(), Some(error));
        assert_eq!(session_stamp(&session), before);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(dir_listing(&dir), listing);
        assert!(session.dirty);
        assert_eq!(session.inputs.q, "mucho");

        // Sin una etapa tampoco hay JSON, y el archivo sigue igual.
        let mut session = Session::default();
        session.save_to(&dir.join("etapa.hmf")).unwrap();
        let mut project = Project::new("etapa", hm_engine::PumpType::EndSuction);
        project.design.method = hm_engine::METHOD.to_string();
        project.params.insert(
            "volute_ratio".into(),
            Param::Manual {
                value: "0.5".into(),
            },
        );
        session.project = Some(project);
        session.basis = Basis {
            design: hm_engine::DesignOptions {
                volute_ratio: Some(0.5),
                ..Default::default()
            },
            ..Basis::default()
        };
        session.inputs.q = "4 m3/s".into();
        session.inputs.h = "30 m".into();
        session.inputs.n = "500 rpm".into();
        session.dirty = true;
        let etapa = session.file.clone().unwrap();
        let etapa_bytes = std::fs::read(&etapa).unwrap();
        let before = session_stamp(&session);

        let error = session.export_json().unwrap_err();

        assert!(
            error.contains("difusor") || error.contains("tubuladura"),
            "{error}"
        );
        assert_eq!(session.notice.take(), Some(error));
        assert_eq!(std::fs::read(&etapa).unwrap(), etapa_bytes);
        assert_eq!(session_stamp(&session), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_saved_clean_export_matches_the_report_of_the_file() {
        let dir = scratch("export-limpio");
        let mut session = Session::default();
        session.save_to(&dir.join("bomba.hmf")).unwrap();
        assert!(!session.dirty);
        let bytes = std::fs::read(session.file.as_ref().unwrap()).unwrap();
        let before = session_stamp(&session);

        let json = session.export_json().unwrap();

        assert_eq!(session_stamp(&session), before);
        assert_eq!(
            std::fs::read(session.file.as_ref().unwrap()).unwrap(),
            bytes
        );
        let loaded = Project::load(session.file.as_ref().unwrap()).unwrap();
        let engine = hm_engine::report(&loaded, hm_engine::CurvePoints::new(11).unwrap()).unwrap();
        let exported: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(exported, serde_json::to_value(&engine).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_to_adds_the_json_extension_and_leaves_the_session_alone() {
        let dir = scratch("export-json");
        let mut session = Session::default();
        session.edited();
        session.notice = Some("aviso viejo".into());

        session.export_to(&dir.join("informe"), "{}\n").unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.join("informe.json")).unwrap(),
            "{}\n"
        );
        assert_eq!(session.notice, None);
        assert!(session.dirty);
        assert_eq!(session.file, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_to_keeps_its_extension_and_adds_to_another() {
        let dir = scratch("export-extension");
        let mut session = Session::default();

        session.export_to(&dir.join("a.JSON"), "a").unwrap();
        session.export_to(&dir.join("b.hmf"), "b").unwrap();

        assert_eq!(
            dir_listing(&dir),
            vec![dir.join("a.JSON"), dir.join("b.hmf.json")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_to_does_not_overwrite_a_file_the_dialog_did_not_ask_about() {
        let dir = scratch("export-colision");
        std::fs::write(dir.join("bomba.json"), "mío").unwrap();
        let mut session = Session::default();

        let error = session.export_to(&dir.join("bomba"), "nuevo").unwrap_err();

        assert!(error.contains("bomba.json"), "{error}");
        assert_eq!(session.notice, Some(error));
        assert_eq!(
            std::fs::read_to_string(dir.join("bomba.json")).unwrap(),
            "mío"
        );
        assert_eq!(dir_listing(&dir), vec![dir.join("bomba.json")]);

        // El nombre exacto sí lo preguntó el diálogo.
        session.export_to(&dir.join("bomba.json"), "nuevo").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("bomba.json")).unwrap(),
            "nuevo"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_to_reports_a_write_failure_and_keeps_the_mark() {
        let dir = scratch("export-falla");
        let mut session = Session::default();
        session.edited();

        let error = session
            .export_to(&dir.join("no-existe").join("informe.json"), "x")
            .unwrap_err();

        assert_eq!(session.notice, Some(error));
        assert!(session.dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Editada Q y armado Nuevo: la sesión que usan las dos pruebas siguientes.
    fn armed_session() -> Session {
        let mut session = Session::default();
        session.inputs.q = "100 m3/h".into();
        session.edited();
        assert_eq!(session.request(Command::New), Gate::Armed);
        session
    }

    #[test]
    fn exporting_disarms_the_discard() {
        let mut session = armed_session();

        // Sin `export_to`: es lo que pasa si se cancela el diálogo.
        session.export_json().unwrap();

        assert_eq!(session.discard_notice(), None);
        assert_eq!(session.request(Command::New), Gate::Armed);
        assert_eq!(session.inputs.q, "100 m3/h");
    }

    #[test]
    fn a_failed_export_disarms_the_discard_too() {
        let mut session = armed_session();
        session.inputs.q = "mucho".into();

        session.export_json().unwrap_err();

        assert_eq!(session.request(Command::New), Gate::Armed);
    }
}
