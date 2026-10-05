// The model's exact topology (#21). See topology.h.
//
// Format: one JSON object, millimetres throughout (OCCT's readers convert).
//
//   {"format": "stepv-topology", "version": 1, "units": "mm",
//    "tree": [node],                 roots, in file order
//      node: {"name": s, "children": [node]}   an assembly
//          | {"name": s, "part": i}            a leaf: parts[i]
//    "parts": [{"name": s, "prototype": k,
//               "transform": [12 x f64]}],     row-major 3x4: prototype coords
//                                              -> model coords. parts[i] IS
//                                              the mesh's part i.
//    "prototypes": [{
//       "area": f, "volume": f | null,        volume: solids only
//       "bbox": [6 x f64] | null,
//       "faces": [{                           faces[j] IS the mesh's face id j
//          "surface": "plane" | "cylinder" | "cone" | "sphere" | "torus"
//                   | "bspline" | "bezier" | "revolution" | "extrusion"
//                   | "offset" | "other",
//          "area": f, "edges": [edge index],
//          plane:    "origin": p, "normal": d      (outward: face orientation applied)
//          cylinder: "origin": p, "axis": d, "radius": f
//          cone:     "origin": p, "axis": d, "radius": f, "semi_angle_deg": f
//          sphere:   "center": p, "radius": f
//          torus:    "origin": p, "axis": d, "major_radius": f, "minor_radius": f}],
//       "edges": [{
//          "curve": "line" | "circle" | "ellipse" | "hyperbola" | "parabola"
//                 | "bezier" | "bspline" | "offset" | "other",
//          "length": f, "vertices": [a, b],    indices (null: none); a == b
//                                              on a closed edge
//          circle:  "center": p, "normal": d, "radius": f
//          ellipse: "center": p, "normal": d, "major_radius": f, "minor_radius": f}],
//       "vertices": [p]}]}
//
// p and d are [x, y, z]. Everything is in the PROTOTYPE's coordinates, like
// the mesh before placement: apply the part's transform to place it.
// Degenerate edges (a cone's apex, a sphere's poles) are skipped.

#include "topology.h"

#include "json.h"

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepBndLib.hxx>
#include <BRepGProp.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <GCPnts_AbscissaPoint.hxx>
#include <GProp_GProps.hxx>
#include <OSD_Parallel.hxx>
#include <Standard_Failure.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopoDS.hxx>
#include <gp_Circ.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Elips.hxx>
#include <gp_Pln.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>

#include <cmath>
#include <fstream>
#include <map>
#include <sstream>

