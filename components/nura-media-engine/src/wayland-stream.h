/* Persistent DMA-BUF Wayland stream. Each committed GBM buffer is owned by
 * the compositor until wl_buffer.release; never release a busy GBM BO. */
#ifndef SL101_WAYLAND_STREAM_H
#define SL101_WAYLAND_STREAM_H
#include <gbm.h>
#include <stdint.h>
struct sl101_stream;
struct sl101_stream *sl101_stream_open(void);
/* Caller retains GBM surface and EGL context until stream_close().
 * Takes ownership of BO only after successful submission. */
int sl101_stream_submit(struct sl101_stream *,struct gbm_surface *,
                        struct gbm_bo *,int fd,unsigned frame);
int sl101_stream_close(struct sl101_stream *);
unsigned sl101_stream_releases(const struct sl101_stream *);
unsigned sl101_stream_callbacks(const struct sl101_stream *);
#endif
