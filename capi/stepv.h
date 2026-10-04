// C ABI of stepv-capi: the renderer and header reader, for Swift.
// Every buffer or string returned here must be released with the matching
// free function; nothing here retains its inputs.
#ifndef STEPV_H
#define STEPV_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
    STEPV_OK = 0,
    STEPV_ERR_ARGS = 1,       // null pointer, size out of 16..=4096
    STEPV_ERR_DECODE = 2,     // not valid STEPVMSH v3
    STEPV_ERR_EMPTY = 3,      // nothing to draw
    STEPV_ERR_INTERNAL = 4,   // encoder failure or a caught panic: a bug
};

// Renders STEPVMSH v3 bytes (from stepv_occt_run) to a square PNG of `size`
// pixels, with the broken-face overlay, from stepv_view_angles' view: exactly
// what `stepv --png` draws.
// On STEPV_OK, *out / *out_len hold the PNG; free with stepv_buffer_free.
int32_t stepv_render_png(const uint8_t* mesh, size_t mesh_len, uint32_t size,
                         bool show_construction, uint8_t** out, size_t* out_len);

void stepv_buffer_free(uint8_t* p, size_t len);

// The view a front-end should open STEPVMSH v3 bytes on, as the angles of
// stepv's orbit camera: a flat part face-on, anything else from the default
// angle. The direction from the model toward the eye, in the Y-up frame
// (CAD (x, y, z) -> (x, z, -y)), is (-sin az cos el, sin el, cos az cos el).
int32_t stepv_view_angles(const uint8_t* mesh, size_t mesh_len, float* azimuth_deg,
                          float* elevation_deg);

// The `stepv --info` metadata of the file at `path`, as a JSON object. Never
// NULL for a non-NULL path: problems are reported in its header_error field.
// Free with stepv_string_free.
char* stepv_info_json(const char* path);

void stepv_string_free(char* p);

#ifdef __cplusplus
}
#endif

#endif
