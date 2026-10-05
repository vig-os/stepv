// The kernel CLI's sandbox (#18); see sandbox.h for the contract.
//
// The threat: OCCT's STEP/IGES/BREP readers are a large C++ parser fed
// untrusted files. If a crafted file corrupts memory and runs code, that code
// must find a process that can read nothing but the input's directory, write
// nothing but the mesh, and neither reach the network nor start a program.
//
//   Linux  Landlock (file system; TCP too from ABI 4) + a seccomp-bpf deny
//          list (sockets, exec, process creation, ptrace, namespaces, …).
//          Both unprivileged; Landlock needs Linux >= 5.13 and degrades.
//   macOS  a Seatbelt (SBPL) profile via sandbox_init, deny by default.

#include "sandbox.h"

#include <fcntl.h>
#include <unistd.h>

#include <cerrno>
#include <cstdio>
#include <cstring>

#if defined(__linux__)
#include <linux/audit.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <sched.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/syscall.h>

#include <cstddef>
#include <cstdint>
#include <vector>
#elif defined(__APPLE__)
#include <sandbox.h>
#endif

namespace stepv {
namespace {

void warn(const char* what, int err) {
    std::fprintf(stderr, "stepv-occt: warning: %s (%s); the kernel runs less contained\n", what,
                 err ? std::strerror(err) : "unsupported");
}

}  // namespace

#if defined(__linux__)
namespace {

// Landlock's ABI, spelled out: it is stable by contract, and spelling it out
// keeps the build independent of how new the build host's kernel headers are.
constexpr long kSysCreateRuleset = 444, kSysAddRule = 445, kSysRestrictSelf = 446;
constexpr uint32_t kCreateRulesetVersion = 1u << 0;
constexpr int kRulePathBeneath = 1;

constexpr uint64_t kExecute = 1ull << 0, kWriteFile = 1ull << 1, kReadFile = 1ull << 2,
                   kReadDir = 1ull << 3, kTruncate = 1ull << 14;
// Every file-system right ABI n knows: v1 has 13, v2 adds REFER, v3
// TRUNCATE, v5 IOCTL_DEV. Handling all of them and granting few is the point.
constexpr uint64_t fs_rights(int abi) {
    uint64_t r = (1ull << 13) - 1;
    if (abi >= 2) r |= 1ull << 13;
    if (abi >= 3) r |= 1ull << 14;
    if (abi >= 5) r |= 1ull << 15;
    return r;
}
constexpr uint64_t kNetBindTcp = 1ull << 0, kNetConnectTcp = 1ull << 1;
constexpr uint64_t kScopeAbstractUnix = 1ull << 0, kScopeSignal = 1ull << 1;

struct RulesetAttr {
    uint64_t handled_access_fs;
    uint64_t handled_access_net;  // ABI 4
    uint64_t scoped;              // ABI 6
};
struct __attribute__((packed)) PathBeneath {
    uint64_t allowed_access;
    int32_t parent_fd;
};

// A rule on a file may carry only the rights that apply to files: anything
// else and the kernel refuses the rule (EINVAL).
constexpr uint64_t kFileRights = kExecute | kWriteFile | kReadFile | kTruncate | (1ull << 15);

bool allow(int ruleset, const std::string& path, uint64_t rights) {
    const int fd = open(path.c_str(), O_PATH | O_CLOEXEC);
    if (fd < 0) return false;
    struct stat st {};
    if (fstat(fd, &st) == 0 && !S_ISDIR(st.st_mode)) rights &= kFileRights;
    PathBeneath rule{rights, fd};
    const bool ok = syscall(kSysAddRule, ruleset, kRulePathBeneath, &rule, 0) == 0;
    close(fd);
    return ok;
}

// Returns 0, or the errno that kept Landlock out.
int landlock(const std::string& read_root, const std::vector<std::string>& writable) {
    const long abi = syscall(kSysCreateRuleset, nullptr, 0, kCreateRulesetVersion);
    if (abi < 1) return errno;
    RulesetAttr attr{fs_rights(static_cast<int>(abi)), 0, 0};
    if (abi >= 4) attr.handled_access_net = kNetBindTcp | kNetConnectTcp;
    if (abi >= 6) attr.scoped = kScopeAbstractUnix | kScopeSignal;
    const size_t size = abi >= 6 ? sizeof attr : abi >= 4 ? offsetof(RulesetAttr, scoped)
                                                          : offsetof(RulesetAttr, handled_access_net);
    const int ruleset = static_cast<int>(syscall(kSysCreateRuleset, &attr, size, 0));
    if (ruleset < 0) return errno;
    // A grant that fails (a path gone since main resolved it) costs that
    // access, never the sandbox: the ruleset denies by default, so entering
    // it with fewer rules is only stricter.
    if (!read_root.empty() && !allow(ruleset, read_root, kReadFile | kReadDir))
        warn("cannot grant reading the input", errno);
    // The outputs exist already (main creates them), so each file is a rule:
    // no right to create or replace files anywhere.
    const uint64_t write = kWriteFile | (abi >= 3 ? kTruncate : 0);
    for (const std::string& w : writable)
        if (!allow(ruleset, w, write)) warn("cannot grant writing an output", errno);
    const int err = syscall(kSysRestrictSelf, ruleset, 0) == 0 ? 0 : errno;
    close(ruleset);
    return err;
}

#if defined(__x86_64__)
constexpr uint32_t kArch = AUDIT_ARCH_X86_64;
#elif defined(__aarch64__)
constexpr uint32_t kArch = AUDIT_ARCH_AARCH64;
#else
#error "stepv-occt's seccomp filter knows x86_64 and aarch64"
#endif

// Returns 0, or the errno that kept the filter out.
int seccomp() {
    // Refused with EPERM, so a refusal is an error the kernel's code handles,
    // not a kill that looks like a crash.
    const int denied[] = {
        SYS_socket, SYS_socketpair, SYS_connect, SYS_bind, SYS_listen, SYS_accept, SYS_accept4,
        SYS_execve, SYS_execveat, SYS_ptrace, SYS_process_vm_readv, SYS_process_vm_writev,
        SYS_mount, SYS_umount2, SYS_pivot_root, SYS_chroot, SYS_unshare, SYS_setns,
        SYS_keyctl, SYS_add_key, SYS_request_key, SYS_bpf, SYS_perf_event_open,
        SYS_userfaultfd, SYS_io_uring_setup, SYS_io_uring_enter, SYS_io_uring_register,
        SYS_tkill,  // targets a bare thread id: no way to tell it is ours
#if defined(SYS_pidfd_open)
        SYS_pidfd_open, SYS_pidfd_send_signal, SYS_pidfd_getfd,
#endif
#if defined(__x86_64__)
        SYS_fork, SYS_vfork,
#endif
    };
    std::vector<sock_filter> f;
    auto ld = [&](uint32_t off) { f.push_back(BPF_STMT(BPF_LD | BPF_W | BPF_ABS, off)); };
    auto ret = [&](uint32_t v) { f.push_back(BPF_STMT(BPF_RET | BPF_K, v)); };
    const auto deny = SECCOMP_RET_ERRNO | EPERM;

    ld(offsetof(seccomp_data, arch));
    f.push_back(BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, kArch, 1, 0));
    ret(SECCOMP_RET_KILL_PROCESS);
    ld(offsetof(seccomp_data, nr));
#if defined(__x86_64__)
    f.push_back(BPF_JUMP(BPF_JMP | BPF_JGE | BPF_K, 0x40000000u, 0, 1));  // x32 ABI
    ret(deny);
#endif
    for (int nr : denied) {
        f.push_back(BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, static_cast<uint32_t>(nr), 0, 1));
        ret(deny);
    }
    // Signals to this process only (abort() raises SIGABRT on itself): never
    // to another of the user's processes, nor to a group (0, -1, -pgid).
    // Without exec or fork the pid stays this one, so it is a constant here.
    const auto self = static_cast<uint32_t>(getpid());
    for (int nr : {SYS_kill, SYS_tgkill, SYS_rt_sigqueueinfo, SYS_rt_tgsigqueueinfo}) {
        f.push_back(BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, static_cast<uint32_t>(nr), 0, 4));
        ld(offsetof(seccomp_data, args[0]));  // the target: pid or tgid (low word)
        f.push_back(BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, self, 0, 1));
        ret(SECCOMP_RET_ALLOW);
        ret(deny);
    }
    // No new processes: threads (clone with CLONE_THREAD) only. clone3 keeps
    // its flags in memory a filter cannot read, so it reports ENOSYS and libc
    // falls back to clone, whose flags are an argument.
    f.push_back(BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_clone3, 0, 1));
    ret(SECCOMP_RET_ERRNO | ENOSYS);
    f.push_back(BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_clone, 0, 3));
    ld(offsetof(seccomp_data, args[0]));
    f.push_back(BPF_JUMP(BPF_JMP | BPF_JSET | BPF_K, CLONE_THREAD, 1, 0));
    ret(deny);
    ret(SECCOMP_RET_ALLOW);

    sock_fprog prog{static_cast<unsigned short>(f.size()), f.data()};
    // TSYNC: every thread, should a library have started one already.
    if (syscall(SYS_seccomp, SECCOMP_SET_MODE_FILTER, SECCOMP_FILTER_FLAG_TSYNC, &prog) != 0)
        return errno;
    return 0;
}

}  // namespace

