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
// A failure's "cause" is "input" when the file itself is at fault, and
// "environment" when the host, the output or files beside the input are (an
// external part file missing, an unwritable mesh): only an input failure
// repeats for the same file anywhere (#62). It is null on success.
// Thread-safe: calls are serialised, since OCCT's readers share global state.
//
// Healing a face during the transfer has a time budget (#51, heal.h):
// STEPV_OCCT_FACE_BUDGET_MS from the environment, read at each call, default
// 2000. A face past it is drawn as its outline and counted in the summary's
// "faces_over_budget".
char* stepv_occt_run(const char* input, const char* mesh_out, double linear_rel,
                     double angular_deg, int* exit_code);

// stepv_occt_run, also writing the model's exact topology (assembly tree,
// surface and curve types and parameters, areas, volumes; format at the top
// of topology.cpp) to `topology_out` (NULL: none). A topology that fails
// after a good mesh does not fail the run: the summary's "topology_error"
// says why, and no topology is written (#38). A file that already existed is
// left as it was, or emptied if the write itself failed partway. Out of
// memory stays fatal.
char* stepv_occt_run_topology(const char* input, const char* mesh_out, const char* topology_out,
                              double linear_rel, double angular_deg, int* exit_code);

// stepv_occt_run_topology, and with `edges` nonzero the mesh is STEPVMSH v4:
// every part also carries its B-rep edges as polylines, numbered as the
// topology numbers them (#31). Quick Look keeps v3 (edges = 0).
char* stepv_occt_run_ex(const char* input, const char* mesh_out, const char* topology_out,
                        int edges, double linear_rel, double angular_deg, int* exit_code);

void stepv_occt_free(char* p);

// A run's events as they happen (#54), for a front-end that shows the
// outline and progress before the geometry. Each is one JSON object:
//
//   STEPV_OCCT_EVENT_TREE      the outline, as stepv_occt_outline returns it,
//                              right after the file is parsed. STEP only, and
//                              left out when the outline itself fails or is
//                              over STEPV_OCCT_MAX_FRAME: the run goes on
//                              without it.
//   STEPV_OCCT_EVENT_PROGRESS  {"stage": "transfer"|"mesh", "done": k,
//                              "total": n}. Stages in that order; within one,
//                              done never falls and ends at total. transfer
//                              counts thousandths of OCCT's own progress,
//                              mesh counts two passes over the prototypes. A
//                              run that fails in a stage leaves it unfinished.
//                              At most one per stage every 50 ms, besides the
//                              first and the last.
//   STEPV_OCCT_EVENT_DONE      the summary. Never passed to the callback: it
//                              is what the run returns. The kind exists for
//                              the CLI's wire (below).
//
// Kinds 4 and up are reserved for per-part boxes and meshes (#54 P1).
//
// `stepv-occt --stream` writes these to stdout as frames, in place of the
// one JSON line: "SVEV", the kind as one byte, the payload's length as a
// little-endian u32, the payload, of at most STEPV_OCCT_MAX_FRAME bytes. The
// last frame is DONE (the summary, with "sandbox" as on the line); a stream
// that ends without it is a crash.
#define STEPV_OCCT_MAX_FRAME (64UL << 20)
#define STEPV_OCCT_EVENT_TREE 1
#define STEPV_OCCT_EVENT_PROGRESS 2
#define STEPV_OCCT_EVENT_DONE 3

// Called one call at a time, though not always on the thread running the
// run: transfer progress comes from whichever thread OCCT advances it on.
// The payload is valid for the call alone. It must not call back into the
// kernel.
typedef void (*stepv_occt_event_fn)(void* ctx, int kind, const char* payload,
                                    unsigned long length);

// stepv_occt_run_ex, passing its events to `on_event` (NULL: none) with
// `ctx` as they happen.
char* stepv_occt_run_stream(const char* input, const char* mesh_out, const char* topology_out,
                            int edges, double linear_rel, double angular_deg,
                            stepv_occt_event_fn on_event, void* ctx, int* exit_code);

// The assembly outline (#54, outline.h): the tree and names of the STEP file
// `input`, read from its product structure with no shape transfer, as JSON
// ({"format": "stepv-outline", ...}, or {"error": ...}) in a malloc'd string
// for stepv_occt_free. *exit_code is 0, or 3 with the error. Serialised with
// the runs above.
char* stepv_occt_outline(const char* input, int* exit_code);

// `stepv-occt --serve` (#33): reads `input` once, writes a ready line
// ({"ready": true, "sandbox": ..., "parts": n}, or ready false with an
// error) to `out_fd`, then answers one JSON query per line from `in_fd`
// until it closes (protocol in measure.h). Returns the exit code.
int stepv_occt_serve(const char* input, const char* sandbox, int in_fd, int out_fd);

#ifdef __cplusplus
}
#endif

#endif
