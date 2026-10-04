// stepv-occt — the kernel CLI the Rust side runs as a subprocess. All the
// work is in stepv-occt-core.cpp; this is argument parsing and the stdout
// contract (one JSON line; OCCT's own chatter goes to stderr).

#include "sandbox.h"
#include "stepv_occt.h"

#include <arpa/inet.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <spawn.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

#include <cerrno>
#include <csignal>
#include <climits>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

namespace {
constexpr int kExitOk = 0;
constexpr int kExitUsage = 2;
const char* const kUsage =
    "usage: stepv-occt <input> [--mesh <out>] [--topology <out.json>] [--linear-rel <f>]\n"
    "                  [--angular-deg <f>]\n";

// realpath(), or "" when it does not resolve.
std::string canonical(const std::string& p) {
    char r[PATH_MAX];
    return realpath(p.c_str(), r) ? r : "";
}

// The canonical path of an open file, "" if the OS will not say.
std::string fd_path(int fd) {
#if defined(__APPLE__)
    char r[PATH_MAX];
    return fcntl(fd, F_GETPATH, r) == 0 ? r : "";
#else
    char r[PATH_MAX];
    const std::string link = "/proc/self/fd/" + std::to_string(fd);
    const ssize_t n = readlink(link.c_str(), r, sizeof r - 1);
    return n > 0 ? std::string(r, static_cast<std::size_t>(n)) : "";
#endif
}

// Test hook for the sandbox (tests/sandbox.rs, #18). `spec` is one action,
// tried from inside the sandbox; the outcome goes to stderr as
// "stepv-occt escape <kind>: allowed" or "... refused (<why>)":
//
//   connect:<ipv4>:<port>   a TCP connection          udp:<ipv4>:<port>   one datagram
//   exec:<path>             `<path> -c 'exit 7'`      write:<path>        create + write
//   read:<path>             open + read one byte  kill:<pid>          SIGTERM to a process
//
// The run then carries on as normal, so one run proves both the refusal and
// that the work the kernel is there for still happens.
void escape_attempt(const std::string& spec) {
    const auto colon = spec.find(':');
    const std::string kind = spec.substr(0, colon);
    const std::string arg = colon == std::string::npos ? "" : spec.substr(colon + 1);
    auto report = [&](bool allowed, int err) {
        std::fprintf(stderr, "stepv-occt escape %s: %s", kind.c_str(), allowed ? "allowed" : "refused");
        if (!allowed) std::fprintf(stderr, " (%s)", err ? std::strerror(err) : "failed");
        std::fputc('\n', stderr);
    };
    auto inet = [&](sockaddr_in& a) {
        const auto c = arg.rfind(':');
        a = {};
        a.sin_family = AF_INET;
        a.sin_port = htons(static_cast<uint16_t>(std::atoi(arg.c_str() + c + 1)));
        return c != std::string::npos && inet_pton(AF_INET, arg.substr(0, c).c_str(), &a.sin_addr) == 1;
    };
    if (kind == "connect" || kind == "udp") {
        sockaddr_in a;
        if (!inet(a)) { report(false, EINVAL); return; }
        const bool tcp = kind == "connect";
        const int fd = socket(AF_INET, tcp ? SOCK_STREAM : SOCK_DGRAM, 0);
        if (fd < 0) { report(false, errno); return; }
        const auto* sa = reinterpret_cast<const sockaddr*>(&a);
        const bool ok = tcp ? connect(fd, sa, sizeof a) == 0
                            : sendto(fd, "x", 1, 0, sa, sizeof a) == 1;
        const int err = errno;
        close(fd);
        report(ok, err);
    } else if (kind == "exec") {
        char* argv[] = {const_cast<char*>(arg.c_str()), const_cast<char*>("-c"),
                        const_cast<char*>("exit 7"), nullptr};
        pid_t pid;
        if (const int err = posix_spawn(&pid, arg.c_str(), nullptr, nullptr, argv, nullptr)) {
            report(false, err);
            return;
        }
        int status = 0;
        waitpid(pid, &status, 0);
        report(WIFEXITED(status) && WEXITSTATUS(status) == 7, 0);
    } else if (kind == "write") {
        const int fd = open(arg.c_str(), O_WRONLY | O_CREAT | O_TRUNC, 0600);
        if (fd < 0) { report(false, errno); return; }
        const bool ok = write(fd, "pwned\n", 6) == 6;
        const int err = errno;
        close(fd);
        report(ok, err);
    } else if (kind == "kill") {
        // errno read only after the call: argument evaluation order is unspecified.
        const bool ok = kill(static_cast<pid_t>(std::atoi(arg.c_str())), SIGTERM) == 0;
        report(ok, errno);
    } else if (kind == "read") {
        const int fd = open(arg.c_str(), O_RDONLY);
        if (fd < 0) { report(false, errno); return; }
        char c;
        const bool ok = read(fd, &c, 1) == 1;
        const int err = errno;
        close(fd);
        report(ok, err);
    } else {
        report(false, EINVAL);
    }
}
}  // namespace

