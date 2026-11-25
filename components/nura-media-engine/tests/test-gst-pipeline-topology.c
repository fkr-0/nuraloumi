/* Host-only parse regression: no device, decoder, GPU or media file needed. */
#include <gst/gst.h>
#include <stdio.h>

int main(void) {
    gst_init(NULL, NULL);
    const char *description =
        "filesrc name=media_source ! qtdemux name=d "
        "d.video_0 ! queue max-size-buffers=4 max-size-bytes=0 max-size-time=0 "
        "! identity name=video_decoder ! fakesink name=probe";
    GError *error = NULL;
    GstElement *pipeline = gst_parse_launch(description, &error);
    if (!pipeline || error) {
        fprintf(stderr, "parse error: %s\n", error ? error->message : "null pipeline");
        if (error) g_error_free(error);
        if (pipeline) gst_object_unref(pipeline);
        return 1;
    }
    printf("root=%s type=%s\n", GST_OBJECT_NAME(pipeline), G_OBJECT_TYPE_NAME(pipeline));
    GstIterator *iterator = gst_bin_iterate_recurse(GST_BIN(pipeline));
    GValue item = G_VALUE_INIT;
    while (gst_iterator_next(iterator, &item) == GST_ITERATOR_OK) {
        GstElement *element = g_value_get_object(&item);
        printf("element=%s type=%s\n", GST_OBJECT_NAME(element), G_OBJECT_TYPE_NAME(element));
        g_value_unset(&item);
    }
    gst_iterator_free(iterator);
    GstElement *decoder = gst_bin_get_by_name(GST_BIN(pipeline), "video_decoder");
    printf("decoder_found=%d\n", decoder != NULL);
    if (decoder) gst_object_unref(decoder);
    gst_object_unref(pipeline);
    return decoder ? 0 : 2;
}
