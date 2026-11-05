//! Sesión de la ventana: archivo abierto, cajas y qué avisos mostrar.
//!
//! Sin egui. No calcula: abrir es `open_file`, y el primer cambio de una caja
//! devuelve el panel al resultado de `compute`.

use std::path::{Path, PathBuf};

use hm_core::Diagnostic;
use hm_project::Project;

use crate::model::{Basis, Inputs, open_file};

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
                self.opened_diagnostics = Some(opened.diagnostics);
                true
            }
            Err(message) => {
                self.notice = Some(message);
                false
            }
        }
    }

    /// Una caja cambió: el panel vuelve a los avisos del cálculo.
    pub fn boxes_changed(&mut self) {
        self.opened_diagnostics = None;
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

        session.boxes_changed();

        assert!(session.opened_diagnostics.is_none());
        assert!(session.project.is_some());
    }
}
