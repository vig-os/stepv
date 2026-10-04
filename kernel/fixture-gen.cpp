// fixture-gen — writes the small, committed test corpus in tests/data/.
//
// Everything in tests/fixtures/ is fetched and gitignored (some of it is
// redistribution-restricted); these files are ours, small, and deterministic
// in content, so the CLI integration tests and `nix flake check` can run with
// no network. Regenerate with `just test-data`.
//
//   assembly.step  an assembly: a plate with a hole (blue, top face red) and
//                  two instances of one pin (orange), all named
//   sketch.step    curves only, no surfaces: the sketch path
//   box.igs        IGES
//   box.brep       OCCT native BREP

#include <BRepAlgoAPI_Cut.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
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
#include <string>

namespace {

bool step_assembly(const std::string& path) {
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

    st->AddComponent(assy, plate_l, TopLoc_Location());
    for (double x : {6.0, 34.0}) {
        gp_Trsf t;
        t.SetTranslation(gp_Vec(x, 15, 0));
        st->AddComponent(assy, pin_l, TopLoc_Location(t));
    }
    st->UpdateAssemblies();

    STEPCAFControl_Writer w;
    w.SetColorMode(true);
    w.SetNameMode(true);
    return w.Transfer(doc, STEPControl_AsIs) && w.Write(path.c_str()) == IFSelect_RetDone;
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
                {"sketch.step", step_sketch},
                {"box.igs", iges_box},
                {"box.brep", brep_box}};
    for (const auto& j : jobs) {
        if (!j.make(dir + "/" + j.name)) {
            std::fprintf(stderr, "fixture-gen: failed to write %s\n", j.name);
            return 1;
        }
    }
    return 0;
}
