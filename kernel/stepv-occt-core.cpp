// stepv-occt core — the Plan B kernel (plan.md §3): native OCCT.
//
// One input file in; a JSON summary and, optionally, the planar mesh buffers
// in a file. Two front doors onto this one implementation:
//
//   - stepv-occt (stepv-occt.cpp): the CLI the Rust side runs as a
//     SUBPROCESS, so a crash takes down a child, not the caller, and the
//     parent enforces time and memory by killing it (Linux, the stepv CLI);
//   - libstepvocct + stepv_occt.h: the same thing IN-PROCESS, for the macOS
//     Quick Look extensions, whose sandbox forbids exec (posix_spawn fails
//     with EPERM). There the extension process itself is the containment:
//     the system runs it apart from Finder and kills it on hang or memory
//     pressure.
//
// Units: OCCT's readers convert to millimetres, so everything emitted here is
// in mm regardless of the file's declared units.
//
// Mesh file format ("STEPVMSH", version 3 or 4, little-endian, no padding):
//
//   magic        8 bytes  "STEPVMSH"
//   version      u32      3, or 4 when B-rep edges were asked for (--edges)
//   bbox         6 x f64  min xyz, max xyz
//   part_count   u32
//   per part:
//     name_len   u32      0 = no name
//     name       name_len bytes, UTF-8
//     has_color  u8
//     rgb        3 x f32  (present, zero when has_color = 0)
//     faces      u32
//     per face:  status u8 (FaceStatus, below), has_color u8, rgb 3 x f32
//                (zero when has_color = 0). The face colour, when present,
//                overrides the part colour: 81% of parts in the S1 corpus
//                carry colour only per face.
//     vertices   u32
//     triangles  u32
//     positions  3 * vertices  x f32
//     normals    3 * vertices  x f32
//     indices    3 * triangles x u32
//     face_ids   triangles     x u32   index into the per-face table
//     segments   u32
//     seg_points 6 * segments  x f32   two xyz endpoints per segment
//     seg_kinds  segments      x u8    LineKind, below
//   version 4 only, still per part (#31):
//     edges      u32      B-rep edges, as polylines
//     edge_ids   edges    x u32   the edge's index in the part's prototype's
//                                 topology edges (topology.h, topology_edges)
//     edge_lens  edges    x u32   points per polyline (>= 2)
//     edge_pts   3 * sum(edge_lens) x f32
// A v3 reader stops before them; Quick Look asks for v3 (no --edges).
//
// FaceStatus records HOW a face's triangles were obtained, so a renderer can
// draw anything short of exact with a warning treatment instead of passing it
// off as the model (plan.md §2, the silent-failure argument):
//   0 Ok          first-pass mesh
//   1 Remeshed    failed first pass; a clean re-mesh of an isolated copy worked
//   2 Healed      needed ShapeFix on the copy, then meshed
//   3 Refined     meshed with deflection relative to the FACE's own size (a
//                 face far smaller than the model, or whose boundary
//                 self-intersects at the model-relative deflection)
//   4 Coarse      meshed only with relaxed deflection/angle; exact, coarse
//   5 Degenerate  zero parametric width or vanishing area: nothing to draw,
//                 and nothing missing either (an exporter's sliver)
//   6 Approx      the mesher gave up; triangles are a UV-grid SAMPLE of the
//                 surface clipped to the face. Shape is right, edges jagged
//   7 Missing     nothing worked; no triangles, only the outline
// The order is severity: a part's worst face is its maximum status.
//
// LineKind: 0 = curve of a part with no faces, in a file with NO faces at
//               all (a sketch): the file's whole content, so draw it;
//           1 = outline of a Missing face;
//           2 = curve of a part with no faces, in a file that ALSO has
//               solids: construction geometry, axes, PMI leaders. Kept,
//               but a renderer should hide it by default.
//
// src/occt.rs is the reader; keep the two in step.

#include "json.h"
#include "stepv_occt.h"
#include "topology.h"

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_Copy.hxx>
#include <BRepClass_FaceClassifier.hxx>
#include <BRepGProp.hxx>
#include <GProp_GProps.hxx>
#include <BRepLProp_SLProps.hxx>
#include <GCPnts_TangentialDeflection.hxx>
#include <NCollection_DataMap.hxx>
#include <ShapeFix_Shape.hxx>
#include <TopTools_ShapeMapHasher.hxx>
#include <TopoDS_Edge.hxx>
#include <gp_Pnt2d.hxx>
#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepTools.hxx>
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <IGESCAFControl_Reader.hxx>
#include <IMeshTools_Parameters.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <Poly_Triangulation.hxx>
#include <Quantity_Color.hxx>
#include <STEPCAFControl_ExternFile.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <Standard_Failure.hxx>
#include <TCollection_AsciiString.hxx>
#include <TDF_Label.hxx>
#include <TDF_LabelSequence.hxx>
#include <TDF_Tool.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <gp_Trsf.hxx>

#include <sys/resource.h>
#include <climits>
#include <cerrno>
#include <cstdlib>
#include <unistd.h>

#include <chrono>
#include <cmath>
#include <array>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <mutex>
#include <new>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

