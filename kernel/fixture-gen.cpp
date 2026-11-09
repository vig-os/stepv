// fixture-gen — writes the small, committed test corpus in tests/data/.
//
// Everything in tests/fixtures/ is fetched and gitignored (some of it is
// redistribution-restricted); these files are ours, small, and deterministic
// in content, so the CLI integration tests and `nix flake check` can run with
// no network. Regenerate with `just test-data`.
//
//   assembly.step  an assembly: a plate with a hole (blue, top face red) and
//                  two instances of one pin (orange), all named
//   no-pcurves.step  the same assembly with no p-curves: every edge only
//                  in 3D, as SolidWorks writes them (#51), so the reader's
//                  healing must project each onto its faces. The pins'
//                  cylindrical face is green, a colour on a face that needs it
//   multifile/     the same assembly as a multi-file STEP: bracket.step holds
//                  the structure, and each part is a sibling file it
//                  references (the CAx-IF s1-c5-214 / SolidWorks export style)
//   multifile-nested/  a multi-file STEP whose trees disagree (#139):
//                  bracket.step holds the bracket inline beside a part that
//                  references pins.stp, and pins.stp is itself an assembly
//                  of two pins. The graph reads bracket.step alone (4
//                  leaves); OCCT follows the reference (5 leaves)
//   sketch.step    curves only, no surfaces: the sketch path
//   box.igs        IGES
//   box.brep       OCCT native BREP
//   left-handed-plane.brep  one square face on a plane with LEFT-handed
//                  axes: its Axis() points +z while the surface (and the
//                  face) faces -z, as mirrored geometry and some .brep do

#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <Geom_Plane.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepTools.hxx>
#include <BRep_Tool.hxx>
#include <cmath>
#include <GC_MakeCircle.hxx>
#include <IGESControl_Controller.hxx>
#include <IGESControl_Writer.hxx>
#include <Interface_Static.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <Quantity_Color.hxx>
#include <STEPCAFControl_Writer.hxx>
#include <STEPControl_Writer.hxx>
#include <TDF_LabelSequence.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <gp_Ax2.hxx>
#include <gp_Circ.hxx>
#include <gp_Trsf.hxx>

#include <cstdio>
#include <filesystem>
#include <string>

namespace {

// `pin_side`: the pins' cylindrical face green, a colour on a curved face.
Handle(TDocStd_Document) bracket(bool pin_side = false) {
    Handle(TDocStd_Document) doc;
    XCAFApp_Application::GetApplication()->NewDocument("MDTV-XCAF", doc);
    Handle(XCAFDoc_ShapeTool) st = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    Handle(XCAFDoc_ColorTool) ct = XCAFDoc_DocumentTool::ColorTool(doc->Main());

    // Plate 40 x 30 x 5 with a through hole.
    TopoDS_Shape plate = BRepAlgoAPI_Cut(
        BRepPrimAPI_MakeBox(40, 30, 5).Shape(),
        BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(20, 15, -1), gp_Dir(0, 0, 1)), 4, 7).Shape());
    TopoDS_Shape pin = BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), 2, 15).Shape();

    TDF_Label assy = st->NewShape();
    TDataStd_Name::Set(assy, "bracket-assy");
    TDF_Label plate_l = st->AddShape(plate, false);
    TDataStd_Name::Set(plate_l, "plate");
    ct->SetColor(plate_l, Quantity_Color(0.10, 0.25, 0.85, Quantity_TOC_RGB), XCAFDoc_ColorSurf);
    // Per-face colour: the plate's top face (z = 5) red.
    for (TopExp_Explorer ex(plate, TopAbs_FACE); ex.More(); ex.Next()) {
        // Planar top face: its vertices all sit at z = 5.
        bool top = true;
        for (TopExp_Explorer v(ex.Current(), TopAbs_VERTEX); v.More(); v.Next())
            top &= std::abs(BRep_Tool::Pnt(TopoDS::Vertex(v.Current())).Z() - 5.0) < 1e-9;
        if (top) {
            TDF_Label f = st->AddSubShape(plate_l, ex.Current());
            ct->SetColor(f, Quantity_Color(0.90, 0.10, 0.10, Quantity_TOC_RGB), XCAFDoc_ColorSurf);
        }
    }
    TDF_Label pin_l = st->AddShape(pin, false);
    TDataStd_Name::Set(pin_l, "pin");
    ct->SetColor(pin_l, Quantity_Color(0.95, 0.50, 0.05, Quantity_TOC_RGB), XCAFDoc_ColorSurf);
    if (pin_side) {
        for (TopExp_Explorer ex(pin, TopAbs_FACE); ex.More(); ex.Next()) {
            if (BRepAdaptor_Surface(TopoDS::Face(ex.Current())).GetType() != GeomAbs_Cylinder)
                continue;
            TDF_Label f = st->AddSubShape(pin_l, ex.Current());
            ct->SetColor(f, Quantity_Color(0.10, 0.80, 0.20, Quantity_TOC_RGB), XCAFDoc_ColorSurf);
        }
    }

    st->AddComponent(assy, plate_l, TopLoc_Location());
    for (double x : {6.0, 34.0}) {
        gp_Trsf t;
        t.SetTranslation(gp_Vec(x, 15, 0));
        st->AddComponent(assy, pin_l, TopLoc_Location(t));
    }
    st->UpdateAssemblies();
    return doc;
}

