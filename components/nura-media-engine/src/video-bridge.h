/* One in-process NuraLoumi video engine, owned exclusively by playerd.
 * No standalone media player, shell command, GPU fallback, or second IPC.
 * ABI v1: thread-safe command/status, bounded path, monotonic PTS.
 */
#ifndef NURAL_VIDEO_BRIDGE_H
#define NURAL_VIDEO_BRIDGE_H
#include <stdint.h>
typedef struct NuraVideo NuraVideo;
typedef struct {
    int32_t state; /* -1 error; 0 initializing; 1 playing; 2 paused; 3 EOS */
    uint64_t position_ms;
    uint64_t duration_ms;
    uint64_t frames;
    char error[192];
} NuraVideoStatus;
typedef struct {
    int stop, pause, muted, volume_dirty;
    int64_t seek_ms; /* -1 means none; consumed exactly once */
    double volume;
} NuraVideoControls;

/* Public C ABI dynamically loaded by the existing playerd Rust process. */
NuraVideo *nura_video_open(const char *absolute_file, double volume, int muted);
int nura_video_command(NuraVideo *handle, int command, int64_t argument);
int nura_video_status(NuraVideo *handle, NuraVideoStatus *out);
void nura_video_close(NuraVideo *handle);

/* Internal bridge: callable only by the single background render thread. */
int nura_video_render_loop(NuraVideo *handle);
const char *nura_video_path(const NuraVideo *handle);
NuraVideoControls nura_video_controls(NuraVideo *handle);
void nura_video_publish(NuraVideo *handle, int state,
                        uint64_t pts_ms, uint64_t duration_ms,
                        uint64_t frames);
void nura_video_error(NuraVideo *handle, const char *message);
#endif
