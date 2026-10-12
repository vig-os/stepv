// Transfer-time healing with a per-face time budget (#51): heal.h.

#include "heal.h"

#include <BRepBndLib.hxx>
#include <BRep_Builder.hxx>
#include <Bnd_Box.hxx>
#include <Message_ProgressScope.hxx>
#include <ShapeBuild_ReShape.hxx>
#include <ShapeConstruct_ProjectCurveOnSurface.hxx>
#include <ShapeExtend_MsgRegistrator.hxx>
#include <ShapeFix_Edge.hxx>
#include <ShapeFix_Face.hxx>
#include <ShapeFix_Shape.hxx>
#include <ShapeFix_Shell.hxx>
#include <ShapeFix_Solid.hxx>
#include <ShapeFix_Wire.hxx>
#include <ShapeProcess.hxx>
#include <ShapeProcess_Operator.hxx>
#include <ShapeProcess_ShapeContext.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_MapOfShape.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Iterator.hxx>

#include <cerrno>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <mutex>
#include <optional>
#include <set>
#include <string>
#include <vector>

namespace {

using Clock = std::chrono::steady_clock;

constexpr long kDefaultBudgetMs = 2000;

// The run's budget and the faces it left unhealed. Runs are serial (one file
// at a time per process, stepv-occt-core.cpp); the lock is for the reader's
// threads, should it ever use more than one.
struct State {
    std::mutex lock;
    Clock::duration budget = std::chrono::milliseconds(kDefaultBudgetMs);
    // STEPV_OCCT_UNHEALED: replay another run's decisions, not the clock's.
    std::optional<std::set<long>> replay;
    long next = 0;                 // the next face's ordinal
    std::vector<long> ordinals;    // of the faces left unhealed, in order
    TopTools_MapOfShape unhealed;  // located nowhere: one entry per TShape
};

State& state() {
    static State s;
    return s;
}

Clock::duration current_budget() {
    std::lock_guard<std::mutex> g(state().lock);
    return state().budget;
}

// OCCT's projector, with a clock. Armed, it counts the time spent projecting
// and, once that reaches the budget, refuses the next edge by throwing
// stepv::BudgetSpent. That is not a Standard_Failure, so no OCCT handler on
// the way out takes it for a failed projection: it unwinds the whole fix of
// the face, to whoever armed the clock. (Caught as a failed projection, it
// would leave the face without p-curves inside its solid, and ShapeFix_Solid's
// classification dereferences a missing p-curve.) Disarmed, it projects as
// OCCT's does.
class BudgetedProjection : public ShapeConstruct_ProjectCurveOnSurface {
public:
    explicit BudgetedProjection(Clock::duration budget) : budget_(budget) {}

    void arm() {
        armed_ = true;
        spent_ = Clock::duration::zero();
    }
    void disarm() { armed_ = false; }

