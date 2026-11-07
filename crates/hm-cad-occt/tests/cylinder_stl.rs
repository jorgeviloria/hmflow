use std::fs;

use hm_cad_occt::{CadError, export_cylinder_stl, occt_linked};

#[test]
fn cylinder_stl_matches_the_brep_volume() {
    let path = std::env::temp_dir().join(format!("hmflow-cilindro-{}.stl", std::process::id()));

    if !occt_linked() {
        let error = export_cylinder_stl(0.05, 0.1, 0.001, &path).expect_err("sin OCCT");
        assert_eq!(error, CadError::Unavailable);
        return;
    }

    let radius_m = 0.05;
    let height_m = 0.12;
    let exported =
        export_cylinder_stl(radius_m, height_m, 0.001, &path).expect("el cilindro se exporta");

    let expected = std::f64::consts::PI * radius_m * radius_m * height_m;
    let relative = (exported.volume_m3 - expected).abs() / expected;
    assert!(
        relative < 1e-9,
        "volumen {} frente a {}",
        exported.volume_m3,
        expected
    );
    assert!(exported.triangle_count > 0);

    let stl = fs::read_to_string(&path).expect("el STL se lee");
    let written = stl.matches("facet normal").count() as u64;
    assert_eq!(written, exported.triangle_count);
    assert!(stl.starts_with("solid"));

    let _ = fs::remove_file(path);
}

#[cfg(unix)]
#[test]
fn a_non_utf8_path_is_rejected() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    let path = PathBuf::from(OsString::from_vec(vec![0x66, 0xff, 0x2e]));
    let error = export_cylinder_stl(0.05, 0.1, 0.001, &path).expect_err("ruta");
    assert_eq!(error, CadError::PathNotUtf8);
}