namespace {

constexpr int kExitOk = 0;
constexpr int kExitFailed = 3;


// ── JSON output ─────────────────────────────────────────────────────────────

using stepv::json_escape;

struct Summary {
    std::string format;
    std::string stage = "start";  // the stage reached, or the one that failed
    std::optional<std::string> error;
    std::optional<Bnd_Box> bbox;
    double diagonal = 0.0;
    double linear_abs = 0.0;
    std::size_t parts = 0;
    std::size_t parts_named = 0;
    std::size_t parts_colored = 0;       // part-level colour (own or inherited)
    std::size_t parts_face_colored = 0;  // no part colour, but per-face colours
    std::size_t prototypes = 0;
    std::size_t prototypes_mesh_failed = 0;
    std::size_t faces = 0;
    std::size_t faces_unmeshed = 0;  // failed the FIRST pass; the ladder below then ran
    // Where each first-pass failure ended up on the recovery ladder.
    std::size_t faces_remeshed = 0, faces_healed = 0, faces_refined = 0, faces_coarse = 0,
                faces_degenerate = 0, faces_approx = 0, faces_missing = 0;
    std::size_t sketch_parts = 0;        // faceless parts in a file with no faces
    std::size_t construction_parts = 0;  // faceless parts beside solids
    std::size_t vertices = 0;
    std::size_t triangles = 0;
    std::size_t segments = 0;
    std::size_t edges = 0;  // B-rep edge polylines written (STEPVMSH v4)
    // Multi-file STEP assemblies (#19): the part files the top-level file
    // references, and how many of them could not be read (missing, or
    // outside what a sandbox lets this process open).
    std::size_t external_files = 0, external_missing = 0;
    double t_read_ms = 0, t_transfer_ms = 0, t_mesh_ms = 0, t_extract_ms = 0, t_topology_ms = 0;
};

std::size_t peak_rss_bytes() {
    rusage ru{};
    getrusage(RUSAGE_SELF, &ru);
#ifdef __APPLE__
    return static_cast<std::size_t>(ru.ru_maxrss);  // bytes on macOS
#else
    return static_cast<std::size_t>(ru.ru_maxrss) * 1024;  // KiB on Linux
#endif
}

std::string to_json(const Summary& s) {
    std::ostringstream o;
    o.precision(17);
    o << "{\"ok\":" << (s.error ? "false" : "true");
    o << ",\"stage\":\"" << s.stage << "\"";
    o << ",\"error\":";
    if (s.error) o << '"' << json_escape(*s.error) << '"'; else o << "null";
    o << ",\"format\":\"" << s.format << "\"";
    o << ",\"bbox\":";
    if (s.bbox && !s.bbox->IsVoid()) {
        double x0, y0, z0, x1, y1, z1;
        s.bbox->Get(x0, y0, z0, x1, y1, z1);
        o << '[' << x0 << ',' << y0 << ',' << z0 << ',' << x1 << ',' << y1 << ',' << z1 << ']';
    } else {
        o << "null";
    }
    o << ",\"diagonal\":" << s.diagonal << ",\"linear_abs\":" << s.linear_abs;
    o << ",\"parts\":" << s.parts << ",\"parts_named\":" << s.parts_named
      << ",\"parts_colored\":" << s.parts_colored
      << ",\"parts_face_colored\":" << s.parts_face_colored;
    o << ",\"prototypes\":" << s.prototypes
      << ",\"prototypes_mesh_failed\":" << s.prototypes_mesh_failed;
    o << ",\"faces\":" << s.faces << ",\"faces_unmeshed\":" << s.faces_unmeshed
      << ",\"faces_remeshed\":" << s.faces_remeshed << ",\"faces_healed\":" << s.faces_healed
      << ",\"faces_refined\":" << s.faces_refined << ",\"faces_coarse\":" << s.faces_coarse
      << ",\"faces_degenerate\":" << s.faces_degenerate << ",\"faces_approx\":" << s.faces_approx
      << ",\"faces_missing\":" << s.faces_missing << ",\"sketch_parts\":" << s.sketch_parts
      << ",\"construction_parts\":" << s.construction_parts;
    o << ",\"vertices\":" << s.vertices << ",\"triangles\":" << s.triangles
      << ",\"segments\":" << s.segments << ",\"edges\":" << s.edges;
    o << ",\"external_files\":" << s.external_files
      << ",\"external_missing\":" << s.external_missing;
    o << ",\"t_read_ms\":" << s.t_read_ms << ",\"t_transfer_ms\":" << s.t_transfer_ms
      << ",\"t_mesh_ms\":" << s.t_mesh_ms << ",\"t_extract_ms\":" << s.t_extract_ms
      << ",\"t_topology_ms\":" << s.t_topology_ms;
    o << ",\"peak_rss_bytes\":" << peak_rss_bytes() << "}";
    return o.str();
}

// ── Timing ──────────────────────────────────────────────────────────────────

using Clock = std::chrono::steady_clock;

double ms_since(Clock::time_point t0) {
    return std::chrono::duration<double, std::milli>(Clock::now() - t0).count();
}

// ── XCAF walk ───────────────────────────────────────────────────────────────

struct Rgb {
    float r, g, b;
};

// One placed instance of a simple (non-assembly) shape.
struct PartInstance {
    TDF_Label prototype;
    TopLoc_Location location;
    std::string name;
    std::optional<Rgb> color;
    bool face_colored = false;
};

std::string label_name(const TDF_Label& label) {
    Handle(TDataStd_Name) attr;
    if (!label.FindAttribute(TDataStd_Name::GetID(), attr)) return {};
    // With no replacement character given, OCCT converts to UTF-8.
    TCollection_AsciiString utf8(attr->Get());
    return utf8.ToCString();
}

std::optional<Rgb> label_color(const Handle(XCAFDoc_ColorTool)& ct, const TDF_Label& label) {
    Quantity_Color c;
    if (ct->GetColor(label, XCAFDoc_ColorSurf, c) || ct->GetColor(label, XCAFDoc_ColorGen, c)) {
        // XCAF stores linear RGB, which is what stepv::Color carries.
        return Rgb{static_cast<float>(c.Red()), static_cast<float>(c.Green()),
                   static_cast<float>(c.Blue())};
    }
    return std::nullopt;
}

bool has_face_colors(const Handle(XCAFDoc_ShapeTool)& st, const Handle(XCAFDoc_ColorTool)& ct,
                     const TDF_Label& prototype) {
    TDF_LabelSequence subs;
    st->GetSubShapes(prototype, subs);
    for (const TDF_Label& sub : subs) {
        if (label_color(ct, sub)) return true;
    }
    return false;
}

// Flattens the assembly under `label` into placed parts, and records its
// structure in `tree` (stepv::TopoNode; `parent` is this subtree's parent).
void walk(const Handle(XCAFDoc_ShapeTool)& st, const Handle(XCAFDoc_ColorTool)& ct,
          const TDF_Label& label, const TopLoc_Location& location,
          const std::string& instance_name, std::optional<Rgb> inherited,
          std::vector<PartInstance>& out, std::vector<stepv::TopoNode>& tree, int parent,
          int depth) {
    // A cyclic or absurdly deep assembly graph is malformed input, not a
    // reason to blow the stack.
    if (depth > 64) throw std::runtime_error("assembly nesting deeper than 64");

    if (auto own = label_color(ct, label)) inherited = own;

    if (st->IsAssembly(label)) {
        const int node = static_cast<int>(tree.size());
        const std::string name = label_name(label);
        tree.push_back({name.empty() ? instance_name : name, parent, -1});
        TDF_LabelSequence components;
        st->GetComponents(label, components, false);
        for (const TDF_Label& comp : components) {
            TDF_Label referred;
            if (!st->GetReferredShape(comp, referred)) continue;
            // An instance colour overrides its prototype's, so resolve the
            // instance's colour first and let the prototype's apply only when
            // the instance has none.
            std::optional<Rgb> c = label_color(ct, comp);
            walk(st, ct, referred, location * st->GetLocation(comp), label_name(comp),
                 c ? c : inherited, out, tree, node, depth + 1);
        }
        return;
    }

    PartInstance part;
    part.prototype = label;
    part.location = location;
    // The prototype's name is the part's name ("M6 bolt"); the instance name
    // is often a generated "NAUO12" or "bolt:3". Prefer the former.
    part.name = label_name(label);
    if (part.name.empty()) part.name = instance_name;
    part.color = inherited;
    part.face_colored = !part.color && has_face_colors(st, ct, label);
    tree.push_back({part.name, parent, static_cast<long>(out.size())});
    out.push_back(std::move(part));
}

// ── Per-face geometry and the recovery ladder ───────────────────────────────

enum FaceStatus : uint8_t {
    kOk = 0, kRemeshed, kHealed, kRefined, kCoarse, kDegenerate, kApprox, kMissing
};
enum LineKind : uint8_t { kSketch = 0, kMissingOutline = 1, kConstruction = 2 };

// One face's output in its PROTOTYPE's coordinates (the face's own location
// applied, the instance placement not yet). Computed once per face and reused
// by every instance.
struct FaceGeom {
    FaceStatus status = kOk;
    std::optional<Rgb> color;  // per-face colour from XCAF, set by the caller
    std::vector<gp_Pnt> nodes;
    std::vector<gp_Dir> normals;
    std::vector<std::array<uint32_t, 3>> tris;
    std::vector<std::array<gp_Pnt, 2>> outline;  // only for kMissing
};

bool has_triangles(const TopoDS_Face& f) {
    TopLoc_Location l;
    Handle(Poly_Triangulation) t = BRep_Tool::Triangulation(f, l);
    return !t.IsNull() && t->NbTriangles() > 0;
}

// Appends an existing triangulation of `face`, oriented outward.
void append_triangulation(const TopoDS_Face& face, FaceGeom& g) {
    TopLoc_Location floc;
    Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, floc);
    if (tri.IsNull() || tri->NbTriangles() == 0) return;
    if (!tri->HasNormals()) BRepLib_ToolTriangulatedShape::ComputeNormals(face, tri);
    const gp_Trsf trsf = floc.Transformation();
    const bool reversed = face.Orientation() == TopAbs_REVERSED;
    const uint32_t base = static_cast<uint32_t>(g.nodes.size());
    for (int i = 1; i <= tri->NbNodes(); ++i) {
        g.nodes.push_back(tri->Node(i).Transformed(trsf));
        gp_Dir n = tri->Normal(i);
        n.Transform(trsf);
        g.normals.push_back(reversed ? n.Reversed() : n);
    }
    for (int i = 1; i <= tri->NbTriangles(); ++i) {
        int a, b, c;
        tri->Triangle(i).Get(a, b, c);
        if (reversed) std::swap(b, c);
        g.tris.push_back({base + uint32_t(a - 1), base + uint32_t(b - 1), base + uint32_t(c - 1)});
    }
}