namespace stepv {
namespace {

// Every number written goes through this: NaN or infinity would make the
// file invalid JSON, and one bad face cost the whole topology.
double num(double v) { return std::isfinite(v) ? v : 0.0; }

void put(std::ostream& o, const gp_XYZ& v) {
    o << '[' << num(v.X()) << ',' << num(v.Y()) << ',' << num(v.Z()) << ']';
}
void put(std::ostream& o, const char* key, const gp_Pnt& p) {
    o << ",\"" << key << "\":";
    put(o, p.XYZ());
}
void put(std::ostream& o, const char* key, const gp_Dir& d) {
    o << ",\"" << key << "\":";
    put(o, d.XYZ());
}
void put(std::ostream& o, const char* key, double v) {
    o << ",\"" << key << "\":" << num(v);
}

// Returns the face's area, which the prototype's is the sum of.
double face_json(std::ostream& o, const TopoDS_Face& face, const TopTools_IndexedMapOfShape& edges) {
    BRepAdaptor_Surface s(face);
    o << "{\"surface\":";
    switch (s.GetType()) {
    case GeomAbs_Plane: {
        // The surface normal is X x Y, which is Axis() only for right-handed
        // axes; the face's orientation flips it again.
        const gp_Pln pl = s.Plane();
        const gp_Dir n = outward_normal(pl, face);
        o << "\"plane\"";
        put(o, "origin", pl.Location());
        put(o, "normal", n);
        break;
    }
    case GeomAbs_Cylinder: {
        const gp_Cylinder c = s.Cylinder();
        o << "\"cylinder\"";
        put(o, "origin", c.Location());
        put(o, "axis", c.Axis().Direction());
        put(o, "radius", c.Radius());
        break;
    }
    case GeomAbs_Cone: {
        const gp_Cone c = s.Cone();
        o << "\"cone\"";
        put(o, "origin", c.Location());
        put(o, "axis", c.Axis().Direction());
        put(o, "radius", c.RefRadius());
        put(o, "semi_angle_deg", c.SemiAngle() * 180.0 / M_PI);
        break;
    }
    case GeomAbs_Sphere: {
        const gp_Sphere c = s.Sphere();
        o << "\"sphere\"";
        put(o, "center", c.Location());
        put(o, "radius", c.Radius());
        break;
    }
    case GeomAbs_Torus: {
        const gp_Torus c = s.Torus();
        o << "\"torus\"";
        put(o, "origin", c.Location());
        put(o, "axis", c.Axis().Direction());
        put(o, "major_radius", c.MajorRadius());
        put(o, "minor_radius", c.MinorRadius());
        break;
    }
    case GeomAbs_BSplineSurface: o << "\"bspline\""; break;
    case GeomAbs_BezierSurface: o << "\"bezier\""; break;
    case GeomAbs_SurfaceOfRevolution: o << "\"revolution\""; break;
    case GeomAbs_SurfaceOfExtrusion: o << "\"extrusion\""; break;
    case GeomAbs_OffsetSurface: o << "\"offset\""; break;
    default: o << "\"other\""; break;
    }
    GProp_GProps props;
    BRepGProp::SurfaceProperties(face, props);
    const double area = props.Mass();
    put(o, "area", area);
    o << ",\"edges\":[";
    bool first = true;
    for (TopExp_Explorer ex(face, TopAbs_EDGE); ex.More(); ex.Next()) {
        const int i = edges.FindIndex(ex.Current());
        if (i == 0 || BRep_Tool::Degenerated(TopoDS::Edge(ex.Current()))) continue;
        o << (first ? "" : ",") << i - 1;
        first = false;
    }
    o << "]}";
    return area;
}

void edge_json(std::ostream& o, const TopoDS_Edge& edge, const TopTools_IndexedMapOfShape& vertices) {
    BRepAdaptor_Curve c(edge);
    o << "{\"curve\":";
    switch (c.GetType()) {
    case GeomAbs_Line: o << "\"line\""; break;
    case GeomAbs_Circle: {
        const gp_Circ k = c.Circle();
        o << "\"circle\"";
        put(o, "center", k.Location());
        put(o, "normal", k.Axis().Direction());
        put(o, "radius", k.Radius());
        break;
    }
    case GeomAbs_Ellipse: {
        const gp_Elips k = c.Ellipse();
        o << "\"ellipse\"";
        put(o, "center", k.Location());
        put(o, "normal", k.Axis().Direction());
        put(o, "major_radius", k.MajorRadius());
        put(o, "minor_radius", k.MinorRadius());
        break;
    }
    case GeomAbs_Hyperbola: o << "\"hyperbola\""; break;
    case GeomAbs_Parabola: o << "\"parabola\""; break;
    case GeomAbs_BezierCurve: o << "\"bezier\""; break;
    case GeomAbs_BSplineCurve: o << "\"bspline\""; break;
    case GeomAbs_OffsetCurve: o << "\"offset\""; break;
    default: o << "\"other\""; break;
    }
    // Lines and circles in closed form: on a corpus of machined parts they
    // are most edges, and the general integration is the slow path.
    const double span = c.LastParameter() - c.FirstParameter();
    const double length = c.GetType() == GeomAbs_Line     ? span
                          : c.GetType() == GeomAbs_Circle ? c.Circle().Radius() * span
                                                          : GCPnts_AbscissaPoint::Length(c);
    put(o, "length", length);
    TopoDS_Vertex a, b;
    TopExp::Vertices(edge, a, b);
    auto index = [&](const TopoDS_Vertex& v) {
        return v.IsNull() ? std::string("null") : std::to_string(vertices.FindIndex(v) - 1);
    };
    o << ",\"vertices\":[" << index(a) << ',' << index(b) << "]}";
}

}  // namespace

gp_Dir outward_normal(const gp_Pln& plane, const TopoDS_Face& face) {
    gp_Dir n = plane.Axis().Direction();
    if (!plane.Position().Direct()) n.Reverse();
    if (face.Orientation() == TopAbs_REVERSED) n.Reverse();
    return n;
}

TopTools_IndexedMapOfShape topology_edges(const TopoDS_Shape& shape) {
    TopTools_IndexedMapOfShape all, edges;
    TopExp::MapShapes(shape, TopAbs_EDGE, all);
    for (int i = 1; i <= all.Extent(); ++i)
        if (!BRep_Tool::Degenerated(TopoDS::Edge(all(i)))) edges.Add(all(i));
    return edges;
}

namespace {

std::string prototype_json(const TopoDS_Shape& shape) {
    std::ostringstream o, faces;
    o.precision(17);
    faces.precision(17);
    // Index maps: the same edge or vertex shared by two faces is one entry.
    TopTools_IndexedMapOfShape vertices;
    TopExp::MapShapes(shape, TopAbs_VERTEX, vertices);
    const TopTools_IndexedMapOfShape edges = topology_edges(shape);

    // The faces in the order the mesh numbers them: TopExp_Explorer over the
    // prototype (stepv-occt-core.cpp, prototype_geometry).
    double area = 0;
    bool first = true;
    for (TopExp_Explorer ex(shape, TopAbs_FACE); ex.More(); ex.Next()) {
        faces << (first ? "" : ",");
        area += face_json(faces, TopoDS::Face(ex.Current()), edges);
        first = false;
    }
    o << "{\"area\":" << num(area) << ",\"volume\":";
    if (TopExp_Explorer(shape, TopAbs_SOLID).More()) {
        // Closed shells only: an open one beside a solid has no volume.
        GProp_GProps vol;
        BRepGProp::VolumeProperties(shape, vol, Standard_True);
        o << num(vol.Mass());
    } else {
        o << "null";
    }
    Bnd_Box box;
    BRepBndLib::Add(shape, box, false);
    o << ",\"bbox\":";
    if (box.IsVoid()) {
        o << "null";
    } else {
        double x0, y0, z0, x1, y1, z1;
        box.Get(x0, y0, z0, x1, y1, z1);
        o << '[' << num(x0) << ',' << num(y0) << ',' << num(z0) << ',' << num(x1) << ','
          << num(y1) << ',' << num(z1) << ']';
    }
    o << ",\"faces\":[" << faces.str() << "],\"edges\":[";
    for (int i = 1; i <= edges.Extent(); ++i) {
        o << (i > 1 ? "," : "");
        edge_json(o, TopoDS::Edge(edges(i)), vertices);
    }
    o << "],\"vertices\":[";
    for (int i = 1; i <= vertices.Extent(); ++i) {
        o << (i > 1 ? "," : "");
        put(o, BRep_Tool::Pnt(TopoDS::Vertex(vertices(i))).XYZ());
    }
    o << "]}";
    return o.str();
}

void node_json(std::ostream& o, const std::vector<TopoNode>& tree,
               const std::multimap<int, int>& children, int i) {
    o << "{\"name\":\"" << json_escape(tree[i].name) << '"';
    if (tree[i].part >= 0) {
        o << ",\"part\":" << tree[i].part << '}';
        return;
    }
    o << ",\"children\":[";
    bool first = true;
    for (auto [it, end] = children.equal_range(i); it != end; ++it) {
        o << (first ? "" : ",");
        node_json(o, tree, children, it->second);
        first = false;
    }
    o << "]}";
}

}  // namespace

std::string write_topology(const std::string& path, const std::vector<TopoNode>& tree,
                           const std::vector<TopoPart>& parts,
                           const std::vector<TopoDS_Shape>& prototypes) {
    std::ostringstream o;
    o.precision(17);
    o << "{\"format\":\"stepv-topology\",\"version\":1,\"units\":\"mm\",\"tree\":[";
    // multimap keeps insertion order among equal keys: children in file order.
    std::multimap<int, int> children;
    for (std::size_t i = 0; i < tree.size(); ++i) children.emplace(tree[i].parent, static_cast<int>(i));
    bool first = true;
    for (auto [it, end] = children.equal_range(-1); it != end; ++it) {
        o << (first ? "" : ",");
        node_json(o, tree, children, it->second);
        first = false;
    }
    o << "],\"parts\":[";
    for (std::size_t i = 0; i < parts.size(); ++i) {
        const gp_Trsf& t = parts[i].placement;
        o << (i ? "," : "") << "{\"name\":\"" << json_escape(parts[i].name)
          << "\",\"prototype\":" << parts[i].prototype << ",\"transform\":[";
        for (int r = 1; r <= 3; ++r)
            for (int c = 1; c <= 4; ++c) o << (r + c > 2 ? "," : "") << num(t.Value(r, c));
        o << "]}";
    }
    // Prototypes are independent: in parallel, each into its own string.
    std::vector<std::string> json(prototypes.size()), errors(prototypes.size());
    OSD_Parallel::For(0, static_cast<int>(prototypes.size()), [&](int i) {
        try {
            json[i] = prototype_json(prototypes[i]);
        } catch (const Standard_Failure& e) {
            errors[i] = e.GetMessageString();
        } catch (const std::exception& e) {
            errors[i] = e.what();
        }
    });
    o << "],\"prototypes\":[";
    for (std::size_t i = 0; i < prototypes.size(); ++i) {
        if (!errors[i].empty())
            return "topology of prototype " + std::to_string(i) + ": " + errors[i];
        o << (i ? "," : "") << json[i];
    }
    o << "]}\n";
    std::ofstream f(path, std::ios::binary | std::ios::trunc);
    if (!f) return "cannot open topology output: " + path;
    const std::string text = o.str();
    f.write(text.data(), static_cast<std::streamsize>(text.size()));
    f.close();
    return f ? "" : "topology output write failed";
}

}  // namespace stepv
