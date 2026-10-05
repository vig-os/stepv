// The kernel CLI's sandbox (#18). Only stepv-occt enters it: libstepvocct
// runs inside the macOS Quick Look extensions, whose App Sandbox is already
// the boundary.
#pragma once

#include <string>
#include <vector>

namespace stepv {

// Confines this process for the rest of its life to what a kernel run needs:
// reading `read_root` — the input's directory and everything below it, where
// multi-file assemblies resolve their external references, or the input file
// alone — and writing the existing files in `writable` (the mesh, the
// topology), each by its canonical path. No
// network, no exec, no new processes, no signals to others, no other reads or
// writes. A path that cannot be granted loses that access, not the sandbox.
//
// Call it single-threaded, before the input is opened. Returns what is in
// effect: "landlock+seccomp" or "macos-profile" when complete; anything else
// ("seccomp", "landlock", "macos-outer" — already inside another macOS
// sandbox, which does not nest —, "none") is partial, and a warning is on
// stderr.
std::string enter_sandbox(const std::string& read_root, const std::vector<std::string>& writable);

}  // namespace stepv