// An independent copy (geometry included, no triangulation): re-meshing it can
// never disturb the shared edges of the faces around the original.
TopoDS_Shape isolated_copy(const TopoDS_Face& f) {
    return BRepBuilderAPI_Copy(f, /*copyGeom=*/true, /*copyMesh=*/false).Shape();
}

bool mesh_and_take(const TopoDS_Shape& shape, const IMeshTools_Parameters& p, FaceGeom& g) {
    try {
        BRepMesh_IncrementalMesh mesher(shape, p);
    } catch (const Standard_Failure&) {
        return false;
    }
    bool any = false;
    for (TopExp_Explorer ex(shape, TopAbs_FACE); ex.More(); ex.Next()) {
        const TopoDS_Face& f = TopoDS::Face(ex.Current());
        if (has_triangles(f)) {
            append_triangulation(f, g);
            any = true;
        }
    }
    return any;
}

// Rung 4: sample the surface on a UV grid and keep the cells whose centre lies
// inside the face. No mesher involved, so it works on faces BRepMesh rejects;
// the boundary is jagged at grid resolution, which is why it is flagged.
bool approximate(const TopoDS_Face& face, FaceGeom& g) {
    double u0, u1, v0, v1;
    try {
        BRepTools::UVBounds(face, u0, u1, v0, v1);
    } catch (const Standard_Failure&) {
        return false;
    }
    if (!std::isfinite(u0) || !std::isfinite(u1) || !std::isfinite(v0) || !std::isfinite(v1) ||
        !(u1 > u0) || !(v1 > v0))
        return false;
    constexpr int N = 48;
    const double du = (u1 - u0) / N, dv = (v1 - v0) / N;
    BRepAdaptor_Surface surf(face);  // applies the face's own location
    BRepLProp_SLProps props(surf, 1, Precision::Confusion());
    const double tol = BRep_Tool::Tolerance(face);
    const bool reversed = face.Orientation() == TopAbs_REVERSED;

    // Grid nodes, computed lazily: a NaN normal marks a degenerate point.
    std::vector<int> index((N + 1) * (N + 1), -1);
    auto node = [&](int i, int j) -> int {
        int& k = index[i * (N + 1) + j];
        if (k >= 0) return k;
        const double u = u0 + i * du, v = v0 + j * dv;
        props.SetParameters(u, v);
        gp_Dir n(0, 0, 1);
        if (props.IsNormalDefined()) n = props.Normal();
        if (reversed) n.Reverse();
        k = static_cast<int>(g.nodes.size());
        g.nodes.push_back(props.Value());
        g.normals.push_back(n);
        return k;
    };
    BRepClass_FaceClassifier classifier;
    for (int i = 0; i < N; ++i) {
        for (int j = 0; j < N; ++j) {
            gp_Pnt2d centre(u0 + (i + 0.5) * du, v0 + (j + 0.5) * dv);
            classifier.Perform(face, centre, tol);
            if (classifier.State() != TopAbs_IN) continue;
            uint32_t a = node(i, j), b = node(i + 1, j), c = node(i + 1, j + 1),
                     d = node(i, j + 1);
            if (reversed) {
                g.tris.push_back({a, c, b});
                g.tris.push_back({a, d, c});
            } else {
                g.tris.push_back({a, b, c});
                g.tris.push_back({a, c, d});
            }
        }
    }
    return !g.tris.empty();
}

