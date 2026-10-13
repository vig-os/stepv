// occt-patch-check — checks the OCCT stepv links against carries the #74
// patch (nix/patches/occt-7.9.3-fixselfintersectwire-hoist.patch), through
// OCCT's public API only. Run by tests/occt_patch.rs; not shipped.
//
//   occt-patch-check scaling <n>   CPU seconds of FixSelfIntersectWire on a
//                                  convex n-gon and on a 4n-gon, and the
//                                  growth exponent between them
//   occt-patch-check wires         one canonical line per generated wire:
//                                  what FixSelfIntersectWire did to it
//
// `scaling`: on a convex polygon no two non-adjacent edges' boxes overlap, so
// every one of the ~n^2/2 edge pairs costs the same constant, plus, without
// the patch, an O(n) walk of the wire's NCollection_Sequence (it caches only
// the last position, and the loop fetched Edge(num1) and Edge(num2) in turn).
// Unpatched the time grows as n^3, patched as n^2. The exponent is a ratio of
// two CPU times of the same process, so a slower or loaded machine scales
// both and leaves it alone; the test's threshold sits halfway between 2 and 3.
//
// `wires`: random closed polylines on a small grid, the generator of the
// patch's correctness gate (issue #74), with a fixed PRNG so every platform
// builds the same wires. Its seeds include wires that reach the big-segment
// branch (NbRemoved > 0), the one place the patch re-fetches edge1, and that a
// hoist without the re-fetch gets wrong. The test compares the output with
// tests/occt-patch/wires.txt, recorded with the UNPATCHED OCCT 7.9.3: the
// patch must change nothing but the time.

#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepLib.hxx>
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <GCE2d_MakeSegment.hxx>
#include <Geom2d_Curve.hxx>
#include <Geom_Plane.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <ShapeAnalysis_Edge.hxx>
#include <ShapeBuild_ReShape.hxx>
#include <ShapeExtend_WireData.hxx>
#include <ShapeFix_IntersectionTool.hxx>
#include <Standard_Failure.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Iterator.hxx>
#include <TopoDS_Wire.hxx>

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <string>
#include <vector>

namespace {

// A face on the XY plane bounded by the closed polyline P, one straight edge
// per side, each with its p-curve. `tol` gives each vertex's tolerance.
TopoDS_Face polygon_face(const std::vector<gp_Pnt2d>& P, const std::vector<double>& tol,
                         const std::vector<bool>& same_param) {
    Handle(Geom_Plane) plane = new Geom_Plane(gp::XOY());
    BRep_Builder B;
    std::vector<TopoDS_Vertex> V(P.size());
    for (size_t i = 0; i < P.size(); ++i)
        B.MakeVertex(V[i], gp_Pnt(P[i].X(), P[i].Y(), 0), tol[i]);
    TopoDS_Wire W;
    B.MakeWire(W);
    for (size_t i = 0; i < P.size(); ++i) {
        const size_t j = (i + 1) % P.size();
        Handle(Geom2d_Curve) c = GCE2d_MakeSegment(P[i], P[j]).Value();
        BRepBuilderAPI_MakeEdge me(c, plane, V[i], V[j], c->FirstParameter(), c->LastParameter());
        if (!me.IsDone()) {
            std::fprintf(stderr, "occt-patch-check: cannot make edge %zu\n", i);
            std::exit(1);
        }
        TopoDS_Edge e = me.Edge();
        BRepLib::BuildCurves3d(e);
        if (!same_param[i]) B.SameParameter(e, Standard_False);
        B.Add(W, e);
    }
    TopoDS_Face F;
    B.MakeFace(F, plane, 1e-7);
    B.Add(F, W);
    return F;
}

TopoDS_Wire first_wire(const TopoDS_Face& f) {
    TopoDS_Iterator it(f);
    return TopoDS::Wire(it.Value());
}

double cpu_seconds() {
    timespec ts{};
    clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &ts);
    return double(ts.tv_sec) + double(ts.tv_nsec) * 1e-9;
}

// CPU seconds of one FixSelfIntersectWire on a convex n-gon: the best of
// three runs, so a single preempted run cannot inflate it.
double fix_ngon_seconds(int n) {
    std::vector<gp_Pnt2d> P(n);
    for (int i = 0; i < n; ++i) {
        const double a = 2 * M_PI * i / n;
        P[i] = gp_Pnt2d(1000 * std::cos(a), 1000 * std::sin(a));
    }
    const TopoDS_Face face = polygon_face(P, std::vector<double>(n, 1e-7), std::vector<bool>(n, true));
    double best = 1e300;
    for (int rep = 0; rep < 3; ++rep) {
        Handle(ShapeExtend_WireData) sewd = new ShapeExtend_WireData(first_wire(face));
        ShapeFix_IntersectionTool tool(new ShapeBuild_ReShape, 1e-7, 1.0);
        Standard_Integer split = 0, cut = 0, removed = 0;
        const double t0 = cpu_seconds();
        tool.FixSelfIntersectWire(sewd, face, split, cut, removed);
        const double t = cpu_seconds() - t0;
        if (split || cut || removed || sewd->NbEdges() != n) {
            std::fprintf(stderr, "occt-patch-check: the %d-gon was modified\n", n);
            std::exit(1);
        }
        if (t < best) best = t;
    }
    return best;
}