    Standard_Boolean Perform(Handle(Geom_Curve)& c3d, const Standard_Real first,
                             const Standard_Real last, Handle(Geom2d_Curve)& c2d,
                             const Standard_Real tol_first, const Standard_Real tol_last) override {
        if (armed_ && spent_ >= budget_) throw stepv::BudgetSpent();
        // Charged even when the projection throws.
        struct Charge {
            Clock::duration& spent;
            Clock::time_point t0 = Clock::now();
            ~Charge() { spent += Clock::now() - t0; }
        } charge{spent_};
        return ShapeConstruct_ProjectCurveOnSurface::Perform(c3d, first, last, c2d, tol_first,
                                                             tol_last);
    }

private:
    Clock::duration budget_;
    Clock::duration spent_ = Clock::duration::zero();
    bool armed_ = false;
};

// ShapeFix_Edge's projector is a protected member with no setter; a member
// pointer formed in a derived class reaches it without a cast.
struct EdgeTool : ShapeFix_Edge {
    static void set_projector(ShapeFix_Edge& tool,
                              const Handle(ShapeConstruct_ProjectCurveOnSurface)& p) {
        tool.*(&EdgeTool::myProjector) = p;
    }
};

// ── ShapeFix_Shape from the shape-processing context ──
// The parameters OCCT's own FixShape operator reads from the reader's context
// (the "read.step" and "read.iges" resource sets, DE_ShapeFixParameters),
// each with the value it takes when the context has none. The list, its
// order and its defaults follow OCCT 7.9.3's fixshape() in
// ShapeProcess_OperLibrary.cxx, as does the operator below (one ReShape
// through two passes when tails are fixed, the history recorded the same
// way): this is derived from it, so a newer OCCT's list wants comparing.

template <class Tool>
struct IntMode {
    const char* key;
    Standard_Integer& (Tool::*mode)();
    Standard_Integer fallback;
};

template <class Tool>
struct BoolMode {
    const char* key;
    Standard_Boolean& (Tool::*mode)();
    Standard_Boolean fallback;
};

template <class Tool, std::size_t N>
void set(ShapeProcess_ShapeContext& ctx, Tool& tool, const IntMode<Tool> (&modes)[N]) {
    for (const auto& m : modes) (tool.*m.mode)() = ctx.IntegerVal(m.key, m.fallback);
}

template <class Tool, std::size_t N>
void set(ShapeProcess_ShapeContext& ctx, Tool& tool, const BoolMode<Tool> (&modes)[N]) {
    for (const auto& m : modes) (tool.*m.mode)() = ctx.BooleanVal(m.key, m.fallback);
}

const IntMode<ShapeFix_Shape> kShapeModes[] = {
    {"FixFreeShellMode", &ShapeFix_Shape::FixFreeShellMode, -1},
    {"FixFreeFaceMode", &ShapeFix_Shape::FixFreeFaceMode, -1},
    {"FixFreeWireMode", &ShapeFix_Shape::FixFreeWireMode, -1},
    {"FixSameParameterMode", &ShapeFix_Shape::FixSameParameterMode, -1},
    {"FixSolidMode", &ShapeFix_Shape::FixSolidMode, -1},
    {"FixVertexPositionMode", &ShapeFix_Shape::FixVertexPositionMode, 0},
    {"FixVertexToleranceMode", &ShapeFix_Shape::FixVertexTolMode, -1},
};
const IntMode<ShapeFix_Solid> kSolidModes[] = {
    {"FixShellMode", &ShapeFix_Solid::FixShellMode, -1},
    {"FixShellOrientationMode", &ShapeFix_Solid::FixShellOrientationMode, -1},
};
const BoolMode<ShapeFix_Solid> kSolidFlags[] = {
    {"CreateOpenSolidMode", &ShapeFix_Solid::CreateOpenSolidMode, Standard_True},
};
const IntMode<ShapeFix_Shell> kShellModes[] = {
    {"FixFaceMode", &ShapeFix_Shell::FixFaceMode, -1},
    {"FixFaceOrientationMode", &ShapeFix_Shell::FixOrientationMode, -1},
};
const IntMode<ShapeFix_Face> kFaceModes[] = {
    {"FixWireMode", &ShapeFix_Face::FixWireMode, -1},
    {"FixOrientationMode", &ShapeFix_Face::FixOrientationMode, -1},
    {"FixAddNaturalBoundMode", &ShapeFix_Face::FixAddNaturalBoundMode, -1},
    {"FixMissingSeamMode", &ShapeFix_Face::FixMissingSeamMode, -1},
    {"FixSmallAreaWireMode", &ShapeFix_Face::FixSmallAreaWireMode, -1},
    {"RemoveSmallAreaFaceMode", &ShapeFix_Face::RemoveSmallAreaFaceMode, -1},
    {"FixIntersectingWiresMode", &ShapeFix_Face::FixIntersectingWiresMode, -1},
    {"FixLoopWiresMode", &ShapeFix_Face::FixLoopWiresMode, -1},
    {"FixSplitFaceMode", &ShapeFix_Face::FixSplitFaceMode, -1},
};
const BoolMode<ShapeFix_Wire> kWireFlags[] = {
    {"ModifyTopologyMode", &ShapeFix_Wire::ModifyTopologyMode, Standard_False},
    {"ModifyGeometryMode", &ShapeFix_Wire::ModifyGeometryMode, Standard_True},
    {"ClosedWireMode", &ShapeFix_Wire::ClosedWireMode, Standard_True},
    {"PreferencePCurveMode", &ShapeFix_Wire::PreferencePCurveMode, Standard_True},
};
const IntMode<ShapeFix_Wire> kWireModes[] = {
    {"FixReorderMode", &ShapeFix_Wire::FixReorderMode, -1},
    {"FixSmallMode", &ShapeFix_Wire::FixSmallMode, -1},
    {"FixConnectedMode", &ShapeFix_Wire::FixConnectedMode, -1},
    {"FixEdgeCurvesMode", &ShapeFix_Wire::FixEdgeCurvesMode, -1},
    {"FixDegeneratedMode", &ShapeFix_Wire::FixDegeneratedMode, -1},
    {"FixLackingMode", &ShapeFix_Wire::FixLackingMode, -1},
    {"FixSelfIntersectionMode", &ShapeFix_Wire::FixSelfIntersectionMode, -1},
    {"RemoveLoopMode", &ShapeFix_Wire::ModifyRemoveLoopMode, -1},
    {"FixReversed2dMode", &ShapeFix_Wire::FixReversed2dMode, -1},
    {"FixRemovePCurveMode", &ShapeFix_Wire::FixRemovePCurveMode, -1},
    {"FixRemoveCurve3dMode", &ShapeFix_Wire::FixRemoveCurve3dMode, -1},
    {"FixAddPCurveMode", &ShapeFix_Wire::FixAddPCurveMode, -1},
    {"FixAddCurve3dMode", &ShapeFix_Wire::FixAddCurve3dMode, -1},
    {"FixShiftedMode", &ShapeFix_Wire::FixShiftedMode, -1},
    {"FixSeamMode", &ShapeFix_Wire::FixSeamMode, -1},
    {"FixEdgeSameParameterMode", &ShapeFix_Wire::FixSameParameterMode, -1},
    {"FixNotchedEdgesMode", &ShapeFix_Wire::FixNotchedEdgesMode, -1},
    {"FixTailMode", &ShapeFix_Wire::FixTailMode, 0},
    {"FixSelfIntersectingEdgeMode", &ShapeFix_Wire::FixSelfIntersectingEdgeMode, -1},
    {"FixIntersectingEdgesMode", &ShapeFix_Wire::FixIntersectingEdgesMode, -1},
    {"FixNonAdjacentIntersectingEdgesMode", &ShapeFix_Wire::FixNonAdjacentIntersectingEdgesMode,
     -1},
};

void configure(ShapeProcess_ShapeContext& ctx, ShapeFix_Shape& fix) {
    fix.SetPrecision(ctx.RealVal("Tolerance3d", Precision::Confusion()));
    fix.SetMinTolerance(ctx.RealVal("MinTolerance3d", Precision::Confusion()));
    fix.SetMaxTolerance(ctx.RealVal("MaxTolerance3d", Precision::Confusion()));
    set(ctx, fix, kShapeModes);
    set(ctx, *fix.FixSolidTool(), kSolidModes);
    set(ctx, *fix.FixSolidTool(), kSolidFlags);
    set(ctx, *fix.FixShellTool(), kShellModes);
    fix.FixShellTool()->SetNonManifoldFlag(ctx.IsNonManifold());
    set(ctx, *fix.FixFaceTool(), kFaceModes);
    ShapeFix_Wire& wire = *fix.FixWireTool();
    set(ctx, wire, kWireFlags);
    set(ctx, wire, kWireModes);
    wire.SetMaxTailAngle(ctx.RealVal("MaxTailAngle", 0) * (M_PI / 180));
    wire.SetMaxTailWidth(ctx.RealVal("MaxTailWidth", -1));
}


// ShapeFix_Root::NeedFix: a mode below zero means "the default", here on.
bool on(Standard_Integer mode) { return mode != 0; }

// Fixes the faces of `shape` one at a time, as ShapeFix_Shape and
// ShapeFix_Shell would, with `projector` armed for each: shell by shell, each
// with the context applied as its turn comes, then the free faces with the
// two modes ShapeFix_Shape sets for one. A face whose clock runs out is taken
// out of the shape through `reshape` and appended to `unhealed`. Returns
// false when the progress was cancelled.
//
// Faces in shells are fixed when the solid, shell and face modes all allow it:
// ShapeFix_Shape asks the same of a shell inside a solid (and of a free shell,
// FixFreeShellMode instead of the solid's two, a difference only when a reader
// turns one off and not the other).
bool fix_faces(ShapeFix_Shape& fix, BudgetedProjection& projector,
               const Handle(ShapeBuild_ReShape)& reshape, const TopoDS_Shape& shape,
               std::vector<TopoDS_Face>& unhealed, const Message_ProgressRange& progress) {
    const Handle(ShapeFix_Face) face_fix = fix.FixFaceTool();
    face_fix->SetContext(reshape);
    const bool in_shells = on(fix.FixSolidMode()) && on(fix.FixSolidTool()->FixShellMode()) &&
                           on(fix.FixShellTool()->FixFaceMode());
    const bool free_faces = on(fix.FixFreeFaceMode());
    TopTools_MapOfShape distinct;
    for (TopExp_Explorer f(shape, TopAbs_FACE); f.More(); f.Next())
        distinct.Add(f.Current().Located(TopLoc_Location()));
    Message_ProgressScope scope(progress, "Fixing face", distinct.Extent());

    TopTools_MapOfShape seen;  // a face shared by two shells is fixed once
    auto fix_one = [&](const TopoDS_Shape& f) {
        // A face an earlier fix removed, or split into several.
        if (f.IsNull() || f.ShapeType() != TopAbs_FACE) return;
        if (!seen.Add(f.Located(TopLoc_Location()))) return;
        const TopoDS_Face& face = TopoDS::Face(f);
        scope.Next();
        // Faces are numbered in the order they are fixed, the same in every
        // run of the same file: what a replay names.
        long ordinal;
        std::optional<bool> replayed;  // set: whether the replayed run gave up on it
        {
            std::lock_guard<std::mutex> g(state().lock);
            ordinal = state().next++;
            if (state().replay) replayed = state().replay->count(ordinal) > 0;
        }
        auto give_up = [&] {
            reshape->Remove(face);
            unhealed.push_back(face);
            std::lock_guard<std::mutex> g(state().lock);
            state().ordinals.push_back(ordinal);
        };
        if (replayed.value_or(false)) {
            give_up();
            return;
        }
        if (!replayed) projector.arm();
        try {
            face_fix->Init(face);
            face_fix->Perform();
        } catch (const stepv::BudgetSpent&) {
            give_up();
        }
        projector.disarm();
    };
    if (in_shells) {
        for (TopExp_Explorer sh(shape, TopAbs_SHELL); sh.More(); sh.Next()) {
            for (TopoDS_Iterator it(reshape->Apply(sh.Current())); it.More(); it.Next()) {
                if (!scope.More()) return false;
                fix_one(it.Value());
            }
        }
    }
    if (free_faces) {
        ShapeFix_Wire& wire = *face_fix->FixWireTool();
        const Standard_Boolean topology = wire.ModifyTopologyMode();
        const Standard_Integer small = face_fix->FixSmallAreaWireMode();
        wire.ModifyTopologyMode() = Standard_True;
        if (small == -1) face_fix->FixSmallAreaWireMode() = Standard_True;
        for (TopExp_Explorer f(shape, TopAbs_FACE, TopAbs_SHELL); f.More() && scope.More(); f.Next())
            fix_one(reshape->Apply(f.Current()));
        wire.ModifyTopologyMode() = topology;
        face_fix->FixSmallAreaWireMode() = small;
    }
    return scope.More();
}

// The FixShape operator, budgeted: the faces first (fix_faces), then
// ShapeFix_Shape with face fixing off for everything else, on a shape with
// no unhealed face in it. The unhealed faces come back as loose faces beside
// it, recorded, so the mesh draws their outlines and the topology counts them.
class BudgetedFixShape : public ShapeProcess_Operator {
public:
    Standard_Boolean Perform(const Handle(ShapeProcess_Context)& context,
                             const Message_ProgressRange& progress) override {
        Handle(ShapeProcess_ShapeContext) ctx = Handle(ShapeProcess_ShapeContext)::DownCast(context);
        if (ctx.IsNull()) return Standard_False;
        // Messages are collected only when the context asks for them.
        Handle(ShapeExtend_MsgRegistrator) msg;
        if (!ctx->Messages().IsNull()) msg = new ShapeExtend_MsgRegistrator;
        Handle(ShapeFix_Shape) fix = new ShapeFix_Shape;
        fix->SetMsgRegistrator(msg);
        configure(*ctx, *fix);
        Handle(BudgetedProjection) projector = new BudgetedProjection(current_budget());
        EdgeTool::set_projector(*fix->FixEdgeTool(), projector);
        // One context through every phase and pass, as ShapeFix_Shape keeps
        // its own across Init().
        Handle(ShapeBuild_ReShape) reshape = new ShapeBuild_ReShape;
        reshape->ModeConsiderLocation() = Standard_True;
        fix->SetContext(reshape);

        std::vector<TopoDS_Face> unhealed;
        Handle(ShapeFix_Wire) wire = fix->FixWireTool();
        const bool tails = wire->FixTailMode() == 1;
        Message_ProgressScope scope(progress, nullptr, tails ? 4 : 2);
        // The faces are fixed in the first pass only: OCCT's second, tail
        // pass finds the shape already fixed (ShapeFix_Shape keeps what it
        // fixed across Init) and returns at once, and so does ours.
        bool faces_fixed = false;
        auto pass = [&]() {
            const Message_ProgressRange faces = scope.Next();
            if (!faces_fixed && !fix_faces(*fix, *projector, reshape, ctx->Result(), unhealed, faces))
                return false;
            faces_fixed = true;
            Standard_Integer& in_shells = fix->FixShellTool()->FixFaceMode();
            Standard_Integer& loose = fix->FixFreeFaceMode();
            const Standard_Integer saved_in_shells = in_shells, saved_loose = loose;
            in_shells = loose = 0;
            fix->Init(ctx->Result());
            fix->Perform(scope.Next());
            in_shells = saved_in_shells;
            loose = saved_loose;
            if (scope.UserBreak()) return false;
            const TopoDS_Shape result = fix->Shape();
            if (result != ctx->Result() || (!msg.IsNull() && !msg->MapShape().IsEmpty())) {
                ctx->RecordModification(fix->Context(), msg);
                ctx->SetResult(result);
            }
            return true;
        };
        if (tails) {
            wire->FixTailMode() = 0;
            const bool ok = pass();
            wire->FixTailMode() = 1;
            if (!ok) return Standard_False;
        }
        if (!pass()) return Standard_False;
        if (!unhealed.empty()) {
            // The history maps each unhealed face to nothing, since it was
            // taken out; it is back, so map it to itself, and the reader
            // finds its colour and name (XSAlgo_ShapeProcessor reads this
            // map into the transfer's binders). The map is the context's own
            // member, only handed out const.
            auto& history = const_cast<TopTools_DataMapOfShapeShape&>(ctx->Map());
            for (TopTools_DataMapOfShapeShape::Iterator it(history); it.More(); it.Next()) {
                if (!it.Value().IsNull()) continue;
                for (const TopoDS_Face& f : unhealed)
                    if (it.Key().TShape() == f.TShape())
                        it.ChangeValue() = f.Located(it.Key().Location()).Oriented(it.Key().Orientation());
            }
            BRep_Builder b;
            TopoDS_Compound with;
            b.MakeCompound(with);
            b.Add(with, ctx->Result());
            std::lock_guard<std::mutex> g(state().lock);
            for (const TopoDS_Face& f : unhealed) {
                b.Add(with, f);
                state().unhealed.Add(f.Located(TopLoc_Location()));
            }
            ctx->SetResult(with);
        }
        return Standard_True;
    }
};

}  // namespace

