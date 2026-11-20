/* CLI for native media discovery. Run with silent MP4, AAC MP4, and an
 * alternate-size AAC MP4. Neither decoder nor Wayland starts here.
 */
#include "../src/media-audio.h"
#include <gst/gst.h>
#include <stdio.h>

int main(int argc, char **argv) {
    if (argc != 4) return 2;
    gst_init(NULL, NULL);
    for (int i = 1; i <= 3; ++i) {
        NuraMediaInfo info={0};
        if (!nura_media_probe(argv[i], &info)) return 3;
        printf("MEDIA_AUDIO_PROBE path=%d audio=%d width=%u height=%u fps=%u/%u\n",
               i, info.has_audio, info.width, info.height,info.fps_n,info.fps_d);
        if (info.has_audio != (i != 1) ||
            info.width != (i == 3 ? 80U : 64U) ||
            info.height != (i == 3 ? 48U : 64U) ||
            info.fps_n != 24 || info.fps_d != 1)
            return 4;
    }
    NuraMediaInfo unchanged={17,18,19,20,21};
    if (nura_media_probe("/nonexistent/nuraloumi-media.mp4", &unchanged))
        return 5;
    if (unchanged.has_audio != 17 || unchanged.width != 18 ||
        unchanged.height != 19 || unchanged.fps_n != 20 ||
        unchanged.fps_d != 21) return 6;
    return 0;
}
