/* Host-only diagnostic: build with clang -fsanitize=leak and pkg-config cairo. */
#include <cairo.h>
#include <stdio.h>
#include <string.h>

/* Modes are deliberately separate processes: reset is process-global. */
int main(int argc, char **argv) {
    if (argc != 2 || (strcmp(argv[1], "no-reset") &&
                      strcmp(argv[1], "terminal-reset") &&
                      strcmp(argv[1], "owner-order"))) {
        fputs("usage: cairo_font_lifetime no-reset|terminal-reset|owner-order\n", stderr);
        return 2;
    }
    cairo_surface_t *surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 80, 32);
    cairo_t *cr = cairo_create(surface);
    cairo_select_font_face(cr, "sans", CAIRO_FONT_SLANT_NORMAL, CAIRO_FONT_WEIGHT_NORMAL);
    cairo_set_font_size(cr, 14);
    cairo_move_to(cr, 2, 18);
    cairo_show_text(cr, "Nura");
    cairo_status_t status = cairo_status(cr);
    /* A retained app-owned font-face reference exercises finalizer ordering. */
    cairo_font_face_t *face = !strcmp(argv[1], "owner-order")
        ? cairo_font_face_reference(cairo_get_font_face(cr)) : NULL;
    cairo_destroy(cr);
    cairo_surface_destroy(surface);
    if (face)
        cairo_font_face_destroy(face);
    if (!strcmp(argv[1], "terminal-reset") || !strcmp(argv[1], "owner-order"))
        cairo_debug_reset_static_data();
    printf("MODE=%s CAIRO_STATUS=%d FINALIZERS=context,surface%s%s\n",
           argv[1], status, face ? ",retained-face" : "",
           strcmp(argv[1], "no-reset") ? ",terminal-reset" : "");
    return status == CAIRO_STATUS_SUCCESS ? 0 : 1;
}
