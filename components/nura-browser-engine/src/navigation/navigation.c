/* SPDX-License-Identifier: MIT
 * WebKit 2.54+ / WPEPlatform navigation lane.
 * No legacy libwpe backend, DOM injection, threads or subprocesses.
 */
#define _POSIX_C_SOURCE 200809L
#include <nurabrowser/contract.h>

#include "state.h"
#include "url.h"
#include "../bookmarks/bookmarks.h"

#include <unistd.h>

#define RECENT_MAX_ENTRIES 8u
#define RECENT_MAX_URL_BYTES 320u
#define TABS_MAX 6u
#define BOOKMARK_PAGE_SIZE 8u

typedef struct {
    WebKitWebView *view;          /* retained as long as tab is open */
    gulong uri_signal, title_signal, loading_signal, load_signal;
} NavTab;

typedef struct _NavState {
    NuraBrowserContext *context; /* engine keeps this alive until shutdown */
    WebKitWebView *view;          /* strong reference, released on shutdown */
    GThread *owner;
    int directory_fd;
    GSource *save_source;
    char *committed_uri;
    char *uri;
    char *title;
    gboolean loading;
    gboolean can_back;
    gboolean can_forward;
    GPtrArray *recent;           /* bounded in-memory HTTP(S) pages */
    GPtrArray *tabs;             /* real, independent WebKitWebView objects */
    guint active;
    NuraBookmarkStore *bookmarks;
    NuraBrowserActivateView activate_view;
    gpointer activate_data;
} NavState;

static void
remember_recent(NavState *state, const char *url)
{
    char *safe = NULL;
    if (!nurabrowser_nav_normalize_url(url, &safe, NULL))
        return;
    /* Do not generate IPC frames that could exceed the 4096-byte server
     * result limit. Long valid pages still navigate; they are just omitted
     * from the optional recent-visit overview. */
    if (strlen(safe) > RECENT_MAX_URL_BYTES) {
        g_free(safe);
        return;
    }
    for (guint i = 0; i < state->recent->len; i++) {
        if (g_str_equal(g_ptr_array_index(state->recent, i), safe)) {
            g_ptr_array_remove_index(state->recent, i);
            break;
        }
    }
    g_ptr_array_insert(state->recent, 0, safe);
    if (state->recent->len > RECENT_MAX_ENTRIES)
        g_ptr_array_remove_index(state->recent, state->recent->len - 1);
}

/* Engine and control both execute on the same GLib main thread. The table is
 * deliberately not synchronized: dispatch from foreign threads is rejected. */
static GHashTable *states;
static GThread *main_thread;
static gboolean fail_dispatch(GError **error, GIOErrorEnum code, const char *message);

static void
snapshot(NavState *state)
{
    const char *uri = webkit_web_view_get_uri(state->view);
    const char *title = webkit_web_view_get_title(state->view);
    g_free(state->uri);
    g_free(state->title);
    /* Page-controlled title and redirect URL must not cause unbounded IPC
     * responses. The JSON emitter replaces truncated invalid UTF-8. */
    state->uri = g_strndup(uri ? uri : "", 8192);
    state->title = g_strndup(title ? title : "", 1024);
    state->loading = webkit_web_view_is_loading(state->view);
    state->can_back = webkit_web_view_can_go_back(state->view);
    state->can_forward = webkit_web_view_can_go_forward(state->view);
}

static void
json_string(GString *json, const char *text, gsize max_bytes)
{
    gsize length = text ? strnlen(text, max_bytes) : 0;
    g_string_append_c(json, '"');
    for (gsize i = 0; i < length; ) {
        unsigned char c = (unsigned char)text[i];
        if (c < 0x20 || c == '"' || c == '\\') {
            if (c == '"' || c == '\\') {
                g_string_append_c(json, '\\');
                g_string_append_c(json, (char)c);
            } else
                g_string_append_printf(json, "\\u%04x", (unsigned int)c);
            i++;
        } else if (c < 0x80) {
            g_string_append_c(json, (char)c);
            i++;
        } else {
            gunichar cp = g_utf8_get_char_validated(text + i, length - i);
            if (cp == (gunichar)-1 || cp == (gunichar)-2) {
                g_string_append(json, "\\ufffd");
                i++;
            } else {
                char bytes[8] = { 0 };
                gint count = g_unichar_to_utf8(cp, bytes);
                g_string_append_len(json, bytes, count);
                i += (gsize)count;
            }
        }
    }
    g_string_append_c(json, '"');
}

