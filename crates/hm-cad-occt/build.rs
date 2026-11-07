//! Enlaza OpenCASCADE si está instalado. Si no, el crate compila igual y
//! las operaciones de sólido devuelven `CadError::Unavailable`.

use std::path::{Path, PathBuf};

/// Toolkits que el puente referencia, de arriba abajo, para el enlazador de una pasada.
const OCCT_LIBS: &[&str] = &[
    "TKDESTL",
    "TKDE",
    "TKMesh",
    "TKShHealing",
    "TKBO",
    "TKPrim",
    "TKTopAlgo",
    "TKGeomAlgo",
    "TKBRep",
    "TKGeomBase",
    "TKG3d",
    "TKG2d",
    "TKMath",
    "TKService",
    "TKernel",
];

fn main() {
    println!("cargo:rustc-check-cfg=cfg(hm_occt)");
    println!("cargo:rerun-if-env-changed=OCCT_PREFIX");
    println!("cargo:rerun-if-env-changed=OCCT_LIB_DIR");
    println!("cargo:rerun-if-changed=src/bridge.rs");
    println!("cargo:rerun-if-changed=cpp/cylinder.hxx");
    println!("cargo:rerun-if-changed=cpp/cylinder.cpp");

    let Some(prefix) = occt_prefix() else {
        println!(
            "cargo:warning=OpenCASCADE no está instalado; hm-cad-occt compila sin sólidos. Defina OCCT_PREFIX."
        );
        return;
    };
    let Some(lib_dir) = occt_lib_dir(&prefix) else {
        println!(
            "cargo:warning=OCCT_PREFIX={prefix} no tiene libTKernel; hm-cad-occt compila sin sólidos.",
            prefix = prefix.display()
        );
        return;
    };

    let include = prefix.join("include").join("opencascade");
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo lo define"));
    let crates_dir = manifest_dir
        .parent()
        .expect("el crate vive en crates/")
        .to_path_buf();

    cxx_build::bridge("src/bridge.rs")
        .file("cpp/cylinder.cpp")
        .include(&crates_dir)
        .include(&include)
        .std("c++17")
        .define("OCC_CONVERT_SIGNALS", None)
        .flag_if_supported("-Wno-deprecated-declarations")
        .compile("hm-cad-occt-bridge");

    println!("cargo:rustc-cfg=hm_occt");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
    for lib in OCCT_LIBS {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
}

fn occt_prefix() -> Option<PathBuf> {
    if let Some(prefix) = std::env::var_os("OCCT_PREFIX") {
        let prefix = PathBuf::from(prefix);
        if has_header(&prefix) {
            return Some(prefix);
        }
        return None;
    }
    [
        "/opt/homebrew/opt/opencascade",
        "/usr/local/opt/opencascade",
        "/usr",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|prefix| has_header(prefix))
}

fn has_header(prefix: &Path) -> bool {
    prefix
        .join("include/opencascade/BRepPrimAPI_MakeCylinder.hxx")
        .is_file()
}

fn occt_lib_dir(prefix: &Path) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OCCT_LIB_DIR") {
        let dir = PathBuf::from(dir);
        return has_kernel(&dir).then_some(dir);
    }
    ["lib", "lib/x86_64-linux-gnu", "lib/aarch64-linux-gnu"]
        .into_iter()
        .map(|rel| prefix.join(rel))
        .find(|dir| has_kernel(dir))
}

fn has_kernel(dir: &Path) -> bool {
    ["libTKernel.dylib", "libTKernel.so", "TKernel.lib"]
        .into_iter()
        .any(|name| dir.join(name).is_file())
}
