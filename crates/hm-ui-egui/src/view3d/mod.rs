//! Visor 3D: cámara, gestos y, más adelante, el dibujo con wgpu. La vista no
//! calcula geometría: todo vértice sale de `hm_design::scene`.

// Nadie usa todavía la cámara ni los gestos: la Tarea 4 los conecta y quita
// este permiso.
#![allow(dead_code)]

pub(crate) mod camera;
pub(crate) mod input;
