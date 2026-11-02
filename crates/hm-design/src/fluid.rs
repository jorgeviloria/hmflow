//! Propiedades del agua líquida: densidad, presión de vapor y viscosidad.
//!
//! Son correlaciones públicas con sus valores de verificación en los tests
//! (`tests/fluid.rs`). Fuera de su intervalo devuelven `None`: el motor no
//! extrapola propiedades.

/// Intervalo de validez de [`water_density`] y de las propiedades que la
/// usan, en °C.
pub const WATER_DENSITY_RANGE_C: (f64, f64) = (0.0, 150.0);

/// Densidad del agua líquida a 1 atm, en kg/m³, para una temperatura en
/// kelvin.
///
/// Correlación de Kell (1975), J. Chem. Eng. Data 20(1), 97–105, válida de
/// 0 a 150 °C. Da 998,20 kg/m³ a 20 °C y 971,80 kg/m³ a 80 °C. Fuera de ese
/// intervalo devuelve `None`.
pub fn water_density(temperature_k: f64) -> Option<f64> {
    let t = temperature_k - 273.15;
    if !t.is_finite() || !(WATER_DENSITY_RANGE_C.0..=WATER_DENSITY_RANGE_C.1).contains(&t) {
        return None;
    }
    let numerator =
        999.839_52 + 16.945_176 * t - 7.987_040_1e-3 * t.powi(2) - 46.170_461e-6 * t.powi(3)
            + 105.563_02e-9 * t.powi(4)
            - 280.542_53e-12 * t.powi(5);
    Some(numerator / (1.0 + 16.879_850e-3 * t))
}

/// Temperatura del punto triple del agua, en K.
const TRIPLE_POINT_K: f64 = 273.16;

/// Temperatura crítica del agua, en K.
const CRITICAL_TEMPERATURE_K: f64 = 647.096;

/// Coeficientes n1..n10 de la ecuación de saturación de IAPWS-IF97.
const IF97_SATURATION: [f64; 10] = [
    0.116_705_214_527_67e4,
    -0.724_213_167_032_06e6,
    -0.170_738_469_400_92e2,
    0.120_208_247_024_70e5,
    -0.323_255_503_223_33e7,
    0.149_151_086_135_30e2,
    -0.482_326_573_615_91e4,
    0.405_113_405_420_57e6,
    -0.238_555_575_678_49,
    0.650_175_348_447_98e3,
];

/// Presión de vapor (saturación) del agua, en Pa, para una temperatura en
/// kelvin.
///
/// IAPWS-IF97 (IAPWS R7-97, 2012), ecuación de saturación de la región 4,
/// válida desde el punto triple (273,16 K) hasta el punto crítico
/// (647,096 K). Fuera de ese intervalo devuelve `None`.
pub fn water_vapor_pressure(temperature_k: f64) -> Option<f64> {
    if !temperature_k.is_finite()
        || !(TRIPLE_POINT_K..=CRITICAL_TEMPERATURE_K).contains(&temperature_k)
    {
        return None;
    }
    let n = IF97_SATURATION;
    let theta = temperature_k + n[8] / (temperature_k - n[9]);
    let a = theta.powi(2) + n[0] * theta + n[1];
    let b = n[2] * theta.powi(2) + n[3] * theta + n[4];
    let c = n[5] * theta.powi(2) + n[6] * theta + n[7];
    let megapascal = (2.0 * c / (-b + (b.powi(2) - 4.0 * a * c).sqrt())).powi(4);
    Some(megapascal * 1e6)
}

/// Coeficientes H0..H3 del término de gas diluido de la viscosidad IAPWS 2008.
const VISCOSITY_DILUTE: [f64; 4] = [1.677_52, 2.204_62, 0.636_656_4, -0.241_605];

/// Coeficientes no nulos (i, j, Hij) del término residual de la viscosidad
/// IAPWS 2008.
const VISCOSITY_RESIDUAL: [(i32, i32, f64); 21] = [
    (0, 0, 5.200_94e-1),
    (1, 0, 8.508_95e-2),
    (2, 0, -1.083_74),
    (3, 0, -2.895_55e-1),
    (0, 1, 2.225_31e-1),
    (1, 1, 9.991_15e-1),
    (2, 1, 1.887_97),
    (3, 1, 1.266_13),
    (5, 1, 1.205_73e-1),
    (0, 2, -2.813_78e-1),
    (1, 2, -9.068_51e-1),
    (2, 2, -7.724_79e-1),
    (3, 2, -4.898_37e-1),
    (4, 2, -2.570_40e-1),
    (0, 3, 1.619_13e-1),
    (1, 3, 2.573_99e-1),
    (0, 4, -3.253_72e-2),
    (3, 4, 6.984_52e-2),
    (4, 5, 8.721_02e-3),
    (3, 6, -4.356_73e-3),
    (5, 6, -5.932_64e-4),
];

/// Viscosidad dinámica del agua, en Pa·s, para una temperatura en kelvin y una
/// densidad en kg/m³.
///
/// IAPWS R12-08 (2008), μ = μ0(T)·μ1(T, ρ) sin el término de realce crítico
/// μ2, que vale 1 lejos del punto crítico (la propia IAPWS lo omite para uso
/// industrial). Devuelve `None` con entradas no finitas o no positivas.
pub fn water_viscosity(temperature_k: f64, density: f64) -> Option<f64> {
    if !(temperature_k.is_finite() && density.is_finite()) || temperature_k <= 0.0 || density <= 0.0
    {
        return None;
    }
    let t = temperature_k / CRITICAL_TEMPERATURE_K;
    let rho = density / 322.0;
    let dilute_sum: f64 = VISCOSITY_DILUTE
        .iter()
        .enumerate()
        .map(|(i, h)| h / t.powi(i as i32))
        .sum();
    let mu0 = 100.0 * t.sqrt() / dilute_sum;
    let residual_sum: f64 = VISCOSITY_RESIDUAL
        .iter()
        .map(|&(i, j, h)| h * (1.0 / t - 1.0).powi(i) * (rho - 1.0).powi(j))
        .sum();
    let mu1 = (rho * residual_sum).exp();
    Some(mu0 * mu1 * 1e-6)
}

/// Viscosidad cinemática del agua líquida, en m²/s, para una temperatura en
/// kelvin: [`water_viscosity`] con la densidad de [`water_density`]. Vale
/// en el intervalo de esta última (0..150 °C).
pub fn water_kinematic_viscosity(temperature_k: f64) -> Option<f64> {
    let density = water_density(temperature_k)?;
    Some(water_viscosity(temperature_k, density)? / density)
}
