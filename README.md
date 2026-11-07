# HidroMac Flow (HMFlow)

Suite open source de diseño de bombas centrífugas, escrita en Rust.

> Estado: **Parte 0 — cimientos** y motor de diseño end-suction
> (`end_suction_v0.9`). El repositorio compila, hay 89 fórmulas auditadas
> (58 piloto, 31 con referencia de ejemplo resuelto, 0 de banco) y `design`
> genera geometría, voluta, pérdidas y curvas preliminares. El cálculo lo
> comparte `hm-engine`. La ventana egui abre un `.hmf`, edita el punto y la
> aspiración, guarda y exporta el informe. OpenCASCADE está enlazado: hoy
> exporta un cilindro de prueba a STL. El rodete y la voluta todavía no son sólidos.

## Qué es

Una herramienta para diseñar bombas centrífugas con las fórmulas a la vista:
cada número se puede abrir, leer, sustituir y auditar.

Alcance: end-suction, inline, multietapa ring-section, carcasa partida con
doble aspiración y barrel. Solo bombas centrífugas.

## Qué no es todavía

- No hay validación de banco todavía: es lo que cierra la Parte 0.
- Las curvas H(Q), η(Q) y NPSHr(Q) son preliminares; falta el modelo de
  pérdidas detallado y su contraste con medidas.
- La ventana egui abre, edita el punto y la aspiración, guarda el `.hmf` y
  exporta el informe. No edita los parámetros del método, ni un fluido que
  no sea agua, ni una viscosidad escrita.
- El kernel CAD enlaza OpenCASCADE y exporta un cilindro a STL. Todavía no
  hay sólido de rodete ni de voluta.
- No hay CFD.
- No sustituye la revisión de un ingeniero.

## Compilar

```sh
# macOS. En Linux, OCCT_PREFIX apunta a la instalación del sistema.
brew install opencascade

cargo build
cargo test
cargo run -p hmflow-cli -- formulas
```

Requiere Rust 1.95 o superior. Sin OpenCASCADE el resto del proyecto compila;
`hm-cad-occt` responde que el kernel no está enlazado.

## Uso rápido

```sh
# Crear un proyecto
cargo run -p hmflow-cli -- new bomba-demo

# Diseñar una bomba end-suction con curvas. Sin --npsha no se evalúa la
# cavitación; con él, el margen se exige en la región preferente (0,7–1,2 Qd)
cargo run -p hmflow-cli -- design --q "120 m3/h" --h "45 m" --n "2900 rpm" --npsha "15 m" --out bomba.hmf --curves

# Agua caliente: sin --rho se usa la densidad del agua a esa temperatura
cargo run -p hmflow-cli -- design --temperature "80 degC" --npsha "15 m" --out caliente.hmf

# NPSHa calculado de la instalación (presión de vapor del agua por IAPWS-IF97)
# y corrección por viscosidad de un aceite
cargo run -p hmflow-cli -- design --fluid oil --rho "880 kg/m3" --viscosity "100 cSt" \
  --suction-pressure "1 atm" --suction-height "8 m" --vapor-pressure "1 kPa" --out aceite.hmf

# Validarlo: dimensiones, signos, campos desconocidos, esquema del método y avisos
cargo run -p hmflow-cli -- check bomba.hmf

# Tras editar a mano una selección (psi, beta2, blade_count…), regenerar
# los parámetros automáticos conservando las selecciones manuales
cargo run -p hmflow-cli -- recalc bomba.hmf

# Tras cambiar Q, H o n, volver a tomar de la tabla las selecciones que
# dependen de nq (las locked se respetan)
cargo run -p hmflow-cli -- recalc --reselect bomba.hmf

# Diseñar un rodete para una carcasa existente (archivo con una tabla [casing])
cargo run -p hmflow-cli -- design --q "120 m3/h" --h "45 m" --n "2900 rpm" --casing carcasa.toml --out rodete.hmf

# Reducir un ensayo de banco y compararlo con el .hmf que indique el caso
cargo run -p hmflow-cli -- bench case.toml

# Informe JSON para la interfaz o el CAD: valores en SI, voluta, curva,
# geometría del álabe y de la salida, y diagnósticos (docs/report-format.md)
cargo run -p hmflow-cli -- report bomba.hmf --out bomba.json

# Ver las fórmulas disponibles
cargo run -p hmflow-cli -- formulas

# Ventana: abre, edita el punto, guarda y exporta el informe
# (workspace aparte; no entra en `cargo build` del raíz)
cargo run --manifest-path crates/hm-ui-egui/Cargo.toml

# Evaluar una expresión con unidades
cargo run -p hmflow-cli -- eval "Q/(pi*d2*b2)" --var "Q=120 m3/h" --var "d2=0.3 m" --var "b2=0.02 m"
```

## Estructura

| Crate | Contenido |
|---|---|
| `hm-core` | Diagnósticos, severidad, revisión, tipo de bomba |
| `hm-units` | Dimensiones, unidades, cantidades |
| `hm-math` | Interpolación, raíces, utilidades |
| `hm-formula` | AST, parser, análisis dimensional, evaluación, packs |
| `hm-design` | Diseño preliminar, voluta, curvas y validación end-suction |
| `hm-project` | Formato `.hmf`, parámetros, validación |
| `hm-engine` | Caso de uso compartido por la CLI y la interfaz |
| `hmflow-cli` | CLI |
| `hm-ui-egui` | Ventana egui: abre, edita el punto, guarda y exporta (workspace propio) |
| `xtask` | Puertas de calidad, incluidas las capas de dependencia |

## Licencia

Apache-2.0 OR MIT. Sin CLA: se usa DCO.

## Agradecimiento

Desarrollado con el apoyo de **Hidromac** y validado en su banco de pruebas.
Hidromac no controla el roadmap del proyecto ni responde por sus resultados.

Ver [NOTICE](NOTICE) y [ATTRIBUTION.md](ATTRIBUTION.md).
