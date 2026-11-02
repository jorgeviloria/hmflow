# HidroMac Flow (HMFlow)

Suite open source de diseño de bombas centrífugas, escrita en Rust.

> Estado: **Parte 0 — cimientos** y primer motor de diseño end-suction. El
> repositorio compila, hay 23 fórmulas auditadas (4 piloto, 19 con referencia
> de ejemplo resuelto; sin validación de banco) y `design` genera una geometría
> preliminar. Todavía no hay CAD ni interfaz gráfica.

## Qué es

Una herramienta para diseñar bombas centrífugas con las fórmulas a la vista:
cada número se puede abrir, leer, sustituir y auditar.

Alcance: end-suction, inline, multietapa ring-section, carcasa partida con
doble aspiración y barrel. Solo bombas centrífugas.

## Qué no es todavía

- No hay validación de banco todavía: es lo que cierra la Parte 0.
- No hay curvas H(Q) ni NPSHr todavía.
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

# Diseñar una bomba end-suction
cargo run -p hmflow-cli -- design --q "120 m3/h" --h "45 m" --n "2900 rpm" --out bomba.hmf

# Validarlo
cargo run -p hmflow-cli -- check bomba.hmf

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
| `hm-design` | Diseño preliminar end-suction OH1/OH2 |
| `hm-project` | Formato `.hmf`, parámetros, validación |
| `hmflow-cli` | CLI |
| `xtask` | Puertas de calidad |

## Licencia

Apache-2.0 OR MIT. Sin CLA: se usa DCO.

## Agradecimiento

Desarrollado con el apoyo de **Hidromac** y validado en su banco de pruebas.
Hidromac no controla el roadmap del proyecto ni responde por sus resultados.

Ver [NOTICE](NOTICE) y [ATTRIBUTION.md](ATTRIBUTION.md).
