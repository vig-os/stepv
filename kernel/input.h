// The kernel's input: a path, or a descriptor its caller opened (#91).
//
// A descriptor is READ, never re-opened by name (#102): /dev/fd/N is a
// name, and on macOS 15 the sandbox the kernel inherits from #64's Quick
// Look service denies opening it (EPERM), where macOS 26 allows it. So:
//   - STEP and BREP are read through FdStreamBuf, an istream over the
//     descriptor, by OCCT's stream readers. Their path readers open the
//     same kind of istream and run the same parser on it
//     (StepFile_Read, BRepTools::Read), so nothing differs but the open.
//   - IGES has no stream reader in OCCT 7.9.3 (IGESSelect_WorkLibrary does
//     not override ReadStream), so it is copied into a private file
//     (InputCopy) and read by that path, which is unlinked afterwards.
#pragma once

#include <fcntl.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

#include <algorithm>
#include <climits>
#include <cctype>
#include <cerrno>
#include <cstdlib>
#include <optional>
#include <istream>
#include <streambuf>
#include <string>

namespace stepv {

// N when `path` is exactly "/dev/fd/N": a file the caller opened and handed
// down, as #64's Quick Look service must (its sandbox opens nothing by
// path). Such a path is never resolved: realpath() names the descriptor's
// device, not the file. It has no extension either: the format is explicit.
inline std::optional<int> input_fd(const std::string& path) {
    constexpr const char* kPrefix = "/dev/fd/";
    if (path.rfind(kPrefix, 0) != 0) return std::nullopt;
    const std::string n = path.substr(8);
    if (n.empty() || n.size() > 9 || !std::all_of(n.begin(), n.end(), [](unsigned char c) {
            return std::isdigit(c);
        }))
        return std::nullopt;
    return static_cast<int>(std::strtol(n.c_str(), nullptr, 10));
}

inline std::string lower_ext(const std::string& path) {
    const auto dot = path.rfind('.');
    std::string e = dot == std::string::npos ? "" : path.substr(dot + 1);
    std::transform(e.begin(), e.end(), e.begin(), [](unsigned char c) { return std::tolower(c); });
    return e;
}

// The reader for `path`: "step", "iges" or "brep". `format` (stepv-occt
// --format) wins when given, else the extension decides. "" when neither
// names one, with `error` saying why.
inline std::string reader_format(const std::string& path, const std::string& format,
                                 std::string& error) {
    if (!format.empty()) {
        if (format == "step" || format == "iges" || format == "brep") return format;
        error = "unsupported --format: " + format + " (step, iges or brep)";
        return "";
    }
    if (input_fd(path)) {
        error = "an input descriptor (" + path + ") needs --format step|iges|brep";
        return "";
    }
    const std::string ext = lower_ext(path);
    if (ext == "step" || ext == "stp") return "step";
    if (ext == "iges" || ext == "igs") return "iges";
    if (ext == "brep") return "brep";
    error = "unsupported extension: ." + ext;
    return "";
}

// A descriptor the kernel can read from its start: open and seekable (a
// regular file). 0, or the errno that says why not (EBADF, ESPIPE).
inline int unreadable_fd(int fd) {
    return ::lseek(fd, 0, SEEK_CUR) < 0 ? errno : 0;
}

// An istream's buffer over descriptor `fd`, read with pread from offset 0:
// the descriptor's own offset, shared with its caller, is left as it is.
// Seeks move the read position. A read error ends the stream like EOF;
// error() then says which (0: none), so it is not taken for the file's.
class FdStreamBuf : public std::streambuf {
public:
    explicit FdStreamBuf(int fd) : fd_(fd) {}
    int error() const { return error_; }

protected:
    int_type underflow() override {
        if (gptr() < egptr()) return traits_type::to_int_type(*gptr());
        ssize_t n;
        do n = ::pread(fd_, buf_, sizeof buf_, next_);
        while (n < 0 && errno == EINTR);
        if (n < 0) error_ = errno;
        if (n <= 0) return traits_type::eof();
        next_ += n;
        setg(buf_, buf_, buf_ + n);
        return traits_type::to_int_type(*gptr());
    }
    pos_type seekoff(off_type off, std::ios_base::seekdir dir, std::ios_base::openmode) override {
        off_type base = next_ - (egptr() - gptr());  // where the reader is
        if (dir == std::ios_base::beg) base = 0;
        if (dir == std::ios_base::end) {
            struct stat st;
            if (::fstat(fd_, &st) != 0) return pos_type(off_type(-1));
            base = st.st_size;
        }
        return seekpos(pos_type(base + off), std::ios_base::in);
    }
    pos_type seekpos(pos_type pos, std::ios_base::openmode) override {
        if (off_type(pos) < 0) return pos_type(off_type(-1));
        next_ = off_type(pos);
        setg(buf_, buf_, buf_);
        return pos;
    }

private:
    int fd_;
    off_type next_ = 0;  // the file offset just past the buffer
    int error_ = 0;
    char buf_[1 << 16];
};

// A private copy of descriptor `fd`'s contents, for a reader that only
// takes a path (IGES), in $TMPDIR (else /tmp) as stepv-input-XXXXXX.<ext>.
// Unlinked when it goes out of scope; a process killed outright leaves it
// to its caller (#64's service sweeps its tmp). ok() false: error() is the
// errno, the environment's fault.
class InputCopy {
public:
    InputCopy(int fd, const std::string& ext) {
        const char* t = std::getenv("TMPDIR");
        std::string dir = t && *t ? t : "/tmp";
        if (dir.back() == '/') dir.pop_back();
        std::string tmpl = dir + "/stepv-input-XXXXXX." + ext;
        const int out = ::mkstemps(tmpl.data(), static_cast<int>(ext.size() + 1));
        if (out < 0) {
            error_ = errno;
            return;
        }
        // Its real path: a sandbox grants it by that (macOS's $TMPDIR is
        // under /var, a symlink to /private/var).
        char real[PATH_MAX];
        path_ = ::realpath(tmpl.c_str(), real) ? real : tmpl;
        char buf[1 << 16];
        off_t at = 0;
        for (;;) {
            ssize_t n;
            do n = ::pread(fd, buf, sizeof buf, at);
            while (n < 0 && errno == EINTR);
            if (n == 0) break;
            if (n < 0) {
                error_ = errno;
                break;
            }
            for (ssize_t w = 0; w < n;) {
                const ssize_t k = ::write(out, buf + w, static_cast<size_t>(n - w));
                if (k < 0 && errno == EINTR) continue;
                if (k <= 0) {
                    error_ = k < 0 ? errno : EIO;
                    break;
                }
                w += k;
            }
            if (error_) break;
            at += n;
        }
        if (::close(out) != 0 && !error_) error_ = errno;
    }
    ~InputCopy() {
        if (!path_.empty()) ::unlink(path_.c_str());
    }
    InputCopy(const InputCopy&) = delete;
    InputCopy& operator=(const InputCopy&) = delete;

    bool ok() const { return !path_.empty() && !error_; }
    int error() const { return error_; }
    const std::string& path() const { return path_; }

private:
    std::string path_;
    int error_ = 0;
};

}  // namespace stepv
