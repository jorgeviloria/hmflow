# Attribution

Este archivo declara el origen de todo lo que no es código propio.

## Fuentes técnicas

| Fuente | Uso | Licencia / condición |
|---|---|---|
| Gülich, *Centrifugal Pumps*, 4.ª ed., Springer, 2020 | Ecuaciones y correlaciones de diseño | Obra con copyright; se citan ecuaciones, no se reproduce texto |
| Stepanoff, *Centrifugal and Axial Flow Pumps*, 1957 | Correlaciones clásicas | Obra con copyright; se citan ecuaciones |
| Pfleiderer/Petermann, *Strömungsmaschinen* | Teoría de turbomáquinas | Obra con copyright; se citan ecuaciones |
| Lobanoff/Ross, *Centrifugal Pumps: Design and Application* | Diseño de volutas y carcasas | Obra con copyright; se citan ecuaciones |
| Japikse/Marscher/Furst, *Centrifugal Pump Design and Performance* | Prestaciones y pérdidas | Obra con copyright; se citan ecuaciones |
| ISO 9906, API 610/ISO 13709, ISO 5199, EN 733, ISO 2858, HI 14.6 | Criterios y tolerancias | Normas con copyright; se citan cláusulas, no se redistribuyen |
| Banco de ensayos de Hidromac | Curvas medidas | Datos cedidos por Hidromac; solo casos normalizados y anonimizados |

## Componentes de software

| Componente | Licencia | Uso |
|---|---|---|
| Rust standard library | MIT OR Apache-2.0 | Base |
| serde | MIT OR Apache-2.0 | Serialización |
| toml | MIT OR Apache-2.0 | Formato de proyecto |
| clap | MIT OR Apache-2.0 | CLI |
| thiserror | MIT OR Apache-2.0 | Errores |
| OpenCASCADE 7.9 | LGPL-2.1 con excepción | Kernel CAD, biblioteca del sistema, no vendida en el repo. Hoy: cilindro a STL |
| netgen | LGPL-2.1 | Mallado (a partir de CFD-0) |
| CoolProp | MIT | Propiedades de fluidos (a partir de la Parte 1) |
| iced / iced_aw | MIT | Interfaz (a partir de la Parte 1) |
| wgpu | MIT OR Apache-2.0 | Render |
| Typst | Apache-2.0 | Informes |

## Assets

Todo asset gráfico, icono o tipografía debe ser original o tener licencia
abierta compatible, y debe aparecer en la tabla anterior antes de entrar al
repositorio. La puerta `cargo xtask assets` lo comprueba.

## Lo que este repositorio no contiene

- Manuales, ejemplos, curvas, iconos o textos de productos comerciales.
- Código decompilado o reutilizado de terceros.
- Datos de clientes, precios o condiciones comerciales.
- Formatos propietarios como formato principal.