static char *
response_json(NavState *state, const char *command)
{
    if (g_str_equal(command, "tabs")) {
        GString *json = g_string_new("{\"command\":\"tabs\",\"tabs\":[");
        for (guint i = 0; i < state->tabs->len; i++) {
            NavTab *tab = g_ptr_array_index(state->tabs, i);
            if (i)
                g_string_append_c(json, ',');
            g_string_append(json, "{\"uri\":");
            json_string(json, webkit_web_view_get_uri(tab->view), 320);
            g_string_append(json, ",\"title\":");
            json_string(json, webkit_web_view_get_title(tab->view), 96);
            g_string_append_printf(json, ",\"loading\":%s}",
                                   webkit_web_view_is_loading(tab->view) ? "true" : "false");
        }
        g_string_append_printf(json, "],\"active\":%u}", state->active);
        return g_string_free(json, FALSE);
    }
    if (g_str_equal(command, "recent")) {
        GString *json = g_string_new("{\"command\":\"recent\",\"entries\":[");
        for (guint i = 0; i < state->recent->len; i++) {
            if (i)
                g_string_append_c(json, ',');
            json_string(json, g_ptr_array_index(state->recent, i),
                        RECENT_MAX_URL_BYTES);
        }
        g_string_append_printf(json, "],\"count\":%u}", state->recent->len);
        return g_string_free(json, FALSE);
    }
    GString *json = g_string_new("{\"command\":");
    json_string(json, command, 32);
    g_string_append(json, ",\"uri\":");
    json_string(json, state->uri, 8192);
    g_string_append(json, ",\"title\":");
    json_string(json, state->title, 1024);
    g_string_append_printf(json,
                           ",\"is_loading\":%s,\"can_go_back\":%s,"
                           "\"can_go_forward\":%s,\"bookmarked\":%s}",
                           state->loading ? "true" : "false",
                           state->can_back ? "true" : "false",
                           state->can_forward ? "true" : "false",
                           nurabrowser_bookmarks_contains(state->bookmarks, state->uri) ?
                           "true" : "false");
    return g_string_free(json, FALSE);
}

static char *
bookmarks_json(NavState *state, guint offset)
{
    guint count = nurabrowser_bookmarks_count(state->bookmarks);
    GString *json = g_string_new("{\"command\":\"bookmarks\",\"entries\":[");
    for (guint i = offset; i < count && i - offset < BOOKMARK_PAGE_SIZE; i++) {
        const NuraBookmark *entry = nurabrowser_bookmarks_get(state->bookmarks, i);
        if (i != offset)
            g_string_append_c(json, ',');
        g_string_append(json, "{\"url\":");
        json_string(json, entry->url, 320);
        g_string_append(json, ",\"title\":");
        json_string(json, entry->title, 160);
        g_string_append_c(json, '}');
    }
    g_string_append_printf(json, "],\"total\":%u,\"offset\":%u}", count, offset);
    return g_string_free(json, FALSE);
}

static gboolean
save_committed(gpointer data)
{
    NavState *state = data;
    GSource *source = state->save_source;
    state->save_source = NULL;
    if (state->committed_uri)
        nurabrowser_nav_state_write(state->directory_fd, state->committed_uri);
    g_source_unref(source);
    return G_SOURCE_REMOVE;
}

static void
schedule_save(NavState *state, const char *uri)
{
    char *safe_uri = NULL;
    if (state->directory_fd < 0 ||
        !nurabrowser_nav_normalize_url(uri, &safe_uri, NULL))
        return;
    g_free(state->committed_uri);
    state->committed_uri = safe_uri;
    if (state->save_source)
        return; /* coalesce redirects and rapid navigation */
    state->save_source = g_timeout_source_new(250);
    g_source_set_callback(state->save_source, save_committed, state, NULL);
    g_source_attach(state->save_source,
                    state->context->loop ?
                    g_main_loop_get_context(state->context->loop) :
                    g_main_context_default());
}

