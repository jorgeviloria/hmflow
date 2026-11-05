//! Sesión de la ventana: archivo abierto, cajas y qué avisos mostrar.
//!
//! Sin egui. No calcula: abrir es `open_file`, y el primer cambio de una caja
//! devuelve el panel al resultado de `compute`. Nuevo y Abrir, con cambios,
//! piden repetir la orden antes de descartar.

use std::path::{Path, PathBuf};

use hm_core::Diagnostic;
use hm_project::Project;

use crate::model::{Basis, Inputs, open_file};

/// Orden del menú Archivo que puede tirar el trabajo en curso.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    New,
    Open,
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
        if self.discard_notice().is_some() {
            self.notice = None;
        }
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

    /// Nuevo: las entradas por defecto, sin archivo, sin proyecto y sin `●`.
    pub fn new_design(&mut self) {
        let shown_title = self.shown_title.take();
        *self = Self {
            shown_title,
            ..Self::default()
        };
    }
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
}
