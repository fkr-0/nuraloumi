#ifndef NURAL_MEDIA_AUDIO_H
#define NURAL_MEDIA_AUDIO_H

/* Probe local media through GStreamer's bounded discoverer, not a shell.
 * Returns 1 with audio presence and video geometry on success; 0 on
 * incomplete discovery. Guessing "no audio" can mask missing plugins.
 */
typedef struct {
    int has_audio;
    unsigned width;
    unsigned height;
    unsigned fps_n;
    unsigned fps_d;
} NuraMediaInfo;

/* Discover, but never decode, a local file. On failure leave *out unchanged. */
int nura_media_probe(const char *path, NuraMediaInfo *out);

#endif