static void
view_changed(WebKitWebView *view, GParamSpec *property, gpointer data)
{
    NavState *state = data;
    (void)property;
    if (view == state->view)
        snapshot(state);
}

static void
load_changed(WebKitWebView *view, WebKitLoadEvent event, gpointer data)
{
    NavState *state = data;
    if (view == state->view)
        snapshot(state);
    if (event == WEBKIT_LOAD_COMMITTED) {
        const char *uri = webkit_web_view_get_uri(view);
        remember_recent(state, uri);
        if (view == state->view)
            schedule_save(state, uri);
    }
}

static void
tab_destroy(gpointer data)
{
    NavTab *tab = data;
    g_signal_handler_disconnect(tab->view, tab->uri_signal);
    g_signal_handler_disconnect(tab->view, tab->title_signal);
    g_signal_handler_disconnect(tab->view, tab->loading_signal);
    g_signal_handler_disconnect(tab->view, tab->load_signal);
    g_object_unref(tab->view);
    g_free(tab);
}

static NavTab *
tab_create(NavState *state, WebKitWebView *view)
{
    NavTab *tab = g_new0(NavTab, 1);
    tab->view = g_object_ref(view);
    tab->uri_signal = g_signal_connect(view, "notify::uri",
                                      G_CALLBACK(view_changed), state);
    tab->title_signal = g_signal_connect(view, "notify::title",
                                        G_CALLBACK(view_changed), state);
    tab->loading_signal = g_signal_connect(view, "notify::is-loading",
                                          G_CALLBACK(view_changed), state);
    tab->load_signal = g_signal_connect(view, "load-changed",
                                       G_CALLBACK(load_changed), state);
    return tab;
}

static gboolean
activate_tab(NavState *state, guint index, GError **error)
{
    if (index >= state->tabs->len)
        return fail_dispatch(error, G_IO_ERROR_INVALID_ARGUMENT, "Tab index is out of range");
    if (index == state->active)
        return TRUE;
    WebKitWebView *view = ((NavTab *)g_ptr_array_index(state->tabs, index))->view;
    if (state->activate_view &&
        !state->activate_view(state->view, view, state->activate_data, error))
        return FALSE;
    /* Both the engine context and current NavState own one active reference;
     * tab array retains independent references to every background WebView. */
    g_object_ref(view);
    g_object_unref(state->context->view);
    state->context->view = view;
    g_object_ref(view);
    g_object_unref(state->view);
    state->view = view;
    state->active = index;
    snapshot(state);
    /* Session resume tracks the selected tab, not the last background load.
     * Switching to about:blank remains intentionally non-persistent. */
    schedule_save(state, webkit_web_view_get_uri(state->view));
    return TRUE;
}

static gboolean
parse_small_index(const char *argument, guint bound, guint *index)
{
    if (!argument || !*argument || strlen(argument) > 2)
        return FALSE;
    for (const char *p = argument; *p; p++)
        if (!g_ascii_isdigit(*p))
            return FALSE;
    guint64 parsed = g_ascii_strtoull(argument, NULL, 10);
    if (parsed >= bound)
        return FALSE;
    *index = (guint)parsed;
    return TRUE;
}

static gboolean
fail_dispatch(GError **error, GIOErrorEnum code, const char *message)
{
    g_set_error_literal(error, G_IO_ERROR, code, message);
    return FALSE;
}

