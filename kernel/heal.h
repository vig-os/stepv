// Transfer-time healing with a per-face time budget (#51).
//
// A STEP or IGES reader heals every shape it transfers with OCCT's FixShape
// operator. On a file written without p-curves (SolidWorks writes none)
// that means projecting every edge onto each face it bounds, and on a few
// pathological faces projection is slow: two ribbon faces of the Ultimaker 3
// assembly take 25 s each, at ~350 ms per edge, while their twins on surfaces
// of the same size take 1 s. One face must not hold up 62,000 good ones.
//
// So stepv's own FixShape stands in for OCCT's. It sets ShapeFix_Shape up
// from the same context parameters, but fixes the faces itself, one at a
// time, with a clock on each: once a face has spent its budget projecting,
// its fix is abandoned at the next edge (so a face overruns by at most one
// edge's projection). The face is taken out before the shell and solid fixes
// run, which cannot cope with a face that lacks p-curves, and comes back
// beside the healed shape as a loose face. It is recorded, and the mesher
// draws it as its outline (FaceStatus Missing) without the recovery ladder.
//
// The budget is STEPV_OCCT_FACE_BUDGET_MS (default 2000; 0 refuses any
// projection), read at the start of each run. It covers the fixes of faces;
// wires and edges outside any face are healed unbudgeted, as before.

#pragma once

#include <TopoDS_Shape.hxx>

#include <exception>

class ShapeFix_Shape;

namespace stepv {

// A budgeted fix ran out of time. Deliberately not a Standard_Failure: no
// OCCT handler catches it on its way out.
struct BudgetSpent : std::exception {
    const char* what() const noexcept override;
};

// Puts the budgeted FixShape in OCCT's operator registry. OCCT keeps the
// operator registered first, so this must run before any reader is made;
// later calls do nothing. Returns whether the budgeted operator is in effect.
bool install_face_budget();

// Starts a run: reads the budget from the environment and forgets the faces
// recorded by the run before.
void begin_face_budget();

// Whether `face` (any location, any orientation) was left unhealed by the
// budget in this run.
bool over_face_budget(const TopoDS_Shape& face);

// Gives `fix` one budget for its whole Perform(), which then throws
// BudgetSpent past it: for a ShapeFix outside the transfer (the recovery
// ladder's).
void budget(ShapeFix_Shape& fix);

}  // namespace stepv