std::string enter_sandbox(const std::string& read_root, const std::vector<std::string>& writable) {
    // Both need it, and it is what lets an unprivileged process confine itself.
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0) {
        warn("no_new_privs refused: no sandbox", errno);
        return "none";
    }
    const int ll = landlock(read_root, writable);
    if (ll) warn("Landlock unavailable: file access is not restricted", ll);
    const int sc = seccomp();
    if (sc) warn("seccomp unavailable: network and exec are not blocked", sc);
    if (!ll && !sc) return "landlock+seccomp";
    return !ll ? "landlock" : !sc ? "seccomp" : "none";
}

#elif defined(__APPLE__)
namespace {

std::string quoted(const std::string& s) {
    std::string q = "\"";
    for (char c : s) {
        if (c == '"' || c == '\\') q += '\\';
        q += c;
    }
    return q + "\"";
}

}  // namespace

std::string enter_sandbox(const std::string& read_root, const std::vector<std::string>& writable) {
    // Paths must be canonical (/private/var/..., not /var/...): Seatbelt
    // matches the resolved path. main hands us realpath()s.
    std::string profile =
        "(version 1)\n"
        "(deny default)\n"
        // stat() anywhere: realpath() walks the input's ancestors. It reveals
        // that a path exists, never what is in it.
        "(allow file-read-metadata)\n"
        "(allow sysctl-read)\n"  // the CPU count, for parallel meshing
        "(allow signal (target self))\n";  // abort(); never another process
    // subpath: a directory and below, or a file alone.
    if (!read_root.empty()) profile += "(allow file-read* (subpath " + quoted(read_root) + "))\n";
    for (const std::string& w : writable)
        profile += "(allow file-write-data (literal " + quoted(w) + "))\n";
    // Deprecated since 10.8 and still what Chromium, Firefox and the system's
    // own daemons confine themselves with; there is no successor API.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    char* err = nullptr;
    if (sandbox_init(profile.c_str(), 0, &err) != 0) {
        const int e = errno;
        std::fprintf(stderr, "stepv-occt: warning: sandbox_init: %s\n", err ? err : "failed");
        if (err) sandbox_free_error(err);
        // Seatbelt does not nest: inside another sandbox (an App-Sandboxed
        // parent, nix's build sandbox) ours cannot be added. That outer
        // sandbox's rules apply, and they are not ours.
        if (e == EPERM) {
            warn("already inside another sandbox: its rules apply, not stepv's", e);
            return "macos-outer";
        }
        warn("macOS sandbox refused: no sandbox", e);
        return "none";
    }
#pragma clang diagnostic pop
    return "macos-profile";
}

#else
std::string enter_sandbox(const std::string&, const std::vector<std::string>&) {
    warn("no sandbox for this platform", 0);
    return "none";
}
#endif

}  // namespace stepv
