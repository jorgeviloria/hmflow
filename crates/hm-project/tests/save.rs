//! `save` no deja a medias el archivo que ya existía.

use std::path::{Path, PathBuf};

use hm_core::PumpType;
use hm_project::Project;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hm-project-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn project() -> Project {
    Project::new("bomba", PumpType::EndSuction)
}

fn entries(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

/// Un enlace duro sigue apuntando a los datos viejos: si `save` los hubiera
/// truncado para reescribirlos, aquí se vería el contenido nuevo.
#[cfg(unix)]
#[test]
fn saving_replaces_the_file_and_never_touches_the_old_data() {
    let dir = scratch("reemplazo");
    let path = dir.join("bomba.hmf");
    std::fs::write(&path, "viejo").unwrap();
    let old = dir.join("copia");
    std::fs::hard_link(&path, &old).unwrap();

    project().save(&path).unwrap();

    assert_eq!(std::fs::read_to_string(&old).unwrap(), "viejo");
    assert_eq!(Project::load(&path).unwrap().project.name, "bomba");
    assert_eq!(entries(&dir), 2, "no queda ningún temporal");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saving_a_new_file_leaves_only_that_file() {
    let dir = scratch("nuevo");
    let path = dir.join("Bomba de río.hmf");

    project().save(&path).unwrap();

    assert!(path.is_file());
    assert_eq!(entries(&dir), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_read_only_file_is_an_error_and_stays_as_it_was() {
    let dir = scratch("solo-lectura");
    let path = dir.join("bomba.hmf");
    std::fs::write(&path, "viejo").unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();

    assert!(project().save(&path).is_err());

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "viejo");
    assert_eq!(entries(&dir), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failure_leaves_no_temporary_file() {
    let dir = scratch("carpeta");
    let path = dir.join("carpeta.hmf");
    std::fs::create_dir(&path).unwrap();

    assert!(project().save(&path).is_err());

    assert_eq!(entries(&dir), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn saving_through_a_link_writes_the_real_file_and_keeps_the_link() {
    let dir = scratch("enlace");
    let real = dir.join("real.hmf");
    std::fs::write(&real, "viejo").unwrap();
    let link = dir.join("enlace.hmf");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    project().save(&link).unwrap();

    assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(Project::load(&real).unwrap().project.name, "bomba");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn saving_keeps_the_permissions_of_the_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = scratch("permisos");
    let path = dir.join("bomba.hmf");
    std::fs::write(&path, "viejo").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

    project().save(&path).unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
    let _ = std::fs::remove_dir_all(&dir);
}
