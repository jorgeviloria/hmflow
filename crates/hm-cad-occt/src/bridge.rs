//! Puente cxx con el cilindro de prueba. Las excepciones de OCCT salen como `Err`.

#![allow(unsafe_code)]

#[cxx::bridge]
pub(crate) mod ffi {
    unsafe extern "C++" {
        include!("hm-cad-occt/cpp/cylinder.hxx");

        fn cylinder_volume_m3(radius_m: f64, height_m: f64) -> Result<f64>;

        fn write_cylinder_stl(
            radius_m: f64,
            height_m: f64,
            deflection_m: f64,
            path: &str,
        ) -> Result<u64>;
    }
}
