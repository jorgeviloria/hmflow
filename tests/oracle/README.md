# Oráculo geométrico

Esta carpeta contendrá los casos de identidad geométrica a partir de la Parte 2,
cuando exista el kernel.

## Formato previsto

```
tests/oracle/<caso>/
├── recipe.json      # receta propia: parámetros, features y supuestos
├── expected.json    # valores esperados con su fuente
└── reference.stl    # opcional, para distancia de Hausdorff
```

## Campos de `expected.json`

| Campo | Contenido |
|---|---|
| `source` | `analytic`, `freecad <versión>`, `benchmark <id>` o `test <id>` |
| `bodies` | Número de cuerpos |
| `volume` | Volumen en m³ |
| `area` | Área en m² |
| `center_of_mass` | Centroide en m |
| `bounding_box` | Caja envolvente en m |
| `faces` | Número de caras, cuando la geometría lo fija |
| `tolerance` | Tolerancia relativa, por defecto 1e-3 |

## Reglas

- Ningún valor esperado procede de un producto comercial.
- Cada pieza reconstruida se exporta a STEP, se valida y se vuelve a leer.
- Si dos kernels construyen formas distintas por convención, se comparan solo
  volumen y área, y se documenta la diferencia.
- Los datos crudos de ensayo viven en `oracle/`, fuera del repositorio.

## Estado

Pendiente de la Parte 2. La Parte 0 valida fórmulas en `tests/cases/`.