void discretize_edge(const TopoDS_Edge& e, double defl, double angle,
                     std::vector<std::array<gp_Pnt, 2>>& out) {
    if (BRep_Tool::Degenerated(e)) return;
    try {
        BRepAdaptor_Curve curve(e);  // applies the edge's own location
        GCPnts_TangentialDeflection pts(curve, angle, defl);
        for (int i = 1; i < pts.NbPoints(); ++i) out.push_back({pts.Value(i), pts.Value(i + 1)});
    } catch (const Standard_Failure&) {
        // An edge that cannot be evaluated is simply not drawn.
    }
}

// A face no renderer could show: zero width in either parameter direction,
// or |area| below 1e-8 of the bbox diagonal squared, which is under one pixel
// on a 10,000-pixel-wide render of the whole model. Exporters leave such
// slivers behind after booleans. It runs AFTER the refined re-mesh, so a real
// but tiny face (a 0.0075 mm² fillet) is meshed, not discarded.
bool degenerate(const TopoDS_Face& face, double diagonal) {
    double u0, u1, v0, v1;
    try {
        BRepTools::UVBounds(face, u0, u1, v0, v1);
    } catch (const Standard_Failure&) {
        return false;
    }
    auto flat = [](double a, double b) { return std::abs(b - a) <= 1e-9 * std::max(1.0, std::abs(a)); };
    if (flat(u0, u1) || flat(v0, v1)) return true;
    GProp_GProps g;
    BRepGProp::SurfaceProperties(face, g);
    return std::abs(g.Mass()) < 1e-8 * diagonal * diagonal;
}

// Runs the ladder for a face the first pass left without triangles.
FaceGeom recover(const TopoDS_Face& face, const IMeshTools_Parameters& p, double diagonal,
                 Summary& s) {
    FaceGeom g;
    // 1. Clean re-mesh of an isolated copy.
    {
        TopoDS_Shape c = isolated_copy(face);
        if (mesh_and_take(c, p, g)) {
            g.status = kRemeshed;
            ++s.faces_remeshed;
            return g;
        }
    }
    // 2. ShapeFix the copy (it may split the face), then mesh.
    try {
        Handle(ShapeFix_Shape) fix = new ShapeFix_Shape(isolated_copy(face));
        fix->Perform();
        if (mesh_and_take(fix->Shape(), p, g)) {
            g.status = kHealed;
            ++s.faces_healed;
            return g;
        }
    } catch (const Standard_Failure&) {
    }
    // 3. Deflection relative to the face itself, not the model. Recovers faces
    //    thousands of times smaller than the model, and boundaries that only
    //    self-intersect once discretized at the model-relative deflection.
    {
        TopoDS_Shape c = isolated_copy(face);
        Bnd_Box fb;
        BRepBndLib::Add(c, fb, false);
        IMeshTools_Parameters fine = p;
        fine.Deflection = std::max(1e-3 * std::sqrt(fb.SquareExtent()), 1e-7);
        fine.InParallel = false;
        if (!fb.IsVoid() && mesh_and_take(c, fine, g)) {
            g.status = kRefined;
            ++s.faces_refined;
            return g;
        }
    }
    // Nothing to draw is not the same as something missing.
    if (degenerate(face, diagonal)) {
        g = FaceGeom{};
        g.status = kDegenerate;
        ++s.faces_degenerate;
        return g;
    }
    // 4. Relaxed parameters: 10x coarser, 45 degrees, no surface-deviation
    //    control. Still the exact surface, just coarsely sampled.
    {
        IMeshTools_Parameters coarse = p;
        coarse.Deflection = p.Deflection * 10;
        coarse.Angle = 45.0 * M_PI / 180.0;
        coarse.ControlSurfaceDeflection = false;
        coarse.AllowQualityDecrease = true;
        coarse.InParallel = false;
        if (mesh_and_take(isolated_copy(face), coarse, g)) {
            g.status = kCoarse;
            ++s.faces_coarse;
            return g;
        }
    }
    // 5. UV-grid approximation.
    g = FaceGeom{};
    if (approximate(face, g)) {
        g.status = kApprox;
        ++s.faces_approx;
        return g;
    }
    // 6. Give up on area; keep the outline so the hole is drawn, not hidden.
    g = FaceGeom{};
    g.status = kMissing;
    ++s.faces_missing;
    for (TopExp_Explorer ex(face, TopAbs_EDGE); ex.More(); ex.Next())
        discretize_edge(TopoDS::Edge(ex.Current()), p.Deflection, p.Angle, g.outline);
    return g;
}

