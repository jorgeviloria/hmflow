#include "hm-cad-occt/src/bridge.rs.h"

#include <cmath>
#include <stdexcept>
#include <string>

#include <BRepCheck_Analyzer.hxx>
#include <BRepGProp.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRep_Tool.hxx>
#include <GProp_GProps.hxx>
#include <Standard_Failure.hxx>
#include <StlAPI_Writer.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Face.hxx>

namespace {

TopoDS_Shape make_cylinder(double radius_m, double height_m) {
    if (!(radius_m > 0.0) || !std::isfinite(radius_m) || !(height_m > 0.0) ||
        !std::isfinite(height_m)) {
        throw std::invalid_argument("el radio y la altura tienen que ser finitos y mayores que cero");
    }

    BRepPrimAPI_MakeCylinder maker(radius_m, height_m);
    maker.Build();
    if (!maker.IsDone()) {
        throw std::runtime_error("OCCT no cerró el cilindro");
    }

    TopoDS_Shape shape = maker.Shape();
    BRepCheck_Analyzer analyzer(shape);
    if (!analyzer.IsValid()) {
        throw std::runtime_error("el cilindro de OCCT no pasó BRepCheck");
    }
    return shape;
}

[[noreturn]] void rethrow_occt(const Standard_Failure& error) {
    const char* message = error.GetMessageString();
    throw std::runtime_error(message == nullptr ? "fallo de OCCT" : message);
}

std::uint64_t triangle_count(const TopoDS_Shape& shape) {
    std::uint64_t count = 0;
    for (TopExp_Explorer faces(shape, TopAbs_FACE); faces.More(); faces.Next()) {
        TopLoc_Location location;
        const TopoDS_Face face = TopoDS::Face(faces.Current());
        Handle(Poly_Triangulation) triangulation = BRep_Tool::Triangulation(face, location);
        if (!triangulation.IsNull()) {
            count += static_cast<std::uint64_t>(triangulation->NbTriangles());
        }
    }
    return count;
}

}  // namespace

double cylinder_volume_m3(double radius_m, double height_m) {
    try {
        const TopoDS_Shape shape = make_cylinder(radius_m, height_m);
        GProp_GProps props;
        BRepGProp::VolumeProperties(shape, props);
        return props.Mass();
    } catch (const Standard_Failure& error) {
        rethrow_occt(error);
    }
}

std::uint64_t write_cylinder_stl(
    double radius_m,
    double height_m,
    double deflection_m,
    rust::Str path) {
    try {
        if (!(deflection_m > 0.0) || !std::isfinite(deflection_m)) {
            throw std::invalid_argument(
                "la deflexión de la malla tiene que ser finita y mayor que cero");
        }

        TopoDS_Shape shape = make_cylinder(radius_m, height_m);
        BRepMesh_IncrementalMesh mesh(shape, deflection_m, Standard_False, 0.5, Standard_False);
        if (!mesh.IsDone()) {
            throw std::runtime_error("OCCT no teseló el cilindro");
        }

        const std::uint64_t triangles = triangle_count(shape);
        if (triangles == 0) {
            throw std::runtime_error("la malla del cilindro no tiene triángulos");
        }

        const std::string file(path);
        StlAPI_Writer writer;
        writer.ASCIIMode() = Standard_True;
        if (!writer.Write(shape, file.c_str())) {
            throw std::runtime_error("OCCT no escribió el STL");
        }
        return triangles;
    } catch (const Standard_Failure& error) {
        rethrow_occt(error);
    }
}
