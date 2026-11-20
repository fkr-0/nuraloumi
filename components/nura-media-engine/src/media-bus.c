#include "media-bus.h"

#include <stdio.h>

void nura_media_bus_consume(NuraMediaBusState *state, GstMessage *message) {
    if (!state || !message) return;
    if (GST_MESSAGE_TYPE(message) == GST_MESSAGE_EOS) {
        state->eos = TRUE;
        puts("STREAM_EOS received=1");
    } else if (GST_MESSAGE_TYPE(message) == GST_MESSAGE_ERROR) {
        GError *error = NULL;
        gchar *debug = NULL;
        gst_message_parse_error(message, &error, &debug);
        const char *source = GST_MESSAGE_SRC(message)
            ? GST_OBJECT_NAME(GST_MESSAGE_SRC(message)) : "unknown";
        const char *reason = error ? error->message : "unknown";
        snprintf(state->error, sizeof(state->error),
                 "GStreamer %s: %s", source, reason);
        printf("STREAM_GST_ERROR source=%s reason=%s\n", source, reason);
        state->errors++;
        g_clear_error(&error);
        g_free(debug);
    }
}

void nura_media_bus_drain(GstBus *bus, NuraMediaBusState *state) {
    if (!bus || !state) return;
    GstMessage *message;
    while ((message = gst_bus_pop_filtered(bus, GST_MESSAGE_EOS |
                                              GST_MESSAGE_ERROR))) {
        nura_media_bus_consume(state, message);
        gst_message_unref(message);
    }
}
