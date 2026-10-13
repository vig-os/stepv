// A stand-in for stepv-occt (#64), put in place of the kernel in a copy of
// the built service by scripts/ql-e2e/supervisor.sh, to hold the service to
// streams the real kernel never writes on cue. Its input (fd 3, as the
// service hands it) names what it does:
//
//   frames     TREE, PROGRESS and DONE, then exit 0 at once
//   late       exit 0 at once, while a process it leaves holds stdout and
//              writes TREE, PROGRESS and DONE 200 ms later: the service sees
//              the exit before the frames, and must drain the pipe to its
//              end rather than decide on the exit (deterministically what a
//              kernel's last frames still in the pipe at its exit are)
//   silent     exit 0 with nothing written: no summary is a crash
//   truncated  half a frame, then exit 0: a crash
//   noexit     DONE, then close stdout and never exit: the deadline ends it
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void frame(uint8_t kind, const char* payload) {
    uint32_t n = (uint32_t)strlen(payload);
    uint8_t head[9] = {'S', 'V', 'E', 'V', kind, n & 0xff, n >> 8 & 0xff, n >> 16 & 0xff, n >> 24};
    write(1, head, sizeof head);
    write(1, payload, n);
}

int main(void) {
    char mode[16] = {0};
    if (pread(3, mode, sizeof mode - 1, 0) < 0) return 2;
    mode[strcspn(mode, "\n")] = 0;
    if (!strcmp(mode, "silent")) return 0;
    if (!strcmp(mode, "truncated")) {
        write(1, "SVEV\x03\x10\0\0\0{\"ok\"", 13);
        return 0;
    }
    if (!strcmp(mode, "late")) {
        if (fork() != 0) return 0;
        usleep(200000);
    }
    frame(1, "{}");
    frame(2, "{\"stage\":\"transfer\",\"done\":1,\"total\":1}");
    frame(3, "{\"ok\":true,\"sandbox\":\"stand-in\"}");
    if (!strcmp(mode, "noexit")) {
        close(1);
        for (;;) pause();
    }
    if (!strcmp(mode, "late")) _exit(0);
    return 0;
}
