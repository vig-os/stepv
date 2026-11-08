// The C ABI the extensions link: the Rust renderer + header reader
// (capi/stepv.h, libstepv_capi.a). The OCCT kernel is not linked: it runs in
// each extension's supervisor service (#64).
#include "../../capi/stepv.h"
