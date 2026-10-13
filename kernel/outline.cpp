// The assembly outline (#54). See outline.h.
//
// It mirrors, entity for entity, the OCCT passes that build the product
// levels of the tree --topology walks:
//   - the shape transfer's choice of roots and children:
//     XSControl_Reader::RootForTransfer, and the NAUO list that
//     STEPControl_ActorRead::TransferEntity(ProductDefinition) builds from
//     the graph's sharings, in that order;
//   - STEPCAFControl_Reader::ReadNames: a product is named by its name, else
//     its id; an instance (NAUO) by its description, else name, else id.
// and then applies the kernel's own naming (stepv-occt-core.cpp `walk`): an
// assembly takes its product's name, else its instance's; a part its
// product's name, else its instance's.
//
// Two things it does NOT mirror. A product with sub-assemblies AND a shape
// of its own: the transfer keeps both (ActorRead, "Product has both
// sub-assemblies and directly assigned shape"), so --topology has its own
// body as an extra unnamed child; the outline has the sub-assemblies only.
// No corpus file has one. And how a product's representations become bodies:
// a part whose main SHAPE_REPRESENTATION relates several sub-representations
// (a solid and a wireframe, AP203's hybrid models) transfers to several
// bodies, which --topology shows as unnamed children. That depends on OCCT's
// representation selection and on what each transfers to, so the outline
// stops at the product: see outline.h for the contract.

#include "outline.h"

#include "input.h"
#include "json.h"

#include <Interface_EntityIterator.hxx>
#include <Interface_Graph.hxx>
#include <Resource_FormatType.hxx>
#include <STEPControl_Reader.hxx>
#include <StepBasic_Product.hxx>
#include <StepBasic_ProductDefinition.hxx>
#include <StepBasic_ProductDefinitionFormation.hxx>
#include <StepData_StepModel.hxx>
#include <StepRepr_NextAssemblyUsageOccurrence.hxx>
#include <StepRepr_ProductDefinitionShape.hxx>
#include <StepDimTol_DatumFeature.hxx>
#include <StepDimTol_GeometricTolerance.hxx>
#include <StepRepr_PropertyDefinition.hxx>
#include <StepRepr_Representation.hxx>
#include <StepRepr_ShapeAspect.hxx>
#include <StepShape_ShapeDefinitionRepresentation.hxx>
#include <TCollection_ExtendedString.hxx>
#include <TCollection_HAsciiString.hxx>
#include <XSControl_TransferReader.hxx>
#include <XSControl_WorkSession.hxx>

#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <cstring>
#include <map>
#include <sstream>
#include <stdexcept>
#include <vector>

namespace stepv {
namespace {

bool usable(const Handle(TCollection_HAsciiString)& s) {
    return !s.IsNull() && s->UsefullLength() > 0;
}

struct Namer {
    bool utf8;

    // As STEPCAFControl_Reader::convertName, then to UTF-8 as label_name()
    // reads it back: the same bytes --topology writes.
    std::string operator()(const Handle(TCollection_HAsciiString)& s) const {
        if (s.IsNull()) return {};
        const TCollection_ExtendedString ext(s->String(), utf8);
        const TCollection_AsciiString out(ext);
        return out.ToCString();
    }

    std::string product(const Handle(StepBasic_ProductDefinition)& pd) const {
        if (pd->Formation().IsNull()) return {};
        const Handle(StepBasic_Product) prod = pd->Formation()->OfProduct();
        if (prod.IsNull()) return {};
        if (usable(prod->Name())) return (*this)(prod->Name());
        return prod->Id().IsNull() ? std::string() : (*this)(prod->Id());
    }

