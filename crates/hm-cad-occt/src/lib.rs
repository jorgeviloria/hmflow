//! Sólidos con OpenCASCADE, enlazado como biblioteca aparte (LGPL-2.1 con excepción).
//!
//! Esta primera operación no diseña una bomba: construye un cilindro cerrado,
//! mide su volumen en el B-Rep y escribe la malla de ese mismo sólido a STL.
//! El rodete y la voluta se apoyan aquí cuando el modelo geométrico exista.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

use std::path::Path;

use thiserror::Error;

#[cfg(hm_occt)]
mod bridge;

/// Resultado de exportar un sólido: el volumen del B-Rep y los triángulos escritos.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidExport {
    pub volume_m3: f64,
    pub triangle_count: u64,
}

/// Fallo al construir o escribir un sólido.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CadError {
    #[error("el radio y la altura tienen que ser finitos y mayores que cero")]
    NotPositive,

    #[error("la deflexión de la malla tiene que ser finita y mayor que cero")]
    BadDeflection,

    #[error("la ruta del STL no es UTF-8")]
    PathNotUtf8,

    #[error(
        "OpenCASCADE no está enlazado; instálelo y defina OCCT_PREFIX si no está en la ruta habitual"
    )]
    Unavailable,

    #[error("{0}")]
    Kernel(String),
}

/// `true` cuando este binario se compiló contra OpenCASCADE.
#[must_use]
pub fn occt_linked() -> bool {
    cfg!(hm_occt)
}

/// Cilindro macizo de eje Z. Unidades en metros. El STL es ASCII y sale del mismo B-Rep.
pub fn export_cylinder_stl(
    radius_m: f64,
    height_m: f64,
    deflection_m: f64,
    path: &Path,
) -> Result<SolidExport, CadError> {
    if !(radius_m.is_finite() && radius_m > 0.0 && height_m.is_finite() && height_m > 0.0) {
        return Err(CadError::NotPositive);
    }
    if !(deflection_m.is_finite() && deflection_m > 0.0) {
        return Err(CadError::BadDeflection);
    }
    let Some(path) = path.to_str() else {
        return Err(CadError::PathNotUtf8);
    };

    #[cfg(not(hm_occt))]
    {
        let _ = path;
        Err(CadError::Unavailable)
    }

    #[cfg(hm_occt)]
    {
        let volume_m3 = bridge::ffi::cylinder_volume_m3(radius_m, height_m).map_err(kernel)?;
        let triangle_count =
            bridge::ffi::write_cylinder_stl(radius_m, height_m, deflection_m, path)
                .map_err(kernel)?;
        Ok(SolidExport {
            volume_m3,
            triangle_count,
        })
    }
}

#[cfg(hm_occt)]
fn kernel(error: cxx::Exception) -> CadError {
    CadError::Kernel(error.what().to_owned())
}

#[cfg(test)]
mod tests {
    use super::{CadError, export_cylinder_stl};

    #[test]
    fn a_non_positive_radius_does_not_call_the_kernel() {
        let error = export_cylinder_stl(0.0, 0.1, 0.001, std::path::Path::new("no-se-escribe.stl"))
            .expect_err("radio nulo");
        assert_eq!(error, CadError::NotPositive);
        assert!(!std::path::Path::new("no-se-escribe.stl").exists());
    }

    #[test]
    fn a_zero_deflection_is_rejected() {
        let error = export_cylinder_stl(0.05, 0.1, 0.0, std::path::Path::new("no-se-escribe.stl"))
            .expect_err("deflexión nula");
        assert_eq!(error, CadError::BadDeflection);
    }
}
