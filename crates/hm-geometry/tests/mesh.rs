use std::f64::consts::{FRAC_PI_2, PI};

use hm_geometry::{
    CREASE_ANGLE_DEG, GeometryError, Polyline3, REVOLVE_MIN_SEGMENTS, TriangleMesh, grid_surface,
    revolve, revolve_segments,
};

const CYLINDER: [[f64; 2]; 2] = [[0.0, 50.0], [100.0, 50.0]];

fn sub(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn radius(point: [f64; 3]) -> f64 {
    point[0].hypot(point[1])
}

fn close(left: [f64; 3], right: [f64; 3]) {
    let distance = norm(sub(left, right));
    assert!(
        distance < 1e-12,
        "{left:?} frente a {right:?}, distancia {distance}"
    );
}

/// Malla sana: una normal unitaria por vértice, y triángulos con área, con tres vértices
/// distintos y en sentido antihorario vistos desde el lado al que apuntan las normales.
fn assert_sound(mesh: &TriangleMesh) {
    assert_eq!(mesh.vertices.len(), mesh.normals.len());
    for (vertex, normal) in mesh.vertices.iter().zip(&mesh.normals) {
        assert!(
            vertex.iter().all(|value| value.is_finite()),
            "vértice {vertex:?}"
        );
        assert!(
            (norm(*normal) - 1.0).abs() < 1e-9,
            "normal {normal:?} en {vertex:?}"
        );
    }
    for triangle in &mesh.triangles {
        let [a, b, c] = triangle.map(|index| index as usize);
        assert!(
            a != b && b != c && a != c,
            "índices repetidos en {triangle:?}"
        );
        let face = cross(
            sub(mesh.vertices[b], mesh.vertices[a]),
            sub(mesh.vertices[c], mesh.vertices[a]),
        );
        assert!(norm(face) / 2.0 > 1e-12, "triángulo sin área: {triangle:?}");
        for index in [a, b, c] {
            assert!(
                dot(face, mesh.normals[index]) > 0.0,
                "el triángulo {triangle:?} mira al revés que la normal de {index}"
            );
        }
    }
}

fn assert_is_the_cylinder(mesh: &TriangleMesh) {
    assert_sound(mesh);
    assert_eq!(mesh.vertices.len(), 2 * 71);
    assert_eq!(mesh.triangles.len(), 2 * 71);
    for (vertex, normal) in mesh.vertices.iter().zip(&mesh.normals) {
        assert!((radius(*vertex) - 50.0).abs() < 1e-9, "vértice {vertex:?}");
        assert!(normal[2].abs() < 1e-9, "normal {normal:?}");
        let outward = (normal[0] * vertex[0] + normal[1] * vertex[1]) / 50.0;
        assert!(
            (outward - 1.0).abs() < 1e-9,
            "normal {normal:?} en {vertex:?}"
        );
    }
}

#[test]
fn segments_follow_the_tolerance() {
    assert_eq!(revolve_segments(50.0, 0.05), 71);
    assert_eq!(revolve_segments(1.0, 0.5), REVOLVE_MIN_SEGMENTS);
}

#[test]
fn segments_never_drop_below_the_minimum_with_absurd_input() {
    for (r_max, tolerance) in [
        (50.0, 1e6),
        (0.0, 0.05),
        (50.0, 0.0),
        (50.0, f64::NAN),
        (-1.0, 0.05),
    ] {
        assert_eq!(
            revolve_segments(r_max, tolerance),
            REVOLVE_MIN_SEGMENTS,
            "{r_max}, {tolerance}"
        );
    }
}

#[test]
fn a_cylinder_lies_on_its_surface_with_radial_normals() {
    let mesh = revolve(&CYLINDER, 0.05).expect("cilindro");
    assert_is_the_cylinder(&mesh);

    // La flecha de un gajo no pasa de la tolerancia.
    let sag = 50.0 * (1.0 - (PI / 71.0).cos());
    assert!(sag <= 0.05, "{sag}");
    let (low, high) = mesh.bounds().expect("caja");
    assert!(
        low[2].abs() < 1e-12 && (high[2] - 100.0).abs() < 1e-12,
        "{low:?} a {high:?}"
    );
}

#[test]
fn a_pole_makes_a_fan_without_degenerate_triangles() {
    let cone = revolve(&[[0.0, 0.0], [10.0, 10.0]], 0.05).expect("cono");
    assert_sound(&cone);
    let segments = revolve_segments(10.0, 0.05);
    assert_eq!(cone.triangles.len(), segments);
    assert_eq!(cone.vertices.len(), segments + 1);
    let poles = cone
        .vertices
        .iter()
        .filter(|vertex| radius(**vertex) < 1e-9)
        .count();
    assert_eq!(poles, 1);
}

#[test]
fn a_hub_closed_to_the_axis_with_straight_lines_is_sound() {
    // Disco desde el eje, cilindro y disco de vuelta al eje: como el cubo que Jorge cierra a (0,0).
    let hub = revolve(&[[0.0, 0.0], [0.0, 20.0], [30.0, 20.0], [30.0, 0.0]], 0.05).expect("cubo");
    assert_sound(&hub);
    let segments = revolve_segments(20.0, 0.05);
    assert_eq!(hub.triangles.len(), 4 * segments);
    // Las dos tapas miran hacia fuera: −z la de z = 0 y +z la de z = 30.
    for (vertex, normal) in hub.vertices.iter().zip(&hub.normals) {
        if radius(*vertex) < 1e-9 {
            let expected = if vertex[2] < 15.0 { -1.0 } else { 1.0 };
            close(*normal, [0.0, 0.0, expected]);
        }
    }
}

#[test]
fn a_stretch_along_the_axis_makes_no_surface() {
    let mesh = revolve(&[[0.0, 0.0], [5.0, 0.0], [10.0, 10.0]], 0.05).expect("eje y cono");
    assert_sound(&mesh);
    assert_eq!(mesh.triangles.len(), revolve_segments(10.0, 0.05));
}

#[test]
fn repeated_points_do_not_break_the_normals() {
    let mesh = revolve(&[[0.0, 50.0], [0.0, 50.0], [100.0, 50.0]], 0.05).expect("cilindro");
    assert_is_the_cylinder(&mesh);
    assert_eq!(mesh, revolve(&CYLINDER, 0.05).expect("cilindro"));
}

#[test]
fn a_sharp_corner_keeps_its_crease() {
    let mesh = revolve(&[[0.0, 50.0], [100.0, 50.0], [100.0, 80.0]], 0.05).expect("escalón");
    assert_sound(&mesh);
    let segments = revolve_segments(80.0, 0.05);
    assert_eq!(mesh.vertices.len(), 4 * segments);

    let corner: Vec<[f64; 3]> = mesh
        .vertices
        .iter()
        .zip(&mesh.normals)
        .filter(|(vertex, _)| {
            (vertex[2] - 100.0).abs() < 1e-9 && (radius(**vertex) - 50.0).abs() < 1e-9
        })
        .map(|(_, normal)| *normal)
        .collect();
    assert_eq!(corner.len(), 2 * segments);
    let radial = corner
        .iter()
        .filter(|normal| normal[2].abs() < 1e-9)
        .count();
    let axial = corner
        .iter()
        .filter(|normal| (normal[2].abs() - 1.0).abs() < 1e-9)
        .count();
    assert_eq!((radial, axial), (segments, segments));
}

#[test]
fn a_gentle_bend_shares_its_vertices() {
    let bend = (CREASE_ANGLE_DEG - 10.0).to_radians();
    let profile = [
        [0.0, 50.0],
        [100.0, 50.0],
        [100.0 + 100.0 * bend.cos(), 50.0 + 100.0 * bend.sin()],
    ];
    let mesh = revolve(&profile, 0.05).expect("codo suave");
    assert_sound(&mesh);
    let segments = revolve_segments(profile[2][1], 0.05);
    assert_eq!(mesh.vertices.len(), 3 * segments);

    // En el codo la normal sale de la derivada del perfil: queda entre las de los dos tramos.
    let half = bend / 2.0;
    for (vertex, normal) in mesh.vertices.iter().zip(&mesh.normals) {
        if (vertex[2] - 100.0).abs() < 1e-9 {
            assert!(
                normal[2] < 0.0 && normal[2] > -bend.sin(),
                "normal {normal:?}"
            );
            assert!((normal[2] + half.sin()).abs() < 1e-9, "normal {normal:?}");
        }
    }
}

#[test]
fn bad_input_is_an_error() {
    assert_eq!(revolve(&[], 0.05), Err(GeometryError::TooFewSamples));
    assert_eq!(
        revolve(&[[0.0, 50.0]], 0.05),
        Err(GeometryError::TooFewSamples)
    );
    assert_eq!(
        revolve(&[[0.0, 50.0], [100.0, -1.0]], 0.05),
        Err(GeometryError::NegativeRadius)
    );
    for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            revolve(&CYLINDER, tolerance),
            Err(GeometryError::BadTolerance),
            "{tolerance}"
        );
    }
    // Todo el perfil sobre el eje, o un solo punto repetido: no hay superficie.
    assert_eq!(
        revolve(&[[0.0, 0.0], [100.0, 0.0]], 0.05),
        Err(GeometryError::Degenerate)
    );
    assert_eq!(
        revolve(&[[0.0, 50.0], [0.0, 50.0]], 0.05),
        Err(GeometryError::Degenerate)
    );
    assert_eq!(
        revolve(&[[0.0, 50.0], [f64::NAN, 50.0]], 0.05),
        Err(GeometryError::Degenerate)
    );
}

