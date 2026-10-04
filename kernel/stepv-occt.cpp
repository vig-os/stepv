// stepv-occt — the Plan B kernel (plan.md §3): native OCCT, run as a
// subprocess by the Rust side.
//
// One input file in; a JSON summary on stdout and, with --mesh, the planar
// mesh buffers in a file. It is a separate process on purpose: a hostile or
// broken CAD file that crashes OCCT takes down this process, not the
// previewer, and the parent enforces the wall-clock cap by killing it. That
// is the isolation the WASM sandbox was going to provide under Plan A.
//
// Units: OCCT's readers convert to millimetres, so everything emitted here is
// in mm regardless of the file's declared units.
//
// Mesh file format ("STEPVMSH", version 1, little-endian, no padding):
//
//   magic        8 bytes  "STEPVMSH"
//   version      u32      1
//   bbox         6 x f64  min xyz, max xyz
//   part_count   u32
//   per part:
//     name_len   u32      0 = no name
//     name       name_len bytes, UTF-8
//     has_color  u8
//     rgb        3 x f32  (present, zero when has_color = 0)
//     vertices   u32
//     triangles  u32
//     positions  3 * vertices  x f32
//     normals    3 * vertices  x f32
//     indices    3 * triangles x u32
//     face_ids   triangles     x u32
//
// src/occt.rs is the reader; keep the two in step.

#include <BRepBndLib.hxx>
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
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <new>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

