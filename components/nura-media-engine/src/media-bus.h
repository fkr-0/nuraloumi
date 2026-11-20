#ifndef NURAL_MEDIA_BUS_H
#define NURAL_MEDIA_BUS_H

#include <gst/gst.h>

/* Owned by the single video render thread. EOS and errors are sticky: a
 * message drained before the appsink reaches EOS must not be forgotten. */
typedef struct {
    gboolean eos;
    unsigned errors;
    char error[192];
} NuraMediaBusState;

void nura_media_bus_consume(NuraMediaBusState *state, GstMessage *message);
void nura_media_bus_drain(GstBus *bus, NuraMediaBusState *state);

#endif