#[test]
fn a_tolerance_too_fine_for_the_indices_is_an_error() {
    assert_eq!(revolve(&CYLINDER, 1e-300), Err(GeometryError::BadTolerance));
}

#[test]
fn a_flat_grid_has_two_triangles_and_one_normal() {
    let points = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let mesh = grid_surface(2, 2, &points).expect("rejilla");
    assert_sound(&mesh);
    assert_eq!(mesh.vertices, points);
    assert_eq!(mesh.triangles.len(), 2);
    for normal in &mesh.normals {
        close(*normal, [0.0, 0.0, 1.0]);
    }
}

#[test]
fn a_curved_grid_takes_its_normals_from_the_grid_differences() {
    // Trozo de cilindro de radio 20: columnas a lo largo del ángulo, filas a lo largo de z.
    let (rows, cols) = (3, 5);
    let mut points = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let angle = 0.1 * col as f64;
            points.push([20.0 * angle.cos(), 20.0 * angle.sin(), 5.0 * row as f64]);
        }
    }
    let mesh = grid_surface(rows, cols, &points).expect("rejilla");
    assert_sound(&mesh);
    assert_eq!(mesh.triangles.len(), 2 * (rows - 1) * (cols - 1));
    for row in 0..rows {
        for col in 1..cols - 1 {
            let index = row * cols + col;
            let vertex = mesh.vertices[index];
            close(
                mesh.normals[index],
                [vertex[0] / 20.0, vertex[1] / 20.0, 0.0],
            );
        }
    }
}

