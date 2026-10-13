// The kernel's input: a path, or a descriptor its caller opened (#91).
#pragma once

#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <optional>
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

}  // namespace stepv
