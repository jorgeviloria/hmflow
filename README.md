# HidroMac Flow (HMFlow)

Suite open source de diseño de bombas centrífugas, escrita en Rust.

> Estado: **Parte 0 — cimientos** y primer motor de diseño end-suction. El
> repositorio compila, hay 71 fórmulas auditadas (40 piloto, 31 con referencia
> de ejemplo resuelto; sin validación de banco) y `design` genera geometría,
> voluta, pérdidas y curvas preliminares. Todavía no hay CAD ni interfaz
> gráfica.

## Qué es

Una herramienta para diseñar bombas centrífugas con las fórmulas a la vista:
cada número se puede abrir, leer, sustituir y auditar.

Alcance: end-suction, inline, multietapa ring-section, carcasa partida con
doble aspiración y barrel. Solo bombas centrífugas.

## Qué no es todavía

- No hay validación de banco todavía: es lo que cierra la Parte 0.
- Las curvas H(Q), η(Q) y NPSHr(Q) son preliminares; falta el modelo de
  pérdidas detallado y su contraste con medidas.
- No hay interfaz gráfica (llega en la Parte 1).
- No hay geometría CAD (llega en la Parte 2).
- No hay CFD.
- No sustituye la revisión de un ingeniero.

## Compilar

```sh
cargo build
cargo test
cargo run -p hmflow-cli -- formulas
```

Requiere Rust 1.95 o superior.

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

# Informe JSON para la interfaz o el CAD: valores en SI, voluta, curva,
# geometría del álabe y de la salida, y diagnósticos (docs/report-format.md)
cargo run -p hmflow-cli -- report bomba.hmf --out bomba.json

# Ver las fórmulas disponibles
cargo run -p hmflow-cli -- formulas

# Evaluar una expresión con unidades
cargo run -p hmflow-cli -- eval "Q/(pi*d2*b2)" --var "Q=120 m3/h" --var "d2=0.3 m" --var "b2=0.02 m"
```

## Estructura

| Crate | Contenido |
|---|---|
| `hm-core` | Diagnósticos, severidad, revisión |
| `hm-units` | Dimensiones, unidades, cantidades |
| `hm-math` | Interpolación, raíces, utilidades |
| `hm-formula` | AST, parser, análisis dimensional, evaluación, packs |
| `hm-design` | Diseño preliminar, voluta y curvas end-suction |
| `hm-project` | Formato `.hmf`, parámetros, validación |
| `hmflow-cli` | CLI |
| `xtask` | Puertas de calidad |

## Licencia

Apache-2.0 OR MIT. Sin CLA: se usa DCO.

## Agradecimiento

Desarrollado con el apoyo de **Hidromac** y validado en su banco de pruebas.
Hidromac no controla el roadmap del proyecto ni responde por sus resultados.

Ver [NOTICE](NOTICE) y [ATTRIBUTION.md](ATTRIBUTION.md).