#[test]
fn a_grid_that_does_not_match_is_an_error() {
    let points = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    assert_eq!(
        grid_surface(2, 3, &points),
        Err(GeometryError::GridMismatch)
    );
    assert_eq!(
        grid_surface(usize::MAX, 2, &points),
        Err(GeometryError::GridMismatch)
    );
    assert_eq!(
        grid_surface(1, 4, &points),
        Err(GeometryError::TooFewSamples)
    );
    assert_eq!(
        grid_surface(4, 1, &points),
        Err(GeometryError::TooFewSamples)
    );
}

#[test]
fn a_grid_without_area_is_an_error() {
    let points = [[1.0, 2.0, 3.0]; 4];
    assert_eq!(grid_surface(2, 2, &points), Err(GeometryError::Degenerate));
    let mut broken = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    broken[3][2] = f64::NAN;
    assert_eq!(grid_surface(2, 2, &broken), Err(GeometryError::Degenerate));
}

#[test]
fn rotation_about_z_turns_vertices_and_normals() {
    let mesh = TriangleMesh {
        vertices: vec![[1.0, 0.0, 5.0], [0.0, 1.0, 5.0], [0.0, 0.0, 6.0]],
        normals: vec![[1.0, 0.0, 0.0]; 3],
        triangles: vec![[0, 1, 2]],
    };
    let turned = mesh.rotated_z(FRAC_PI_2);
    close(turned.vertices[0], [0.0, 1.0, 5.0]);
    close(turned.vertices[1], [-1.0, 0.0, 5.0]);
    close(turned.vertices[2], [0.0, 0.0, 6.0]);
    for normal in &turned.normals {
        close(*normal, [0.0, 1.0, 0.0]);
    }
    assert_eq!(turned.triangles, mesh.triangles);

    let line = Polyline3 {
        points: vec![[1.0, 0.0, 5.0], [2.0, 0.0, 7.0]],
        closed: true,
    };
    let turned = line.rotated_z(FRAC_PI_2);
    close(turned.points[0], [0.0, 1.0, 5.0]);
    close(turned.points[1], [0.0, 2.0, 7.0]);
    assert!(turned.closed);
}