bool step_assembly(const std::string& path) {
    STEPCAFControl_Writer w;
    w.SetColorMode(true);
    w.SetNameMode(true);
    return w.Transfer(bracket(), STEPControl_AsIs) && w.Write(path.c_str()) == IFSelect_RetDone;
}

// Without SURFACE_CURVE/PCURVE entities: the 2D curves are left for the
// reader to rebuild by projection, the cost #51 budgets.
bool step_no_pcurves(const std::string& path) {
    Interface_Static::SetIVal("write.surfacecurve.mode", 0);
    STEPCAFControl_Writer w;
    w.SetColorMode(true);
    w.SetNameMode(true);
    const bool ok =
        w.Transfer(bracket(true), STEPControl_AsIs) && w.Write(path.c_str()) == IFSelect_RetDone;
    Interface_Static::SetIVal("write.surfacecurve.mode", 1);
    return ok;
}

// Multi-file mode: the parts go to sibling files named after their labels.
bool step_multifile(const std::string& path) {
    // The writer puts the part files in the working directory and references
    // them by bare name: write from inside the target directory.
    const std::filesystem::path p = std::filesystem::absolute(path);
    std::filesystem::create_directories(p.parent_path());
    const auto cwd = std::filesystem::current_path();
    std::filesystem::current_path(p.parent_path());
    STEPCAFControl_Writer w;
    w.SetColorMode(true);
    w.SetNameMode(true);
    // A non-null `multi` prefix switches the writer to multi-file mode.
    const bool ok = w.Transfer(bracket(), STEPControl_AsIs, "") &&
                    w.Write(p.filename().c_str()) == IFSelect_RetDone;
    std::filesystem::current_path(cwd);
    return ok;
}