gboolean
nurabrowser_nav_dispatch(NuraBrowserContext *ctx, const char *command,
                         const char *argument, char **response, GError **error)
{
    NavState *state;
    char *normalized = NULL;

    if (response)
        *response = NULL;
    if (!ctx || !response || !command)
        return fail_dispatch(error, G_IO_ERROR_INVALID_ARGUMENT,
                             "Missing navigation context, command or response");
    if (!main_thread || main_thread != g_thread_self())
        return fail_dispatch(error, G_IO_ERROR_FAILED,
                             "Navigation must run on the engine's GLib main thread");
    state = states ? g_hash_table_lookup(states, ctx) : NULL;
    if (!state || !ctx->view || ctx->view != state->view)
        return fail_dispatch(error, G_IO_ERROR_CLOSED,
                             "Navigation is not attached to the active WebView");
    if (g_strcmp0(command, "open") == 0) {
        if (!nurabrowser_nav_normalize_url(argument, &normalized, error))
            return FALSE;
        webkit_web_view_load_uri(state->view, normalized);
        g_free(normalized);
    } else if (g_str_equal(command, "tab-new")) {
        if (argument && *argument)
            return fail_dispatch(error, G_IO_ERROR_INVALID_ARGUMENT,
                                 "tab-new takes no argument");
        if (state->tabs->len >= TABS_MAX)
            return fail_dispatch(error, G_IO_ERROR_NO_SPACE, "Maximum tab count reached");
        WebKitWebView *view = (WebKitWebView *)g_object_new(WEBKIT_TYPE_WEB_VIEW, NULL);
        if (!view)
            return fail_dispatch(error, G_IO_ERROR_FAILED, "Cannot create WebKit tab");
        guint index = state->tabs->len;
        g_ptr_array_add(state->tabs, tab_create(state, view));
        if (!activate_tab(state, index, error)) {
            g_ptr_array_remove_index(state->tabs, index);
            g_object_unref(view);
            return FALSE;
        }
        webkit_web_view_load_uri(view, "about:blank");
        g_object_unref(view);
    } else if (g_str_equal(command, "tab-select") ||
               g_str_equal(command, "tab-close")) {
        guint index = 0;
        if (!parse_small_index(argument, TABS_MAX, &index) || index >= state->tabs->len)
            return fail_dispatch(error, G_IO_ERROR_INVALID_ARGUMENT,
                                 "Invalid tab index");
        if (g_str_equal(command, "tab-select")) {
            if (!activate_tab(state, index, error))
                return FALSE;
        } else {
            if (state->tabs->len == 1)
                return fail_dispatch(error, G_IO_ERROR_FAILED,
                                     "Cannot close the last browser tab");
            if (index == state->active) {
                guint replacement = index == 0 ? 1 : index - 1;
                if (!activate_tab(state, replacement, error))
                    return FALSE;
            }
            g_ptr_array_remove_index(state->tabs, index);
            if (state->active > index)
                state->active--;
            snapshot(state);
        }
    } else if (g_str_equal(command, "bookmarks")) {
        guint offset = 0;
        if (argument && *argument) {
            if (!parse_small_index(argument, NURABROWSER_MAX_BOOKMARKS /
                                   BOOKMARK_PAGE_SIZE, &offset))
                return fail_dispatch(error, G_IO_ERROR_INVALID_ARGUMENT,
                                     "Invalid bookmark page");
            offset *= BOOKMARK_PAGE_SIZE;
        }
        if (!state->bookmarks)
            return fail_dispatch(error, G_IO_ERROR_CLOSED, "Bookmark store unavailable");
        *response = bookmarks_json(state, offset);
        return TRUE;
    } else if (g_str_equal(command, "bookmark-toggle")) {
        if (argument && *argument)
            return fail_dispatch(error, G_IO_ERROR_INVALID_ARGUMENT,
                                 "bookmark-toggle takes no argument");
        if (!state->bookmarks)
            return fail_dispatch(error, G_IO_ERROR_CLOSED, "Bookmark store unavailable");
        const char *uri = webkit_web_view_get_uri(state->view);
        if (nurabrowser_bookmarks_contains(state->bookmarks, uri)) {
            if (!nurabrowser_bookmarks_remove(state->bookmarks, uri, error))
                return FALSE;
        } else if (!nurabrowser_bookmarks_add(state->bookmarks, uri,
                                               webkit_web_view_get_title(state->view),
                                               error))
            return FALSE;
    } else {
        if (argument && *argument)
            return fail_dispatch(error, G_IO_ERROR_INVALID_ARGUMENT,
                                 "This navigation command does not take an argument");
        if (g_strcmp0(command, "back") == 0) {
            if (!webkit_web_view_can_go_back(state->view))
                return fail_dispatch(error, G_IO_ERROR_FAILED, "No previous history entry");
            webkit_web_view_go_back(state->view);
        } else if (g_strcmp0(command, "forward") == 0) {
            if (!webkit_web_view_can_go_forward(state->view))
                return fail_dispatch(error, G_IO_ERROR_FAILED, "No next history entry");
            webkit_web_view_go_forward(state->view);
        } else if (g_strcmp0(command, "reload") == 0) {
            if (!webkit_web_view_get_uri(state->view))
                return fail_dispatch(error, G_IO_ERROR_FAILED, "No page to reload");
            webkit_web_view_reload(state->view);
        } else if (g_strcmp0(command, "stop") == 0)
            webkit_web_view_stop_loading(state->view);
        else if (g_strcmp0(command, "uri") != 0 &&
                 g_strcmp0(command, "recent") != 0 &&
                 g_strcmp0(command, "tabs") != 0)
            return fail_dispatch(error, G_IO_ERROR_NOT_SUPPORTED,
                                 "Unsupported navigation command");
    }
    snapshot(state);
    *response = response_json(state, command);
    return TRUE;
}