int scaling(int n) {
    const double t1 = fix_ngon_seconds(n), t4 = fix_ngon_seconds(4 * n);
    std::printf("{\"n\":%d,\"cpu_s\":%.6f,\"cpu_s_4n\":%.6f,\"exponent\":%.3f}\n", n, t1, t4,
                std::log(t4 / t1) / std::log(4.0));
    return 0;
}

// splitmix64: the same sequence on every platform, unlike std::rand.
struct Rng {
    uint64_t s;
    uint32_t next() {
        uint64_t z = (s += 0x9E3779B97F4A7C15ULL);
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
        return uint32_t((z ^ (z >> 31)) >> 32);
    }
    int below(int k) { return int(next() % uint32_t(k)); }
};

// One line: the tool's result and counts, then every edge of the fixed wire
// as its two vertices (6 significant digits, so last-bit differences between
// compilers or libms cannot show).
std::string describe(const TopoDS_Face& face) {
    Handle(ShapeExtend_WireData) sewd = new ShapeExtend_WireData(first_wire(face));
    ShapeFix_IntersectionTool tool(new ShapeBuild_ReShape, 1e-5, 1.0);
    Standard_Integer split = 0, cut = 0, removed = 0;
    std::string s;
    char b[160];
    try {
        const bool r = tool.FixSelfIntersectWire(sewd, face, split, cut, removed);
        std::snprintf(b, sizeof b, "r%d s%d c%d rm%d n%d |", int(r), int(split), int(cut), int(removed),
                      int(sewd->NbEdges()));
        s += b;
    } catch (const Standard_Failure& e) {
        return std::string("throw ") + e.GetMessageString();
    }
    ShapeAnalysis_Edge sae;
    for (int i = 1; i <= sewd->NbEdges(); ++i) {
        const TopoDS_Edge e = sewd->Edge(i);
        const gp_Pnt p = BRep_Tool::Pnt(sae.FirstVertex(e)), q = BRep_Tool::Pnt(sae.LastVertex(e));
        std::snprintf(b, sizeof b, " %.6g,%.6g>%.6g,%.6g", p.X() + 0.0, p.Y() + 0.0, q.X() + 0.0, q.Y() + 0.0);
        s += b;
    }
    return s;
}

// Seeds 0..199: 91 of these wires reach the big-segment branch, and a hoist
// without the re-fetch changes the result of 39 of them.
int wires() {
    const double tols[] = {1e-7, 1e-4, 1e-3, 1e-2, 0.05};
    for (int sd = 0; sd < 200; ++sd) {
        Rng rng{uint64_t(sd) * 7919 + 13};
        const int L = 4 + rng.below(24), G = 4;
        std::vector<gp_Pnt2d> P;
        while (int(P.size()) < L) {
            // Two statements, not two arguments: C++ leaves argument order
            // unspecified, and GCC draws y first where clang draws x.
            const int x = rng.below(G + 1);
            const int y = rng.below(G + 1);
            gp_Pnt2d q(x, y);
            if (rng.below(4) == 0) q.SetX(q.X() + 0.5);
            if (!P.empty() && P.back().Distance(q) < 1e-9) continue;
            P.push_back(q);
        }
        if (P.front().Distance(P.back()) < 1e-9) P.pop_back();
        if (P.size() < 3) continue;
        std::vector<double> tol(P.size());
        std::vector<bool> same(P.size());
        for (size_t i = 0; i < P.size(); ++i) tol[i] = tols[rng.below(5)];
        for (size_t i = 0; i < P.size(); ++i) same[i] = rng.below(3) != 0;
        std::printf("%d %s\n", sd, describe(polygon_face(P, tol, same)).c_str());
    }
    return 0;
}

}  // namespace

int main(int argc, char** argv) {
    Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
    if (argc == 3 && !std::strcmp(argv[1], "scaling")) return scaling(std::atoi(argv[2]));
    if (argc == 2 && !std::strcmp(argv[1], "wires")) return wires();
    std::fputs("usage: occt-patch-check scaling <n> | wires\n", stderr);
    return 2;
}