namespace {

constexpr int kExitOk = 0;
constexpr int kExitUsage = 2;
constexpr int kExitFailed = 3;

const char* const kUsage =
    "usage: stepv-occt <input> [--mesh <out>] [--linear-rel <f>] [--angular-deg <f>] [--serial]\n";

// ── JSON output ─────────────────────────────────────────────────────────────

std::string json_escape(const std::string& s) {
    std::string out;
    out.reserve(s.size() + 2);
    for (unsigned char c : s) {
        switch (c) {
        case '"': out += "\\\""; break;
        case '\\': out += "\\\\"; break;
        case '\n': out += "\\n"; break;
        case '\r': out += "\\r"; break;
        case '\t': out += "\\t"; break;
        default:
            if (c < 0x20) {
                char buf[8];
                std::snprintf(buf, sizeof buf, "\\u%04x", c);
                out += buf;
            } else {
                out += static_cast<char>(c);
            }
        }
    }
    return out;
}

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
    std::size_t faces_unmeshed = 0;
    std::size_t vertices = 0;
    std::size_t triangles = 0;
    double t_read_ms = 0, t_transfer_ms = 0, t_mesh_ms = 0, t_extract_ms = 0;
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
    o << ",\"faces\":" << s.faces << ",\"faces_unmeshed\":" << s.faces_unmeshed;
    o << ",\"vertices\":" << s.vertices << ",\"triangles\":" << s.triangles;
    o << ",\"t_read_ms\":" << s.t_read_ms << ",\"t_transfer_ms\":" << s.t_transfer_ms
      << ",\"t_mesh_ms\":" << s.t_mesh_ms << ",\"t_extract_ms\":" << s.t_extract_ms;
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

void walk(const Handle(XCAFDoc_ShapeTool)& st, const Handle(XCAFDoc_ColorTool)& ct,
          const TDF_Label& label, const TopLoc_Location& location,
          const std::string& instance_name, std::optional<Rgb> inherited,
          std::vector<PartInstance>& out, int depth) {
    // A cyclic or absurdly deep assembly graph is malformed input, not a
    // reason to blow the stack.
    if (depth > 64) throw std::runtime_error("assembly nesting deeper than 64");

    if (auto own = label_color(ct, label)) inherited = own;

    if (st->IsAssembly(label)) {
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
                 c ? c : inherited, out, depth + 1);
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
    out.push_back(std::move(part));
}

// ── Mesh extraction ─────────────────────────────────────────────────────────

struct PartMesh {
    std::vector<float> positions, normals;
    std::vector<uint32_t> indices, face_ids;
};

void extract(const TopoDS_Shape& shape, const TopLoc_Location& placement, PartMesh& m,
             Summary& s) {
    uint32_t face_id = 0;
    for (TopExp_Explorer ex(shape, TopAbs_FACE); ex.More(); ex.Next(), ++face_id) {
        const TopoDS_Face& face = TopoDS::Face(ex.Current());
        ++s.faces;
        TopLoc_Location floc;
        Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, floc);
        if (tri.IsNull() || tri->NbTriangles() == 0) {
            // A face with no triangles is a HOLE in the rendered part. Count
            // it — this is the silent failure plan.md §2 holds against Foxtrot.
            ++s.faces_unmeshed;
            continue;
        }
        if (!tri->HasNormals()) BRepLib_ToolTriangulatedShape::ComputeNormals(face, tri);

        const gp_Trsf trsf = (placement * floc).Transformation();
        const bool reversed = face.Orientation() == TopAbs_REVERSED;
        const uint32_t base = static_cast<uint32_t>(m.positions.size() / 3);

        for (int i = 1; i <= tri->NbNodes(); ++i) {
            gp_Pnt p = tri->Node(i).Transformed(trsf);
            gp_Dir n = tri->Normal(i);
            n.Transform(trsf);
            if (reversed) n.Reverse();
            m.positions.insert(m.positions.end(), {static_cast<float>(p.X()),
                                                   static_cast<float>(p.Y()),
                                                   static_cast<float>(p.Z())});
            m.normals.insert(m.normals.end(), {static_cast<float>(n.X()),
                                               static_cast<float>(n.Y()),
                                               static_cast<float>(n.Z())});
        }
        for (int i = 1; i <= tri->NbTriangles(); ++i) {
            int a, b, c;
            tri->Triangle(i).Get(a, b, c);
            if (reversed) std::swap(b, c);
            m.indices.insert(m.indices.end(), {base + static_cast<uint32_t>(a - 1),
                                               base + static_cast<uint32_t>(b - 1),
                                               base + static_cast<uint32_t>(c - 1)});
            m.face_ids.push_back(face_id);
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

int run(const std::string& input_arg, const std::string& mesh_out, double linear_rel,
        double angular_deg, bool parallel, Summary& s) {
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
    Bnd_Box bbox;
    for (const TDF_Label& root : roots) {
        BRepBndLib::Add(st->GetShape(root), bbox, false);
        walk(st, ct, root, TopLoc_Location(), label_name(root), std::nullopt, parts, 0);
    }
    s.parts = parts.size();
    for (const auto& p : parts) {
        if (!p.name.empty()) ++s.parts_named;
        if (p.color) ++s.parts_colored;
        if (p.face_colored) ++s.parts_face_colored;
    }
    if (parts.empty() || bbox.IsVoid()) {
        s.error = "no geometry in file";
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
    }
    s.prototypes = prototypes.size();
    s.t_mesh_ms = ms_since(t0);

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
        put<uint32_t>(f, 1);
        double x0, y0, z0, x1, y1, z1;
        bbox.Get(x0, y0, z0, x1, y1, z1);
        for (double v : {x0, y0, z0, x1, y1, z1}) put(f, v);
        put<uint32_t>(f, static_cast<uint32_t>(parts.size()));
    }
    for (const auto& p : parts) {
        PartMesh m;
        extract(prototypes.at(entry(p.prototype)), p.location, m, s);
        s.vertices += m.positions.size() / 3;
        s.triangles += m.indices.size() / 3;
        if (f.is_open()) {
            put<uint32_t>(f, static_cast<uint32_t>(p.name.size()));
            f.write(p.name.data(), static_cast<std::streamsize>(p.name.size()));
            put<uint8_t>(f, p.color ? 1 : 0);
            Rgb c = p.color.value_or(Rgb{0, 0, 0});
            put(f, c.r);
            put(f, c.g);
            put(f, c.b);
            put<uint32_t>(f, static_cast<uint32_t>(m.positions.size() / 3));
            put<uint32_t>(f, static_cast<uint32_t>(m.indices.size() / 3));
            put_vec(f, m.positions);
            put_vec(f, m.normals);
            put_vec(f, m.indices);
            put_vec(f, m.face_ids);
        }
    }
    if (f.is_open()) {
        f.close();
        if (!f) {
            s.error = "mesh output write failed";
            return kExitFailed;
        }
    }
    s.t_extract_ms = ms_since(t0);

    s.stage = "done";
    if (s.triangles == 0) {
        s.error = "tessellation produced no triangles";
        return kExitFailed;
    }
    return kExitOk;
}

}  // namespace

int main(int argc, char** argv) {
    std::string input, mesh_out;
    double linear_rel = 0.001, angular_deg = 20.0;  // = stepv::Deflection::PREVIEW
    bool parallel = true;
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        auto value = [&]() -> const char* {
            if (i + 1 >= argc) {
                std::fputs(kUsage, stderr);
                std::exit(kExitUsage);
            }
            return argv[++i];
        };
        if (a == "--mesh") mesh_out = value();
        else if (a == "--linear-rel") linear_rel = std::strtod(value(), nullptr);
        else if (a == "--angular-deg") angular_deg = std::strtod(value(), nullptr);
        else if (a == "--serial") parallel = false;
        else if (a == "-h" || a == "--help") { std::fputs(kUsage, stdout); return kExitOk; }
        else if (!a.empty() && a[0] == '-') { std::fputs(kUsage, stderr); return kExitUsage; }
        else if (input.empty()) input = a;
        else { std::fputs(kUsage, stderr); return kExitUsage; }
    }
    if (input.empty() || !(linear_rel > 0) || !(angular_deg > 0)) {
        std::fputs(kUsage, stderr);
        return kExitUsage;
    }

    // The JSON summary is the contract on stdout, and OCCT's readers print
    // progress chatter to stdout. Keep the real stdout for the summary alone
    // and point fd 1 at stderr for everything else.
    const int json_fd = dup(STDOUT_FILENO);
    std::fflush(stdout);
    dup2(STDERR_FILENO, STDOUT_FILENO);
    Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));

    Summary s;
    int code;
    try {
        code = run(input, mesh_out, linear_rel, angular_deg, parallel, s);
    } catch (const Standard_Failure& e) {
        s.error = std::string("OCCT: ") + e.GetMessageString();
        code = kExitFailed;
    } catch (const std::bad_alloc&) {
        s.error = "out of memory";
        code = kExitFailed;
    } catch (const std::exception& e) {
        s.error = e.what();
        code = kExitFailed;
    }

    const std::string json = to_json(s) + "\n";
    if (write(json_fd, json.data(), json.size()) < 0) return kExitFailed;
    return code;
}