using FaceCache = NCollection_DataMap<TopoDS_Shape, FaceGeom, TopTools_ShapeMapHasher>;
using FaceColors = NCollection_DataMap<TopoDS_Shape, Rgb, TopTools_ShapeMapHasher>;

// Per-face colours of one prototype, from its XCAF sub-shape labels. A
// colour on a shell or solid sub-shape applies to its faces; a colour on a
// face label wins over both, so face labels are applied last.
FaceColors face_colors(const Handle(XCAFDoc_ShapeTool)& st, const Handle(XCAFDoc_ColorTool)& ct,
                       const TDF_Label& prototype) {
    FaceColors out;
    TDF_LabelSequence subs;
    st->GetSubShapes(prototype, subs);
    for (int pass = 0; pass < 2; ++pass) {
        for (const TDF_Label& sub : subs) {
            const TopoDS_Shape shape = st->GetShape(sub);
            const bool is_face = shape.ShapeType() == TopAbs_FACE;
            if (is_face != (pass == 1)) continue;
            const std::optional<Rgb> c = label_color(ct, sub);
            if (!c) continue;
            for (TopExp_Explorer ex(shape, TopAbs_FACE); ex.More(); ex.Next()) out.Bind(ex.Current(), *c);
        }
    }
    return out;
}

// Per-prototype output, in prototype coordinates.
struct EdgeGeom {
    uint32_t id;  // topology edge index
    std::vector<gp_Pnt> points;
};

struct ProtoGeom {
    std::vector<FaceGeom> faces;
    std::vector<std::array<gp_Pnt, 2>> sketch;  // curves of a part with no faces
    std::vector<EdgeGeom> edges;                // B-rep edges, with --edges
};

// The prototype's B-rep edges as polylines, at the mesh's deflection and in
// the topology's numbering (#31). An edge OCCT cannot sample is left out:
// the edge list is for drawing and picking, not a contract on completeness.
std::vector<EdgeGeom> edge_geometry(const TopoDS_Shape& shape, const IMeshTools_Parameters& p) {
    std::vector<EdgeGeom> out;
    const TopTools_IndexedMapOfShape edges = stepv::topology_edges(shape);
    out.reserve(static_cast<std::size_t>(edges.Extent()));
    for (int i = 1; i <= edges.Extent(); ++i) {
        try {
            const TopoDS_Edge& e = TopoDS::Edge(edges(i));
            if (!BRep_Tool::IsGeometric(e)) continue;
            BRepAdaptor_Curve c(e);
            GCPnts_TangentialDeflection d(c, p.Angle, p.Deflection);
            if (d.NbPoints() < 2) continue;
            EdgeGeom g{static_cast<uint32_t>(i - 1), {}};
            g.points.reserve(static_cast<std::size_t>(d.NbPoints()));
            for (int k = 1; k <= d.NbPoints(); ++k) g.points.push_back(d.Value(k));
            out.push_back(std::move(g));
        } catch (const Standard_Failure&) {
        }
    }
    return out;
}

ProtoGeom prototype_geometry(const TopoDS_Shape& shape, const IMeshTools_Parameters& p,
                             double diagonal, const FaceColors& colors, FaceCache& cache,
                             Summary& s) {
    ProtoGeom pg;
    for (TopExp_Explorer ex(shape, TopAbs_FACE); ex.More(); ex.Next()) {
        const TopoDS_Face& face = TopoDS::Face(ex.Current());
        const Rgb* color = colors.Seek(face);
        if (const FaceGeom* hit = cache.Seek(face)) {
            pg.faces.push_back(*hit);
            pg.faces.back().color = color ? std::optional<Rgb>(*color) : std::nullopt;
            continue;
        }
        FaceGeom g;
        if (has_triangles(face)) {
            append_triangulation(face, g);
        } else {
            ++s.faces_unmeshed;
            g = recover(face, p, diagonal, s);
        }
        cache.Bind(face, g);
        pg.faces.push_back(std::move(g));
        pg.faces.back().color = color ? std::optional<Rgb>(*color) : std::nullopt;
    }
    if (pg.faces.empty()) {
        // A sketch, a wireframe export, a curve set: no surfaces to mesh, but
        // real content. Draw the curves rather than report "no geometry".
        for (TopExp_Explorer ex(shape, TopAbs_EDGE); ex.More(); ex.Next())
            discretize_edge(TopoDS::Edge(ex.Current()), p.Deflection, p.Angle, pg.sketch);
    }
    return pg;
}

