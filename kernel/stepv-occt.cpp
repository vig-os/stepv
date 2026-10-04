// stepv-occt — the kernel CLI the Rust side runs as a subprocess. All the
// work is in stepv-occt-core.cpp; this is argument parsing and the stdout
// contract (one JSON line; OCCT's own chatter goes to stderr).

#include "stepv_occt.h"

#include <unistd.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

namespace {
constexpr int kExitOk = 0;
constexpr int kExitUsage = 2;
const char* const kUsage =
    "usage: stepv-occt <input> [--mesh <out>] [--linear-rel <f>] [--angular-deg <f>]\n";
}  // namespace

int main(int argc, char** argv) {
    std::string input, mesh_out;
    double linear_rel = 0.001, angular_deg = 20.0;  // = stepv::Deflection::PREVIEW
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        auto value = [&]() -> const char* {
            if (i + 1 >= argc) {
                std::fputs(kUsage, stderr);
                std::exit(kExitUsage);
            }
            return argv[++i];
        };
        if (a == "--mesh") mesh_out = value();
        else if (a == "--linear-rel") linear_rel = std::strtod(value(), nullptr);
        else if (a == "--angular-deg") angular_deg = std::strtod(value(), nullptr);
        else if (a == "-h" || a == "--help") { std::fputs(kUsage, stdout); return kExitOk; }
        else if (!a.empty() && a[0] == '-') { std::fputs(kUsage, stderr); return kExitUsage; }
        else if (input.empty()) input = a;
        else { std::fputs(kUsage, stderr); return kExitUsage; }
    }
    if (input.empty() || !(linear_rel > 0) || !(angular_deg > 0)) {
        std::fputs(kUsage, stderr);
        return kExitUsage;
    }

    // Test hook for the memory cap (tests/cli.rs): allocate and touch this many
    // MiB, then hold them, so the parent's cap has something deterministic to
    // catch. A real file's footprint depends on its geometry and timing.
    if (const char* b = std::getenv("STEPV_OCCT_TEST_BALLOON_MB")) {
        const std::size_t bytes = std::strtoull(b, nullptr, 10) << 20;
        auto* p = static_cast<volatile char*>(std::malloc(bytes));
        for (std::size_t i = 0; p && i < bytes; i += 4096) p[i] = 1;
        sleep(5);
        std::free(const_cast<char*>(p));
    }

    // The JSON summary is the contract on stdout, and OCCT's readers print
    // progress chatter to stdout. Keep the real stdout for the summary alone
    // and point fd 1 at stderr for everything else.
    const int json_fd = dup(STDOUT_FILENO);
    std::fflush(stdout);
    dup2(STDERR_FILENO, STDOUT_FILENO);

    int code = 3;
    char* json = stepv_occt_run(input.c_str(), mesh_out.empty() ? nullptr : mesh_out.c_str(),
                                linear_rel, angular_deg, &code);
    if (!json) return 3;
    const std::string line = std::string(json) + "\n";
    stepv_occt_free(json);
    if (write(json_fd, line.data(), line.size()) < 0) return 3;
    return code;
}
