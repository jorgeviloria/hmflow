#pragma once

#include <cstdint>

#include "rust/cxx.h"

// Un cilindro macizo de eje Z, centrado en el origen del plano XY.
// El radio y la altura van en metros. Devuelve el volumen del B-Rep, en m³.
double cylinder_volume_m3(double radius_m, double height_m);

// Escribe la malla de ese cilindro en un STL ASCII y devuelve el número de triángulos.
// La deflexión lineal va en metros.
std::uint64_t write_cylinder_stl(
    double radius_m,
    double height_m,
    double deflection_m,
    rust::Str path);