// The kernel's tree has a leaf more than the graph's: the part file is an
// assembly, which OCCT's multi-file writer never writes (it keeps every
// assembly in the main file), so pins.stp is replaced after the write.
bool step_multifile_nested(const std::string& path) {
    const std::filesystem::path p = std::filesystem::absolute(path);
    std::filesystem::create_directories(p.parent_path());
    const auto cwd = std::filesystem::current_path();
    std::filesystem::current_path(p.parent_path());
    Handle(TDocStd_Document) doc = bracket();
    Handle(XCAFDoc_ShapeTool) st = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    TDF_LabelSequence roots;
    st->GetFreeShapes(roots);
    const TDF_Label assy = roots.Value(1);
    // A second root, whose one part goes to its own file, pins.stp.
    const TopoDS_Shape pin =
        BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), 2, 15).Shape();
    TDF_Label rack = st->NewShape();
    TDataStd_Name::Set(rack, "rack");
    TDF_Label pins = st->AddShape(pin, false);
    TDataStd_Name::Set(pins, "pins");
    gp_Trsf away;
    away.SetTranslation(gp_Vec(0, 50, 0));
    st->AddComponent(rack, pins, TopLoc_Location(away));
    st->UpdateAssemblies();
    STEPCAFControl_Writer w;
    w.SetColorMode(true);
    w.SetNameMode(true);
    bool ok = w.Transfer(assy, STEPControl_AsIs, nullptr) &&
              w.Transfer(rack, STEPControl_AsIs, "") &&
              w.Write(p.filename().c_str()) == IFSelect_RetDone;
    // pins.stp becomes an assembly of two pins.
    Handle(TDocStd_Document) two;
    XCAFApp_Application::GetApplication()->NewDocument("MDTV-XCAF", two);
    Handle(XCAFDoc_ShapeTool) t = XCAFDoc_DocumentTool::ShapeTool(two->Main());
    TDF_Label top = t->NewShape();
    TDataStd_Name::Set(top, "pins");
    TDF_Label one = t->AddShape(pin, false);
    TDataStd_Name::Set(one, "pin");
    for (double x : {0.0, 10.0}) {
        gp_Trsf tr;
        tr.SetTranslation(gp_Vec(x, 0, 0));
        t->AddComponent(top, one, TopLoc_Location(tr));
    }
    t->UpdateAssemblies();
    STEPCAFControl_Writer pw;
    pw.SetNameMode(true);
    ok = ok && pw.Transfer(two, STEPControl_AsIs) && pw.Write("pins.stp") == IFSelect_RetDone;
    std::filesystem::current_path(cwd);
    return ok;
}

bool step_sketch(const std::string& path) {
    gp_Circ circle(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), 25);
    TopoDS_Edge e = BRepBuilderAPI_MakeEdge(circle);
    STEPControl_Writer w;
    return w.Transfer(e, STEPControl_GeometricCurveSet) == IFSelect_RetDone &&
           w.Write(path.c_str()) == IFSelect_RetDone;
}

bool iges_box(const std::string& path) {
    IGESControl_Controller::Init();
    IGESControl_Writer w("MM", 1);  // BRep mode
    w.AddShape(BRepPrimAPI_MakeBox(20, 10, 5).Shape());
    w.ComputeModel();
    return w.Write(path.c_str());
}

bool brep_left_handed_plane(const std::string& path) {
    // Axis() = +z, X = +x, Y = -y: indirect, so the surface normal X x Y
    // is -z. The face is FORWARD, so it faces -z too.
    gp_Ax3 ax(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1), gp_Dir(1, 0, 0));
    ax.YReverse();
    Handle(Geom_Plane) plane = new Geom_Plane(ax);
    TopoDS_Face face = BRepBuilderAPI_MakeFace(plane, 0, 10, -10, 0, 1e-7);
    return BRepTools::Write(face, path.c_str());
}

bool brep_box(const std::string& path) {
    return BRepTools::Write(BRepPrimAPI_MakeBox(20, 10, 5).Shape(), path.c_str());
}

}  // namespace

int main(int argc, char** argv) {
    if (argc != 2) {
        std::fputs("usage: fixture-gen <out-dir>\n", stderr);
        return 2;
    }
    Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
    const std::string dir = argv[1];
    struct {
        const char* name;
        bool (*make)(const std::string&);
    } jobs[] = {{"assembly.step", step_assembly},
                {"no-pcurves.step", step_no_pcurves},
                {"multifile/bracket.step", step_multifile},
                {"multifile-nested/bracket.step", step_multifile_nested},
                {"sketch.step", step_sketch},
                {"box.igs", iges_box},
                {"box.brep", brep_box},
                {"left-handed-plane.brep", brep_left_handed_plane}};
    for (const auto& j : jobs) {
        if (!j.make(dir + "/" + j.name)) {
            std::fprintf(stderr, "fixture-gen: failed to write %s\n", j.name);
            return 1;
        }
    }
    return 0;
}
