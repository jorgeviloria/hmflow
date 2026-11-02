# Seguridad

## Versiones soportadas

Solo la última release recibe correcciones de seguridad.

## Reportar una vulnerabilidad

No abras un issue público. Escribe a los maintainers con:

- versión afectada;
- descripción del problema;
- pasos para reproducirlo;
- impacto estimado.

Plazo de respuesta: 72 horas. Plazo de corrección: 90 días para problemas
graves.

## Alcance

- Parsers de proyecto (`.hmf`), CSV y futuros importadores STEP/DXF.
- Desbordamientos, `panic` o consumo de memoria con entrada hostil.
- Ejecución de código a través de packs de fórmulas: los packs son datos, nunca
  código.

## Fuera de alcance

- Resultados de ingeniería incorrectos por fórmulas mal parametrizadas. Eso es
  un bug funcional, no de seguridad: abre un issue normal.
- Problemas en dependencias de terceros: repórtalos aguas arriba y avísanos.
