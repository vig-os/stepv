// Measurements between B-rep entities (#33). The protocol is in measure.h.
#include "measure.h"

#include "json.h"
#include "topology.h"

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepLProp_SLProps.hxx>
#include <BRep_Tool.hxx>
#include <Standard_Failure.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <gp_Ax1.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>

#include <chrono>
#include <cmath>
#include <cstdlib>
#include <map>
#include <memory>
#include <sstream>
#include <thread>
#include <vector>

namespace stepv {
namespace {

// ── A small JSON reader: enough for the queries, strict about the rest ────

struct Json {
    enum Kind { Null, Bool, Number, String, Array, Object } kind = Null;
    double number = 0;
    bool boolean = false;
    std::string string;
    std::vector<Json> array;
    std::map<std::string, Json> object;

    const Json* get(const std::string& key) const {
        if (kind != Object) return nullptr;
        const auto it = object.find(key);
        return it == object.end() ? nullptr : &it->second;
    }
};

struct Parser {
    const std::string& s;
    std::size_t i = 0;
    int depth = 0;

    void ws() {
        while (i < s.size() && (s[i] == ' ' || s[i] == '\t' || s[i] == '\r' || s[i] == '\n')) ++i;
    }
    bool eat(char c) {
        ws();
        if (i < s.size() && s[i] == c) {
            ++i;
            return true;
        }
        return false;
    }
    bool literal(const char* w) {
        const std::size_t n = std::char_traits<char>::length(w);
        if (s.compare(i, n, w) != 0) return false;
        i += n;
        return true;
    }
    bool str(std::string& out) {
        if (!eat('"')) return false;
        while (i < s.size() && s[i] != '"') {
            if (s[i] == '\\') {
                if (++i >= s.size()) return false;
                const char e = s[i];
                if (e == 'u') return false;  // no query needs it
                out += e == 'n' ? '\n' : e == 't' ? '\t' : e;
            } else {
                out += s[i];
            }
            ++i;
        }
        return i < s.size() && s[i++] == '"';
    }
    bool value(Json& v) {
        if (++depth > 16) return false;
        ws();
        if (i >= s.size()) return false;
        const char c = s[i];
        bool ok = true;
        if (c == '{') {
            ++i;
            v.kind = Json::Object;
            if (!eat('}')) {
                do {
                    std::string k;
                    Json item;
                    if (!str(k) || !eat(':') || !value(item)) return false;
                    v.object[k] = std::move(item);
                } while (eat(','));
                ok = eat('}');
            }
        } else if (c == '[') {
            ++i;
            v.kind = Json::Array;
            if (!eat(']')) {
                do {
                    Json item;
                    if (!value(item)) return false;
                    v.array.push_back(std::move(item));
                } while (eat(','));
                ok = eat(']');
            }
        } else if (c == '"') {
            v.kind = Json::String;
            ok = str(v.string);
        } else if (literal("true") || literal("false")) {
            v.kind = Json::Bool;
            v.boolean = s[i - 1] == 'e' && s[i - 2] == 'u';
        } else if (literal("null")) {
            v.kind = Json::Null;
        } else {
            char* end = nullptr;
            v.kind = Json::Number;
            v.number = std::strtod(s.c_str() + i, &end);
            ok = end && end != s.c_str() + i && std::isfinite(v.number);
            if (ok) i = static_cast<std::size_t>(end - s.c_str());
        }
        --depth;
        return ok;
    }
};

bool parse(const std::string& line, Json& out) {
    Parser p{line};
    if (!p.value(out)) return false;
    p.ws();
    return p.i == line.size() && out.kind == Json::Object;
}

// ── Answers ───────────────────────────────────────────────────────────────

std::string num(double v) {
    std::ostringstream o;
    o.precision(17);
    o << (std::isfinite(v) ? v : 0.0);
    return o.str();
}

std::string pnt(const gp_Pnt& p) {
    return "[" + num(p.X()) + "," + num(p.Y()) + "," + num(p.Z()) + "]";
}

std::string vec(const gp_Dir& d) {
    return "[" + num(d.X()) + "," + num(d.Y()) + "," + num(d.Z()) + "]";
}

std::string error(const std::string& id, const std::string& what) {
    return "{\"id\":" + id + ",\"ok\":false,\"error\":\"" + json_escape(what) + "\"}";
}

// An entity of the query: {"part": p, "face": f} or {"part": p, "edge": e}.
std::optional<TopoDS_Shape> entity(const Json* j, const Resolve& resolve, std::string& why) {
    const Json* part = j ? j->get("part") : nullptr;
    const Json* face = j ? j->get("face") : nullptr;
    const Json* edge = j ? j->get("edge") : nullptr;
    const auto index = [](const Json* n) {
        return n && n->kind == Json::Number && n->number >= 0 && n->number < 1e12 &&
               std::floor(n->number) == n->number;
    };
    if (!index(part) || (index(face) == index(edge))) {
        why = "an entity is {\"part\": p, \"face\": f} or {\"part\": p, \"edge\": e}";
        return std::nullopt;
    }
    const bool is_edge = index(edge);
    auto shape = resolve(static_cast<long>(part->number), is_edge,
                         static_cast<long>((is_edge ? edge : face)->number));
    if (!shape) why = "no such entity";
    return shape;
}

// An entity's axis (see measure.h). `plane` says it is a plane's normal:
// a direction with a meaning (outward), not just a line.
struct Axis {
    gp_Ax1 line;
    bool plane = false;
};

std::optional<Axis> axis_of(const TopoDS_Shape& s) {
    if (s.ShapeType() == TopAbs_FACE) {
        const TopoDS_Face& f = TopoDS::Face(s);
        BRepAdaptor_Surface a(f);
        switch (a.GetType()) {
        case GeomAbs_Plane: {
            const gp_Pln pl = a.Plane();
            return Axis{gp_Ax1(pl.Location(), outward_normal(pl, f)), true};
        }
        case GeomAbs_Cylinder: return Axis{a.Cylinder().Axis()};
        case GeomAbs_Cone: return Axis{a.Cone().Axis()};
        default: return std::nullopt;
        }
    }
    if (s.ShapeType() == TopAbs_EDGE) {
        BRepAdaptor_Curve c(TopoDS::Edge(s));
        switch (c.GetType()) {
        case GeomAbs_Line: return Axis{c.Line().Position()};
        case GeomAbs_Circle: return Axis{c.Circle().Axis()};
        case GeomAbs_Ellipse: return Axis{c.Ellipse().Axis()};
        default: return std::nullopt;
        }
    }
    return std::nullopt;
}

// The distance between two lines: parallel, the distance from one to a
// point of the other; else the common perpendicular's length.
double line_distance(const gp_Ax1& a, const gp_Ax1& b) {
    const gp_Vec w(a.Location(), b.Location());
    const gp_Vec da(a.Direction()), db(b.Direction());
    const gp_Vec n = da.Crossed(db);
    if (n.Magnitude() < 1e-9) return w.Crossed(da).Magnitude();
    return std::abs(w.Dot(n)) / n.Magnitude();
}

std::string distance(const std::string& id, const TopoDS_Shape& a, const TopoDS_Shape& b) {
    BRepExtrema_DistShapeShape d(a, b);
    if (!d.IsDone() || d.NbSolution() < 1) return error(id, "no distance found");
    std::string out = "{\"id\":" + id + ",\"ok\":true,\"distance\":" + num(d.Value()) +
                      ",\"points\":[" + pnt(d.PointOnShape1(1)) + "," + pnt(d.PointOnShape2(1)) +
                      "]";
    const auto aa = axis_of(a), ab = axis_of(b);
    if (aa && ab && !aa->plane && !ab->plane)
        out += ",\"axis_distance\":" + num(line_distance(aa->line, ab->line));
    return out + "}";
}

std::string angle(const std::string& id, const TopoDS_Shape& a, const TopoDS_Shape& b) {
    const auto aa = axis_of(a), ab = axis_of(b);
    if (!aa || !ab) return error(id, "an angle needs planes, cylinders, cones, lines or circles");
    const double c = aa->line.Direction().Dot(ab->line.Direction());
    double deg;
    if (aa->plane && ab->plane) {
        // Between outward normals: two faces of a box meet at 90, a face and
        // its opposite at 180.
        deg = std::acos(std::clamp(c, -1.0, 1.0)) * 180.0 / M_PI;
    } else if (aa->plane != ab->plane) {
        // A line against a plane: 0 when it lies in it, 90 along its normal.
        deg = std::asin(std::clamp(std::abs(c), 0.0, 1.0)) * 180.0 / M_PI;
    } else {
        // Two lines have no direction: the acute angle.
        deg = std::acos(std::clamp(std::abs(c), 0.0, 1.0)) * 180.0 / M_PI;
    }
    return "{\"id\":" + id + ",\"ok\":true,\"angle_deg\":" + num(deg) + "}";
}

std::string point(const std::string& id, const TopoDS_Shape& a, const Json* near) {
    if (!near || near->kind != Json::Array || near->array.size() != 3)
        return error(id, "point needs \"near\": [x, y, z]");
    double xyz[3];
    for (int k = 0; k < 3; ++k) {
        if (near->array[k].kind != Json::Number) return error(id, "near: numbers");
        xyz[k] = near->array[k].number;
    }
    const TopoDS_Shape v = BRepBuilderAPI_MakeVertex(gp_Pnt(xyz[0], xyz[1], xyz[2])).Shape();
    BRepExtrema_DistShapeShape d(v, a);
    if (!d.IsDone() || d.NbSolution() < 1) return error(id, "no point found");
    const gp_Pnt p = d.PointOnShape2(1);
    std::string out = "{\"id\":" + id + ",\"ok\":true,\"point\":" + pnt(p);
    if (a.ShapeType() == TopAbs_FACE && d.SupportTypeShape2(1) == BRepExtrema_IsInFace) {
        const TopoDS_Face& f = TopoDS::Face(a);
        Standard_Real u, w;
        d.ParOnFaceS2(1, u, w);
        BRepAdaptor_Surface s(f);
        BRepLProp_SLProps props(s, u, w, 1, 1e-9);
        if (props.IsNormalDefined()) {
            gp_Dir n = props.Normal();
            if (f.Orientation() == TopAbs_REVERSED) n.Reverse();
            out += ",\"normal\":" + vec(n);
        }
    }
    return out + "}";
}

}  // namespace

std::string answer_query(const std::string& line, const Resolve& resolve) {
    Json q;
    if (!parse(line, q)) return error("null", "not a JSON object");
    const Json* idj = q.get("id");
    const std::string id = idj && idj->kind == Json::Number ? num(idj->number) : "null";
    const Json* op = q.get("op");
    if (!op || op->kind != Json::String) return error(id, "no \"op\"");
    try {
        if (op->string == "ping") return "{\"id\":" + id + ",\"ok\":true}";
        // A test hook (tests/measure.rs): a query that takes `ms`, so the
        // caller's per-query time limit has something to catch.
        if (op->string == "test_sleep" && std::getenv("STEPV_OCCT_TEST_HOOKS")) {
            const Json* ms = q.get("ms");
            std::this_thread::sleep_for(std::chrono::milliseconds(
                ms && ms->kind == Json::Number ? static_cast<long>(ms->number) : 0));
            return "{\"id\":" + id + ",\"ok\":true}";
        }
        std::string why;
        const auto a = entity(q.get("a"), resolve, why);
        if (!a) return error(id, "a: " + why);
        if (op->string == "point") return point(id, *a, q.get("near"));
        const auto b = entity(q.get("b"), resolve, why);
        if (!b) return error(id, "b: " + why);
        if (op->string == "distance") return distance(id, *a, *b);
        if (op->string == "angle") return angle(id, *a, *b);
        return error(id, "unknown op: " + op->string);
    } catch (const Standard_Failure& e) {
        return error(id, std::string("OCCT: ") + e.GetMessageString());
    } catch (const std::exception& e) {
        return error(id, e.what());
    }
}

}  // namespace stepv