struct PartMesh {
    std::vector<uint8_t> face_status;
    std::vector<std::optional<Rgb>> face_color;
    std::vector<float> positions, normals;
    std::vector<uint32_t> indices, face_ids;
    std::vector<float> seg_points;
    std::vector<uint8_t> seg_kinds;
    std::vector<uint32_t> edge_ids, edge_lens;
    std::vector<float> edge_points;
};

void push3(std::vector<float>& v, double x, double y, double z) {
    v.insert(v.end(), {static_cast<float>(x), static_cast<float>(y), static_cast<float>(z)});
}

// Places one instance of a prototype.
void place(const ProtoGeom& pg, const TopLoc_Location& placement, LineKind curve_kind,
           PartMesh& m, Summary& s) {
    const gp_Trsf t = placement.Transformation();
    for (std::size_t fid = 0; fid < pg.faces.size(); ++fid) {
        const FaceGeom& g = pg.faces[fid];
        ++s.faces;
        m.face_status.push_back(g.status);
        m.face_color.push_back(g.color);
        const uint32_t base = static_cast<uint32_t>(m.positions.size() / 3);
        for (std::size_t i = 0; i < g.nodes.size(); ++i) {
            gp_Pnt p = g.nodes[i].Transformed(t);
            gp_Dir n = g.normals[i].Transformed(t);
            push3(m.positions, p.X(), p.Y(), p.Z());
            push3(m.normals, n.X(), n.Y(), n.Z());
        }
        for (const auto& tri : g.tris) {
            m.indices.insert(m.indices.end(), {base + tri[0], base + tri[1], base + tri[2]});
            m.face_ids.push_back(static_cast<uint32_t>(fid));
        }
        for (const auto& seg : g.outline) {
            for (const gp_Pnt& q : seg) {
                gp_Pnt p = q.Transformed(t);
                push3(m.seg_points, p.X(), p.Y(), p.Z());
            }
            m.seg_kinds.push_back(kMissingOutline);
        }
    }
    for (const auto& seg : pg.sketch) {
        for (const gp_Pnt& q : seg) {
            gp_Pnt p = q.Transformed(t);
            push3(m.seg_points, p.X(), p.Y(), p.Z());
        }
        m.seg_kinds.push_back(curve_kind);
    }
    if (!pg.sketch.empty()) ++(curve_kind == kSketch ? s.sketch_parts : s.construction_parts);
    for (const EdgeGeom& e : pg.edges) {
        m.edge_ids.push_back(e.id);
        m.edge_lens.push_back(static_cast<uint32_t>(e.points.size()));
        for (const gp_Pnt& q : e.points) {
            gp_Pnt p = q.Transformed(t);
            push3(m.edge_points, p.X(), p.Y(), p.Z());
        }
    }
}

// ── Mesh file ───────────────────────────────────────────────────────────────

template <typename T>
void put(std::ofstream& f, const T& v) {
    f.write(reinterpret_cast<const char*>(&v), sizeof v);
}

template <typename T>
void put_vec(std::ofstream& f, const std::vector<T>& v) {
    f.write(reinterpret_cast<const char*>(v.data()),
            static_cast<std::streamsize>(v.size() * sizeof(T)));
}

static_assert(sizeof(float) == 4 && sizeof(double) == 8, "IEEE-754 widths assumed");

// ── Readers ─────────────────────────────────────────────────────────────────

std::string lower_ext(const std::string& path) {
    auto dot = path.find_last_of('.');
    if (dot == std::string::npos) return {};
    std::string e = path.substr(dot + 1);
    for (char& c : e) c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    return e;
}

// Reads `path` into `doc`. Fills the read/transfer timings and returns false
// with s.error set on failure.
bool read_into(const std::string& path, const Handle(TDocStd_Document)& doc, Summary& s) {
    const std::string ext = lower_ext(path);
    auto t0 = Clock::now();

    if (ext == "step" || ext == "stp") {
        s.format = "step";
        s.stage = "read";
        STEPCAFControl_Reader reader;
        reader.SetColorMode(true);
        reader.SetNameMode(true);
        reader.SetLayerMode(true);
        if (reader.ReadFile(path.c_str()) != IFSelect_RetDone) {
            s.error = "STEP read failed";
            return false;
        }
        s.t_read_ms = ms_since(t0);
        s.stage = "transfer";
        t0 = Clock::now();
        if (!reader.Transfer(doc)) {
            s.error = "STEP transfer failed";
            return false;
        }
        // OCCT skips an unreadable part file SILENTLY: count them, so a
        // half-empty or empty assembly says why.
        for (NCollection_DataMap<TCollection_AsciiString, Handle(STEPCAFControl_ExternFile)>::Iterator
                 it(reader.ExternFiles());
             it.More(); it.Next()) {
            ++s.external_files;
            if (it.Value().IsNull() || it.Value()->GetLoadStatus() != IFSelect_RetDone)
                ++s.external_missing;
        }
    } else if (ext == "iges" || ext == "igs") {
        s.format = "iges";
        s.stage = "read";
        IGESCAFControl_Reader reader;
        reader.SetColorMode(true);
        reader.SetNameMode(true);
        reader.SetLayerMode(true);
        if (reader.ReadFile(path.c_str()) != IFSelect_RetDone) {
            s.error = "IGES read failed";
            return false;
        }
        s.t_read_ms = ms_since(t0);
        s.stage = "transfer";
        t0 = Clock::now();
        if (!reader.Transfer(doc)) {
            s.error = "IGES transfer failed";
            return false;
        }
    } else if (ext == "brep") {
        s.format = "brep";
        s.stage = "read";
        TopoDS_Shape shape;
        BRep_Builder builder;
        if (!BRepTools::Read(shape, path.c_str(), builder) || shape.IsNull()) {
            s.error = "BREP read failed";
            return false;
        }
        s.t_read_ms = ms_since(t0);
        s.stage = "transfer";
        t0 = Clock::now();
        XCAFDoc_DocumentTool::ShapeTool(doc->Main())->AddShape(shape, false);
    } else {
        s.format = "unknown";
        s.stage = "read";
        s.error = "unsupported extension: ." + ext;
        return false;
    }
    s.t_transfer_ms = ms_since(t0);
    return true;
}

