# Contribuir

Gracias por querer ayudar. Este proyecto es open source de verdad: no hay
versión cerrada, no hay CLA y no hay funciones de pago.

## Reglas duras

1. **Clean-room.** No copies código, iconos, textos, plantillas, curvas ni datos
   de otro producto. No uses herramientas comerciales para extraer información
   interna.
2. **Declara la fuente.** Toda fórmula, dato o geometría debe indicar de dónde
   sale: libro con edición y página, norma con cláusula, benchmark público o
   ensayo propio.
3. **Sin contenido de terceros.** No subas manuales, ejemplos, catálogos,
   capturas de productos comerciales ni datos de clientes.
4. **DCO, no CLA.** Cada commit lleva `Signed-off-by: Tu Nombre <tu@correo>`.
   Se añade con `git commit -s`.
5. **Licencia compatible.** Solo dependencias MIT, Apache-2.0, BSD, ISC, Zlib,
   MPL-2.0 o equivalentes. Nada de GPL, LGPL o AGPL en código propio.

## Cómo enviar un cambio

```sh
git checkout -b feat/mi-cambio
cargo xtask ci
git commit -s -m "feat: describir el cambio"
```

- Un tema por PR.
- Tests para todo cambio de comportamiento.
- Si añades una fórmula: fuente, rango, test y fila en
  `docs/validation/matrix.md`.
- Si añades un asset: fila en `ATTRIBUTION.md`.

## Puertas de calidad

```sh
cargo xtask ci           # fmt + clippy + tests
cargo xtask provenance   # fórmulas con fuente, rango y test
cargo xtask licences     # licencias y ficheros legales
cargo xtask assets       # assets declarados
```

Un PR no se fusiona si falla cualquiera de ellas.

## Estilo

- Rust 2024, `rustfmt` por defecto.
- Sin `unwrap`, `expect`, `panic` ni `unsafe` fuera de tests y FFI.
- Errores con `Result` y diagnósticos con código estable.
- Comentarios y documentación en español; identificadores en inglés.
- Mensajes de commit en español, con prefijo (`feat`, `fix`, `docs`, `test`,
  `refactor`, `chore`).

## Dónde ayudar

- Fórmulas con fuente pública y test.
- Casos de validación con geometría y curva medidas.
- Documentación de teoría.
- Traducciones.
- Revisión de unidades y tolerancias.

## Código de conducta

Al participar aceptas el [código de conducta](CODE_OF_CONDUCT.md).
