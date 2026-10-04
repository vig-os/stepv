// The kernel CLI's sandbox (#18). Only stepv-occt enters it: libstepvocct
// runs inside the macOS Quick Look extensions, whose App Sandbox is already
// the boundary.
#pragma once

#include <string>

namespace stepv {

// Confines this process for the rest of its life to what a kernel run needs:
// reading `input_dir` and everything below it (multi-file assemblies resolve
// external references there), and writing the one existing file `mesh_out`
// (empty: none). No network, no exec, no other reads or writes.
//
// Call it single-threaded, before the input is opened. Returns what is in
// effect: "landlock+seccomp" or "macos-profile" when complete; anything else
// ("seccomp", "landlock", "none") is partial, and a warning is on stderr.
std::string enter_sandbox(const std::string& input_dir, const std::string& mesh_out);

}  // namespace stepv
