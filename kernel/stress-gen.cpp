// stress-gen — writes the `stress-assembly` fixture (manifest.toml): one
// STEP file made of `copies` DISTINCT deep copies of a donor part, laid out
// on a grid. Distinct, not instanced: instancing would keep the file small
// and the kernel would mesh the prototype once, which defeats the purpose of
// finding the memory cliff (plan.md §3 risk 3, §5 S3).
//
//   stress-gen <donor.step> <copies> <out.step>

#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_Copy.hxx>
#include <BRep_Builder.hxx>
#include <Bnd_Box.hxx>
#include <Interface_Static.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <TopoDS_Compound.hxx>
#include <gp_Trsf.hxx>

#include <cmath>
#include <cstdio>
#include <cstdlib>

int main(int argc, char** argv) {
    if (argc != 4) {
        std::fputs("usage: stress-gen <donor.step> <copies> <out.step>\n", stderr);
        return 2;
    }
    Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
    STEPControl_Reader reader;
    if (reader.ReadFile(argv[1]) != IFSelect_RetDone || reader.TransferRoots() == 0) {
        std::fputs("stress-gen: cannot read donor\n", stderr);
        return 1;
    }
    const TopoDS_Shape donor = reader.OneShape();
    Bnd_Box box;
    BRepBndLib::Add(donor, box, false);
    const double pitch = 1.2 * std::sqrt(box.SquareExtent());

    const int copies = std::atoi(argv[2]);
    const int side = static_cast<int>(std::ceil(std::cbrt(copies)));
    BRep_Builder builder;
    TopoDS_Compound all;
    builder.MakeCompound(all);
    for (int i = 0; i < copies; ++i) {
        gp_Trsf t;
        t.SetTranslation(gp_Vec(pitch * (i % side), pitch * ((i / side) % side),
                                pitch * (i / (side * side))));
        // Copy geometry too, then place: every copy is its own B-rep.
        TopoDS_Shape copy = BRepBuilderAPI_Copy(donor, true).Shape();
        builder.Add(all, copy.Moved(TopLoc_Location(t)));
    }
    STEPControl_Writer writer;
    if (writer.Transfer(all, STEPControl_AsIs) != IFSelect_RetDone ||
        writer.Write(argv[3]) != IFSelect_RetDone) {
        std::fputs("stress-gen: write failed\n", stderr);
        return 1;
    }
    return 0;
}
