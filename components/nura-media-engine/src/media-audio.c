#include "media-audio.h"

#include <gst/pbutils/pbutils.h>
#include <stdio.h>

int nura_media_is_qualified_video(const NuraMediaInfo *media) {
    return media && media->width == 854 && media->height == 480 &&
           media->fps_n == 24 && media->fps_d == 1;
}

int nura_media_probe(const char *path, NuraMediaInfo *out) {
    if (!path || !out) return 0;
    GError *error = NULL;
    gchar *uri = g_filename_to_uri(path, NULL, &error);
    if (!uri) {
        g_clear_error(&error);
        return 0;
    }
    GstDiscoverer *discoverer = gst_discoverer_new(3 * GST_SECOND, &error);
    if (!discoverer) {
        g_free(uri);
        g_clear_error(&error);
        return 0;
    }
    GstDiscovererInfo *info = gst_discoverer_discover_uri(discoverer, uri, &error);
    int ok = info && gst_discoverer_info_get_result(info) == GST_DISCOVERER_OK;
    if (ok) {
        GList *videos = gst_discoverer_info_get_video_streams(info);
        if (!videos) {
            ok = 0;
        } else {
            GstDiscovererVideoInfo *video = GST_DISCOVERER_VIDEO_INFO(videos->data);
            NuraMediaInfo detected = {
                .width = gst_discoverer_video_info_get_width(video),
                .height = gst_discoverer_video_info_get_height(video),
                .fps_n = gst_discoverer_video_info_get_framerate_num(video),
                .fps_d = gst_discoverer_video_info_get_framerate_denom(video),
            };
            GList *audios = gst_discoverer_info_get_audio_streams(info);
            detected.has_audio = audios != NULL;
            gst_discoverer_stream_info_list_free(audios);
            *out = detected;
        }
        gst_discoverer_stream_info_list_free(videos);
    } else {
        fprintf(stderr, "STREAM_AUDIO_PROBE_FAIL discovery=%d\n",
                info ? (int)gst_discoverer_info_get_result(info) : -1);
    }
    if (info) gst_discoverer_info_unref(info);
    g_object_unref(discoverer);
    g_free(uri);
    g_clear_error(&error);
    return ok;
}