    std::string instance(const Handle(StepRepr_NextAssemblyUsageOccurrence)& n) const {
        if (n->HasDescription() && usable(n->Description())) return (*this)(n->Description());
        if (usable(n->Name())) return (*this)(n->Name());
        return n->Id().IsNull() ? std::string() : (*this)(n->Id());
    }
};

// The sub-assembly occurrences of `pd`, in the order the shape transfer
// visits them (STEPControl_ActorRead::TransferEntity and getSDR).
std::vector<Handle(StepRepr_NextAssemblyUsageOccurrence)> children(
    const Interface_Graph& graph, const Handle(StepBasic_ProductDefinition)& pd) {
    std::vector<Handle(StepRepr_NextAssemblyUsageOccurrence)> out;
    for (Interface_EntityIterator it = graph.Sharings(pd); it.More(); it.Next()) {
        if (auto pds = Handle(StepRepr_ProductDefinitionShape)::DownCast(it.Value())) {
            for (Interface_EntityIterator jt = graph.Sharings(pds); jt.More(); jt.Next()) {
                auto n = Handle(StepRepr_NextAssemblyUsageOccurrence)::DownCast(jt.Value());
                if (!n.IsNull() && pds->Definition().ProductDefinition() == n->RelatingProductDefinition())
                    out.push_back(n);
            }
            continue;
        }
        auto n = Handle(StepRepr_NextAssemblyUsageOccurrence)::DownCast(it.Value());
        if (!n.IsNull() && n->RelatingProductDefinition() == pd) out.push_back(n);
    }
    return out;
}

// An SDR the transfer would translate: one with a representation
// (STEPControl_ActorRead getSDR).
bool usable_sdr(const Handle(Standard_Transient)& e) {
    auto sdr = Handle(StepShape_ShapeDefinitionRepresentation)::DownCast(e);
    return !sdr.IsNull() && !sdr->UsedRepresentation().IsNull();
}

// Whether `pd` has a shape of its own, found the way getSDR finds one: an
// SDR on one of its PRODUCT_DEFINITION_SHAPEs, or on a SHAPE_ASPECT of one
// (getListSDR; aspects used for GD&T do not count). A product with neither,
// and no sub-assemblies (a material, a document), transfers to nothing, so
// the tree --topology walks does not have it either.
bool has_shape(const Interface_Graph& graph, const Handle(StepBasic_ProductDefinition)& pd) {
    for (Interface_EntityIterator it = graph.Sharings(pd); it.More(); it.Next()) {
        if (Handle(StepRepr_ProductDefinitionShape)::DownCast(it.Value()).IsNull()) continue;
        for (Interface_EntityIterator jt = graph.Sharings(it.Value()); jt.More(); jt.Next()) {
            if (usable_sdr(jt.Value())) return true;
            auto sa = Handle(StepRepr_ShapeAspect)::DownCast(jt.Value());
            if (sa.IsNull() || sa->IsKind(STANDARD_TYPE(StepDimTol_DatumFeature))) continue;
            bool tolerance = false;
            for (Interface_EntityIterator kt = graph.Sharings(sa); kt.More(); kt.Next())
                tolerance |= kt.Value()->IsKind(STANDARD_TYPE(StepDimTol_GeometricTolerance));
            if (tolerance) continue;
            for (Interface_EntityIterator kt = graph.Sharings(sa); kt.More(); kt.Next()) {
                if (Handle(StepRepr_PropertyDefinition)::DownCast(kt.Value()).IsNull()) continue;
                for (Interface_EntityIterator lt = graph.Sharings(kt.Value()); lt.More(); lt.Next())
                    if (usable_sdr(lt.Value())) return true;
            }
        }
    }
    return false;
}

// Bounds on what one file may make the walker write. Shared sub-assemblies
// expand per instance, so a small file can describe an exponential tree;
// the transfer would expand it too, but the outline must stay cheap, and it
// also runs in-process (the C API). Depth 60 also keeps the JSON's nesting
// (about two levels per node) inside serde_json's default limit of 128.
constexpr int kMaxDepth = 60;
constexpr long kMaxNodes = 1000000;

struct Walker {
    const Interface_Graph& graph;
    Namer name;
    std::ostringstream o;
    long leaves = 0;
    long nodes = 0;
    std::map<const Standard_Transient*, bool> shown_;