int main(int argc, char** argv) {
    std::string input, mesh_out, topology_out;
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
        else if (a == "--topology") topology_out = value();
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

    // Into the sandbox before the input is opened: from here on, what a
    // hostile file can make this process do is bounded (sandbox.h). The mesh
    // is created now, so the sandbox can grant that one file and no right to
    // create any. An input that does not resolve gets no read access at all;
    // the kernel then fails on it as it would anyway.
    //
    // Both paths are resolved here, outside it: inside, realpath() of a
    // relative path cannot read the working directory's ancestry (macOS).
    const std::string input_path = canonical(input);
    const auto slash = input_path.rfind('/');
    const std::string input_dir = slash == std::string::npos ? ""
                                  : slash == 0               ? "/"
                                                             : input_path.substr(0, slash);
    // Siblings are readable for multi-file assemblies, but not when that
    // means the whole of $HOME (~/.ssh among it), of an ancestor of it, or of
    // "/": a file lying there gets read access to itself alone.
    std::string read_root = input_dir;
    const char* home_env = std::getenv("HOME");
    const std::string home = home_env ? canonical(home_env) : "";
    if (input_dir == "/" ||
        (!home.empty() && !input_dir.empty() &&
         (home == input_dir || home.rfind(input_dir + "/", 0) == 0)))
        read_root = input_path;
    // The writable files (the mesh, the topology), each granted by the path
    // its descriptor really has.
    // O_NOFOLLOW: through a symlink, whatever it points at would become
    // writable. Not created, not granted: the core then fails to open it.
    std::vector<std::string> writable;
    for (std::string* out : {&mesh_out, &topology_out}) {
        if (out->empty()) continue;
        const int fd =
            open(out->c_str(), O_WRONLY | O_CREAT | O_TRUNC | O_NOFOLLOW | O_CLOEXEC, 0644);
        if (fd < 0) continue;
        if (const std::string granted = fd_path(fd); !granted.empty()) {
            writable.push_back(granted);
            *out = granted;
        }
        close(fd);
    }
    const std::string sandbox = stepv::enter_sandbox(read_root, writable);
    // Unresolvable: the core reports it, in its own words.
    if (!input_path.empty()) input = input_path;

    if (const char* e = std::getenv("STEPV_OCCT_TEST_ESCAPE")) escape_attempt(e);

    // The JSON summary is the contract on stdout, and OCCT's readers print
    // progress chatter to stdout. Keep the real stdout for the summary alone
    // and point fd 1 at stderr for everything else.
    const int json_fd = dup(STDOUT_FILENO);
    std::fflush(stdout);
    dup2(STDERR_FILENO, STDOUT_FILENO);

    int code = 3;
    char* json = stepv_occt_run_topology(
        input.c_str(), mesh_out.empty() ? nullptr : mesh_out.c_str(),
        topology_out.empty() ? nullptr : topology_out.c_str(), linear_rel, angular_deg, &code);
    if (!json) return 3;
    // The summary says which sandbox the run was in.
    std::string line = json;
    stepv_occt_free(json);
    if (!line.empty() && line[0] == '{')
        line.insert(1, "\"sandbox\":\"" + sandbox + "\"" + (line.size() > 2 ? "," : ""));
    line += "\n";
    if (write(json_fd, line.data(), line.size()) < 0) return 3;
    return code;
}