#[test]
fn bounds_enclose_the_vertices_and_an_empty_mesh_has_none() {
    assert_eq!(TriangleMesh::default().bounds(), None);
    let mesh = TriangleMesh {
        vertices: vec![[1.0, -2.0, 5.0], [-3.0, 4.0, 0.5]],
        normals: vec![[0.0, 0.0, 1.0]; 2],
        triangles: Vec::new(),
    };
    assert_eq!(mesh.bounds(), Some(([-3.0, -2.0, 0.5], [1.0, 4.0, 5.0])));
}

// ------------------------------------------------- mallas cerradas y su volumen

/// Una cara plana de una caja como rejilla de 2×2 puntos: `origin`, y los
/// pasos a lo largo de las columnas y de las filas. Su normal, hacia fuera,
/// es `columnas × filas`.
fn quad(origin: [f64; 3], columns: [f64; 3], rows: [f64; 3]) -> TriangleMesh {
    let at = |c: f64, r: f64| -> [f64; 3] {
        std::array::from_fn(|axis| origin[axis] + c * columns[axis] + r * rows[axis])
    };
    grid_surface(
        2,
        2,
        &[at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0)],
    )
    .unwrap()
}

/// Las seis caras de la caja [0, 2] × [0, 3] × [0, 4], con la normal hacia
/// fuera.
fn box_faces() -> Vec<TriangleMesh> {
    let (x, y, z) = ([2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]);
    vec![
        quad([0.0, 0.0, 4.0], x, y), // z = 4, hacia +z
        quad([0.0, 0.0, 0.0], y, x), // z = 0, hacia −z
        quad([2.0, 0.0, 0.0], y, z), // x = 2, hacia +x
        quad([0.0, 0.0, 0.0], z, y), // x = 0, hacia −x
        quad([0.0, 3.0, 0.0], z, x), // y = 3, hacia +y
        quad([0.0, 0.0, 0.0], x, z), // y = 0, hacia −y
    ]
}

#[test]
fn a_box_of_six_grids_is_closed_and_has_its_volume() {
    let closed = TriangleMesh::merged(&box_faces());
    assert_eq!(closed.vertices.len(), 24);
    assert_eq!(closed.triangles.len(), 12);
    assert_eq!(closed.open_edges(1e-9), 0);
    assert!((closed.signed_volume() - 24.0).abs() < 1e-9);
}

#[test]
fn a_missing_face_leaves_open_edges() {
    let mut faces = box_faces();
    faces.pop();
    let open = TriangleMesh::merged(&faces);
    assert_eq!(open.open_edges(1e-9), 4);
}

#[test]
fn a_flipped_face_is_not_closed() {
    let mut faces = box_faces();
    for triangle in &mut faces[0].triangles {
        triangle.swap(1, 2);
    }
    let flipped = TriangleMesh::merged(&faces);
    assert!(flipped.open_edges(1e-9) > 0);
    // Y su volumen ya no es el de la caja.
    assert!((flipped.signed_volume() - 24.0).abs() > 1e-3);
}

#[test]
fn merged_keeps_every_triangle_and_shifts_the_indices() {
    let faces = box_faces();
    let merged = TriangleMesh::merged(&faces);
    let total: usize = faces.iter().map(|mesh| mesh.triangles.len()).sum();
    assert_eq!(merged.triangles.len(), total);
    assert_eq!(merged.normals.len(), merged.vertices.len());
    let mut shift = 0u32;
    let mut at = 0;
    for mesh in &faces {
        for triangle in &mesh.triangles {
            let expected = triangle.map(|index| index + shift);
            assert_eq!(merged.triangles[at], expected);
            at += 1;
        }
        for (i, vertex) in mesh.vertices.iter().enumerate() {
            assert_eq!(merged.vertices[shift as usize + i], *vertex);
            assert_eq!(merged.normals[shift as usize + i], mesh.normals[i]);
        }
        shift += mesh.vertices.len() as u32;
    }
    assert_eq!(TriangleMesh::merged(&[]), TriangleMesh::default());
}

#[test]
fn vertices_closer_than_the_weld_count_as_one() {
    // Dos caras con un vértice casi coincidente se sueldan con una tolerancia
    // mayor que la separación, y no con una menor.
    let mut faces = box_faces();
    faces[0].vertices[0][0] += 1e-7;
    let merged = TriangleMesh::merged(&faces);
    assert_eq!(merged.open_edges(1e-6), 0);
    assert!(merged.open_edges(1e-9) > 0);
}