    // Whether node() writes anything for `pd`: it has a shape, or a child
    // that is shown (an assembly of shapeless products transfers to nothing
    // too). Memoised, since a product can be instanced many times.
    bool shown(const Handle(StepBasic_ProductDefinition)& pd, int depth = 0) {
        if (depth > kMaxDepth) return true;  // node() reports the depth
        if (auto it = shown_.find(pd.get()); it != shown_.end()) return it->second;
        bool r = has_shape(graph, pd);
        if (!r) {
            for (const auto& n : children(graph, pd)) {
                const Handle(StepBasic_ProductDefinition) c = n->RelatedProductDefinition();
                if (!c.IsNull() && shown(c, depth + 1)) {
                    r = true;
                    break;
                }
            }
        }
        shown_[pd.get()] = r;
        return r;
    }

    void node(const Handle(StepBasic_ProductDefinition)& pd, const std::string& instance,
              int depth) {
        // A cyclic or absurdly deep product graph is malformed input, not a
        // reason to blow the stack (the same bound as walk()).
        if (depth > kMaxDepth) throw std::runtime_error("assembly nesting deeper than 60");
        if (++nodes > kMaxNodes) throw std::runtime_error("outline larger than a million nodes");
        std::string own = name.product(pd);
        const std::string& label = own.empty() ? instance : own;
        std::vector<Handle(StepBasic_ProductDefinition)> kids;
        std::vector<std::string> names;
        for (const auto& n : children(graph, pd)) {
            const Handle(StepBasic_ProductDefinition) child = n->RelatedProductDefinition();
            if (child.IsNull() || !shown(child, depth + 1)) continue;
            kids.push_back(child);
            names.push_back(name.instance(n));
        }
        o << "{\"name\":\"" << json_escape(label) << '"';
        if (kids.empty()) {
            o << ",\"part\":" << leaves++ << '}';
            return;
        }
        o << ",\"children\":[";
        bool first = true;
        for (std::size_t k = 0; k < kids.size(); ++k) {
            o << (first ? "" : ",");
            first = false;
            node(kids[k], names[k], depth + 1);
        }
        o << "]}";
    }
};

}  // namespace

std::string outline_json(const std::string& path, const std::string& format) {
    std::string why;
    const std::string fmt = reader_format(path, format, why);
    if (fmt.empty()) throw std::runtime_error(why);
    if (fmt != "step") {
        throw std::runtime_error("an outline needs a STEP file; IGES and BREP have no product structure");
    }
    // A descriptor is read from its start, never re-opened by name (input.h).
    STEPControl_Reader reader;
    IFSelect_ReturnStatus read;
    if (const auto fd = input_fd(path)) {
        if (const int err = unreadable_fd(*fd))
            throw std::runtime_error("cannot read input descriptor: " + std::string(std::strerror(err)));
        FdStreamBuf buf(*fd);
        std::istream in(&buf);
        read = reader.ReadStream(path.c_str(), in);
        if (buf.error())
            throw std::runtime_error("cannot read input descriptor: " + std::string(std::strerror(buf.error())));
    } else {
        read = reader.ReadFile(path.c_str());
    }
    if (read != IFSelect_RetDone) throw std::runtime_error("STEP read failed");
    return outline_json(reader);
}

std::string outline_json(STEPControl_Reader& reader) {
    const Handle(StepData_StepModel) model = reader.StepModel();
    const Namer namer{model->SourceCodePage() != Resource_FormatType_NoConversion};
    const Interface_Graph& graph = reader.WS()->Graph();

    Walker w{graph, namer, {}, 0, 0, {}};
    w.o << "{\"format\":\"stepv-outline\",\"version\":1,\"tree\":[";
    bool first = true;
    const int roots = reader.NbRootsForTransfer();
    for (int i = 1; i <= roots; ++i) {
        auto pd = Handle(StepBasic_ProductDefinition)::DownCast(reader.RootForTransfer(i));
        if (pd.IsNull() || !w.shown(pd)) continue;
        w.o << (first ? "" : ",");
        first = false;
        w.node(pd, std::string(), 0);
    }
    w.o << "],\"leaves\":" << w.leaves << '}';
    return w.o.str();
}

}  // namespace stepv