namespace stepv {

const char* BudgetSpent::what() const noexcept { return "stepv: the face's healing budget is spent"; }

bool install_face_budget() {
    static const bool installed = [] {
        Handle(ShapeProcess_Operator) ours = new BudgetedFixShape;
        ShapeProcess::RegisterOperator("FixShape", ours);
        Handle(ShapeProcess_Operator) found;
        return ShapeProcess::FindOperator("FixShape", found) && found == ours;
    }();
    return installed;
}

void begin_face_budget() {
    std::optional<std::set<long>> replay;
    if (const char* v = std::getenv("STEPV_OCCT_UNHEALED")) {
        replay.emplace();
        for (const char* p = v; *p;) {
            char* end = nullptr;
            const long n = std::strtol(p, &end, 10);
            if (end == p) break;  // not a number: the list ends here
            replay->insert(n);
            p = *end == ',' ? end + 1 : end;
        }
    }
    long ms = kDefaultBudgetMs;
    if (const char* v = std::getenv("STEPV_OCCT_FACE_BUDGET_MS"); v && *v) {
        char* end = nullptr;
        errno = 0;
        const long n = std::strtol(v, &end, 10);
        if (*end == '\0' && errno == 0 && n >= 0) ms = n;
    }
    std::lock_guard<std::mutex> g(state().lock);
    state().budget = std::chrono::milliseconds(ms);
    state().replay = std::move(replay);
    state().next = 0;
    state().ordinals.clear();
    state().unhealed.Clear();
}

std::vector<long> unhealed_faces() {
    std::lock_guard<std::mutex> g(state().lock);
    return state().ordinals;
}

void add_bounds(const TopoDS_Shape& shape, Bnd_Box& box) {
    bool any = false;
    {
        std::lock_guard<std::mutex> g(state().lock);
        for (TopExp_Explorer f(shape, TopAbs_FACE); f.More() && !any; f.Next())
            any = state().unhealed.Contains(f.Current().Located(TopLoc_Location()));
    }
    if (!any) {
        BRepBndLib::Add(shape, box, false);
        return;
    }
    // Without p-curves a face's extent on its surface is unknown, and the
    // box opens out to the whole surface (infinite for a cylinder): bound
    // such a face by its edges instead.
    for (TopExp_Explorer f(shape, TopAbs_FACE); f.More(); f.Next()) {
        if (over_face_budget(f.Current())) {
            for (TopExp_Explorer e(f.Current(), TopAbs_EDGE); e.More(); e.Next())
                BRepBndLib::Add(e.Current(), box, false);
        } else {
            BRepBndLib::Add(f.Current(), box, false);
        }
    }
    for (TopExp_Explorer e(shape, TopAbs_EDGE, TopAbs_FACE); e.More(); e.Next())
        BRepBndLib::Add(e.Current(), box, false);
    for (TopExp_Explorer v(shape, TopAbs_VERTEX, TopAbs_EDGE); v.More(); v.Next())
        BRepBndLib::Add(v.Current(), box, false);
}

bool over_face_budget(const TopoDS_Shape& face) {
    std::lock_guard<std::mutex> g(state().lock);
    return state().unhealed.Contains(face.Located(TopLoc_Location()));
}

void budget(ShapeFix_Shape& fix) {
    Handle(BudgetedProjection) p = new BudgetedProjection(current_budget());
    p->arm();
    EdgeTool::set_projector(*fix.FixEdgeTool(), p);
}

}  // namespace stepv