int run(const std::string& input_arg, const std::string& mesh_out,
        const std::string& topology_out, bool edges, double linear_rel, double angular_deg,
        bool parallel, Summary& s) {
    // STEPCAFControl_Reader resolves multi-file assemblies' external
    // references against the main file's directory ONLY when the path is
    // absolute; given a relative one it silently yields an empty document.
    char resolved[PATH_MAX];
    if (!realpath(input_arg.c_str(), resolved)) {
        s.format = "unknown";
        s.stage = "read";
        s.error = "cannot resolve input path: " + std::string(std::strerror(errno));
        return kExitFailed;
    }
    const std::string input = resolved;

    // Off, so "parts_named" counts names an exporter wrote, not ones OCCT
    // invents ("SOLID", "COMPOUND") for unnamed shapes.
    XCAFDoc_ShapeTool::SetAutoNaming(false);
    Handle(XCAFApp_Application) app = XCAFApp_Application::GetApplication();
    Handle(TDocStd_Document) doc;
    app->NewDocument("BinXCAF", doc);

    if (!read_into(input, doc, s)) return kExitFailed;

    Handle(XCAFDoc_ShapeTool) st = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    Handle(XCAFDoc_ColorTool) ct = XCAFDoc_DocumentTool::ColorTool(doc->Main());

    // ── Assembly tree → placed part instances ──
    s.stage = "walk";
    TDF_LabelSequence roots;
    st->GetFreeShapes(roots);
    std::vector<PartInstance> parts;
    std::vector<stepv::TopoNode> tree;
    Bnd_Box bbox;
    for (const TDF_Label& root : roots) {
        BRepBndLib::Add(st->GetShape(root), bbox, false);
        walk(st, ct, root, TopLoc_Location(), label_name(root), std::nullopt, parts, tree, -1, 0);
    }
    s.parts = parts.size();
    for (const auto& p : parts) {
        if (!p.name.empty()) ++s.parts_named;
        if (p.color) ++s.parts_colored;
        if (p.face_colored) ++s.parts_face_colored;
    }
    if (parts.empty() || bbox.IsVoid()) {
        s.error = s.external_missing
                      ? "multi-file assembly: " + std::to_string(s.external_missing) + " of " +
                            std::to_string(s.external_files) +
                            " part files it references could not be read"
                      : "no geometry in file";
        return kExitFailed;
    }
    s.bbox = bbox;
    s.diagonal = std::sqrt(bbox.SquareExtent());

    // ── Tessellate each prototype ONCE; instances share its triangulation ──
    // Deflection is RELATIVE TO THE WHOLE MODEL'S bbox diagonal (plan.md §4),
    // converted to an absolute length here. OCCT's own `Relative` flag is a
    // different thing — per-edge relative — and is deliberately left off.
    s.stage = "mesh";
    s.linear_abs = linear_rel * s.diagonal;
    IMeshTools_Parameters params;
    params.Deflection = s.linear_abs;
    params.Angle = angular_deg * M_PI / 180.0;
    params.Relative = false;
    params.InParallel = parallel;

    auto t0 = Clock::now();
    // Keyed by the label's entry ("0:1:1:3"): TDF_Label itself is not ordered.
    std::map<std::string, TopoDS_Shape> prototypes;
    std::map<std::string, TDF_Label> prototype_labels;
    auto entry = [](const TDF_Label& l) {
        TCollection_AsciiString e;
        TDF_Tool::Entry(l, e);
        return std::string(e.ToCString());
    };
    for (const auto& p : parts) {
        if (prototypes.count(entry(p.prototype))) continue;
        TopoDS_Shape shape = st->GetShape(p.prototype);
        try {
            BRepMesh_IncrementalMesh mesher(shape, params);
            if (!mesher.IsDone()) ++s.prototypes_mesh_failed;
        } catch (const Standard_Failure&) {
            // Leave the shape unmeshed; its faces are counted as unmeshed
            // below, so the failure is visible rather than swallowed.
            ++s.prototypes_mesh_failed;
        }
        prototypes.emplace(entry(p.prototype), shape);
        prototype_labels.emplace(entry(p.prototype), p.prototype);
    }
    s.prototypes = prototypes.size();

    // First-pass triangles plus the recovery ladder for every face that has
    // none, once per prototype. Counted as mesh time: it is meshing.
    FaceCache cache;
    std::map<std::string, ProtoGeom> geoms;
    for (const auto& [key, shape] : prototypes) {
        ProtoGeom g = prototype_geometry(shape, params, s.diagonal,
                                         face_colors(st, ct, prototype_labels.at(key)), cache, s);
        if (edges) g.edges = edge_geometry(shape, params);
        geoms.emplace(key, std::move(g));
    }
    s.t_mesh_ms = ms_since(t0);
    // Faceless parts are the content of a sketch-only file, but construction
    // geometry in a file that has solids.
    bool any_faces = false;
    for (const auto& [key, g] : geoms) any_faces |= !g.faces.empty();
    const LineKind curve_kind = any_faces ? kConstruction : kSketch;

    // ── Extract placed buffers ──
    s.stage = "extract";
    t0 = Clock::now();
    std::ofstream f;
    if (!mesh_out.empty()) {
        f.open(mesh_out, std::ios::binary | std::ios::trunc);
        if (!f) {
            s.error = "cannot open mesh output: " + mesh_out;
            return kExitFailed;
        }
        f.write("STEPVMSH", 8);
        put<uint32_t>(f, edges ? 4 : 3);
        double x0, y0, z0, x1, y1, z1;
        bbox.Get(x0, y0, z0, x1, y1, z1);
        for (double v : {x0, y0, z0, x1, y1, z1}) put(f, v);
        put<uint32_t>(f, static_cast<uint32_t>(parts.size()));
    }
    for (const auto& p : parts) {
        PartMesh m;
        place(geoms.at(entry(p.prototype)), p.location, curve_kind, m, s);
        s.vertices += m.positions.size() / 3;
        s.triangles += m.indices.size() / 3;
        s.segments += m.seg_kinds.size();
        if (f.is_open()) {
            put<uint32_t>(f, static_cast<uint32_t>(p.name.size()));
            f.write(p.name.data(), static_cast<std::streamsize>(p.name.size()));
            put<uint8_t>(f, p.color ? 1 : 0);
            Rgb c = p.color.value_or(Rgb{0, 0, 0});
            put(f, c.r);
            put(f, c.g);
            put(f, c.b);
            put<uint32_t>(f, static_cast<uint32_t>(m.face_status.size()));
            for (std::size_t i = 0; i < m.face_status.size(); ++i) {
                put<uint8_t>(f, m.face_status[i]);
                put<uint8_t>(f, m.face_color[i] ? 1 : 0);
                const Rgb fc = m.face_color[i].value_or(Rgb{0, 0, 0});
                put(f, fc.r);
                put(f, fc.g);
                put(f, fc.b);
            }
            put<uint32_t>(f, static_cast<uint32_t>(m.positions.size() / 3));
            put<uint32_t>(f, static_cast<uint32_t>(m.indices.size() / 3));
            put_vec(f, m.positions);
            put_vec(f, m.normals);
            put_vec(f, m.indices);
            put_vec(f, m.face_ids);
            put<uint32_t>(f, static_cast<uint32_t>(m.seg_kinds.size()));
            put_vec(f, m.seg_points);
            put_vec(f, m.seg_kinds);
            if (edges) {
                put<uint32_t>(f, static_cast<uint32_t>(m.edge_ids.size()));
                put_vec(f, m.edge_ids);
                put_vec(f, m.edge_lens);
                put_vec(f, m.edge_points);
            }
        }
        s.edges += m.edge_ids.size();
    }
    if (f.is_open()) {
        f.close();
        if (!f) {
            s.error = "mesh output write failed";
            return kExitFailed;
        }
    }
    s.t_extract_ms = ms_since(t0);

    // ── Exact topology, on request (#21) ──
    if (!topology_out.empty()) {
        s.stage = "topology";
        t0 = Clock::now();
        // Prototypes numbered by first use, the order a reader meets them.
        std::map<std::string, std::size_t> index;
        std::vector<TopoDS_Shape> shapes;
        std::vector<stepv::TopoPart> placed;
        for (const auto& p : parts) {
            const auto [it, fresh] = index.emplace(entry(p.prototype), shapes.size());
            if (fresh) shapes.push_back(prototypes.at(entry(p.prototype)));
            placed.push_back({p.name, it->second, p.location.Transformation()});
        }
        if (const std::string err = stepv::write_topology(topology_out, tree, placed, shapes);
            !err.empty()) {
            s.error = err;
            return kExitFailed;
        }
        s.t_topology_ms = ms_since(t0);
    }

    s.stage = "done";
    if (s.triangles == 0 && s.segments == 0) {
        s.error = "file has neither surfaces nor curves to draw";
        return kExitFailed;
    }
    return kExitOk;
}

}  // namespace


