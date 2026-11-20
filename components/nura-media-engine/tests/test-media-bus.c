/* Deterministic bus-state regression. No media, decoder or Wayland needed. */
#include "../src/media-bus.h"

#include <stdio.h>
#include <string.h>

int main(void) {
    gst_init(NULL, NULL);
    GstBus *bus = gst_bus_new();
    GstElement *source = gst_pipeline_new("test-source");
    if (!bus || !source) return 2;
    NuraMediaBusState state = {0};

    /* The main loop may consume EOS before the appsink observes its own EOS. */
    if (!gst_bus_post(bus, gst_message_new_eos(GST_OBJECT(source)))) return 3;
    nura_media_bus_drain(bus, &state);
    if (!state.eos || state.errors) return 4;
    nura_media_bus_drain(bus, &state);
    if (!state.eos || state.errors) return 5;

    GError *error = g_error_new_literal(GST_CORE_ERROR,
                                       GST_CORE_ERROR_FAILED,
                                       "synthetic bus failure");
    GstMessage *failure = gst_message_new_error(GST_OBJECT(source),
                                               error, "test debug");
    g_clear_error(&error);
    if (!gst_bus_post(bus, failure)) return 6;
    nura_media_bus_drain(bus, &state);
    if (!state.eos || state.errors != 1 ||
        !strstr(state.error, "synthetic bus failure")) return 7;
    nura_media_bus_drain(bus, &state);
    if (state.errors != 1) return 8;

    printf("MEDIA_BUS_REGRESSION_PASS eos_sticky=1 errors_sticky=1\n");
    gst_object_unref(source);
    gst_object_unref(bus);
    return 0;
}