void
nurabrowser_nav_attach(NuraBrowserContext *ctx)
{
    NavState *state;
    char *restored;

    if (!ctx || !ctx->view || !WEBKIT_IS_WEB_VIEW(ctx->view))
        return;
    if (main_thread && main_thread != g_thread_self())
        return;
    if (!main_thread)
        main_thread = g_thread_self();
    if (!states)
        states = g_hash_table_new(g_direct_hash, g_direct_equal);
    if (g_hash_table_contains(states, ctx))
        return;

    state = g_new0(NavState, 1);
    state->context = ctx;
    state->view = g_object_ref(ctx->view);
    state->recent = g_ptr_array_new_with_free_func(g_free);
    state->tabs = g_ptr_array_new_with_free_func(tab_destroy);
    g_ptr_array_add(state->tabs, tab_create(state, ctx->view));
    state->owner = g_thread_self();
    state->directory_fd = nurabrowser_nav_state_open(ctx->state_dir);
    state->bookmarks = nurabrowser_bookmarks_open(ctx->state_dir, NULL);
    g_hash_table_insert(states, ctx, state);
    snapshot(state);

    /* One last committed HTTP(S) URL is restored. We deliberately do not
     * load a blank page or replace a URL already loaded by the engine. */
    restored = nurabrowser_nav_state_read(state->directory_fd);
    if (restored && (!webkit_web_view_get_uri(state->view) ||
                     g_strcmp0(webkit_web_view_get_uri(state->view),
                               "about:blank") == 0))
        webkit_web_view_load_uri(state->view, restored);
    g_free(restored);
}

void
nurabrowser_nav_set_activate_view(NuraBrowserContext *ctx,
                                  NuraBrowserActivateView callback,
                                  gpointer user_data)
{
    if (!ctx || !states || !main_thread || main_thread != g_thread_self())
        return;
    NavState *state = g_hash_table_lookup(states, ctx);
    if (!state)
        return;
    state->activate_view = callback;
    state->activate_data = user_data;
}

void
nurabrowser_nav_shutdown(NuraBrowserContext *ctx)
{
    NavState *state;
    if (!ctx || !main_thread || main_thread != g_thread_self() || !states)
        return;
    state = g_hash_table_lookup(states, ctx);
    if (!state)
        return;
    /* First disconnect signals, then destroy callbacks, then persist the
     * last known COMMITTED URI. No callback may retain a freed state. */
    /* An inactive WebKit view can outlive its native tab until WebProcess
     * teardown. Disconnect every callback into the stack-owned engine, not
     * only the active view's callbacks. */
    if (state->activate_data) {
        for (guint i = 0; i < state->tabs->len; i++) {
            NavTab *tab = g_ptr_array_index(state->tabs, i);
            g_signal_handlers_disconnect_by_data(tab->view, state->activate_data);
        }
    }
    state->activate_view = NULL;
    state->activate_data = NULL;
    g_ptr_array_unref(state->tabs);
    if (state->save_source) {
        g_source_destroy(state->save_source);
        g_source_unref(state->save_source);
        state->save_source = NULL;
    }
    if (state->committed_uri)
        nurabrowser_nav_state_write(state->directory_fd, state->committed_uri);
    if (state->directory_fd >= 0)
        close(state->directory_fd);
    g_object_unref(state->view);
    g_free(state->uri);
    g_free(state->title);
    g_free(state->committed_uri);
    g_ptr_array_unref(state->recent);
    nurabrowser_bookmarks_close(state->bookmarks);
    g_hash_table_remove(states, ctx);
    g_free(state);
}