extern "C" char* stepv_occt_run(const char* input, const char* mesh_out, double linear_rel,
                                double angular_deg, int* exit_code) {
    return stepv_occt_run_ex(input, mesh_out, nullptr, 0, linear_rel, angular_deg, exit_code);
}

extern "C" char* stepv_occt_run_topology(const char* input, const char* mesh_out,
                                         const char* topology_out, double linear_rel,
                                         double angular_deg, int* exit_code) {
    return stepv_occt_run_ex(input, mesh_out, topology_out, 0, linear_rel, angular_deg,
                             exit_code);
}

extern "C" char* stepv_occt_run_ex(const char* input, const char* mesh_out,
                                   const char* topology_out, int edges, double linear_rel,
                                   double angular_deg, int* exit_code) {
    // OCCT's data-exchange layer keeps global state (Interface_Static, the
    // XSControl session): one file at a time per process. The CLI never
    // notices (one run per process); Quick Look issues concurrent requests.
    static std::mutex serial;
    std::lock_guard<std::mutex> lock(serial);
    static std::once_flag quiet;
    std::call_once(quiet, [] {
        Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
    });

    Summary s;
    int code = kExitFailed;
    if (!input || !(linear_rel > 0) || !(angular_deg > 0)) {
        s.error = "invalid arguments";
    } else {
        try {
            code = run(input, mesh_out ? mesh_out : "", topology_out ? topology_out : "",
                       edges != 0, linear_rel, angular_deg, true, s);
        } catch (const Standard_Failure& e) {
            s.error = std::string("OCCT: ") + e.GetMessageString();
        } catch (const std::bad_alloc&) {
            s.error = "out of memory";
        } catch (const std::exception& e) {
            s.error = e.what();
        }
    }
    if (exit_code) *exit_code = code;
    const std::string json = to_json(s);
    char* out = static_cast<char*>(std::malloc(json.size() + 1));
    if (out) std::memcpy(out, json.c_str(), json.size() + 1);
    return out;
}

extern "C" void stepv_occt_free(char* p) { std::free(p); }
