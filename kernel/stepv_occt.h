// C ABI of the stepv OCCT kernel (libstepvocct). See stepv-occt-core.cpp.
#ifndef STEPV_OCCT_H
#define STEPV_OCCT_H

#ifdef __cplusplus
extern "C" {
#endif

// Reads `input` (STEP/IGES/BREP by extension), tessellates at `linear_rel` of
// the bbox diagonal and `angular_deg`, and writes STEPVMSH v3 to `mesh_out`
// (NULL: summary only). Returns the JSON summary — the same one stepv-occt
// prints — as a malloc'd string to release with stepv_occt_free, and sets
// *exit_code to 0 (geometry) or 3 (clean failure; the summary says why).
// Thread-safe: calls are serialised, since OCCT's readers share global state.
char* stepv_occt_run(const char* input, const char* mesh_out, double linear_rel,
                     double angular_deg, int* exit_code);

void stepv_occt_free(char* p);

#ifdef __cplusplus
}
#endif

#endif
