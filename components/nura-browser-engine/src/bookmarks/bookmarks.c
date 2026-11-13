/* SPDX-License-Identifier: MIT
 * Bounded, crash-safe private bookmark store. Never constructs shell commands
 * or logs visited URLs. Persistent state is different from volatile recent.
 */
#define _POSIX_C_SOURCE 200809L
#include "bookmarks.h"
#include "../navigation/state.h"
#include "../navigation/url.h"

#include <json-glib/json-glib.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define BOOKMARK_FILE "bookmarks.v1"
#define BOOKMARK_MAX_FILE (96u * 1024u)
#define BOOKMARK_MAX_URL 320u
#define BOOKMARK_MAX_TITLE 160u

typedef struct {
    NuraBookmark public;
} OwnedBookmark;

struct _NuraBookmarkStore {
    int dir_fd;
    GPtrArray *items;
};

static gboolean
store_error(GError **error, GIOErrorEnum code, const char *reason)
{
    g_set_error_literal(error, G_IO_ERROR, code, reason);
    return FALSE;
}

static void
bookmark_free(gpointer ptr)
{
    OwnedBookmark *bookmark = ptr;
    g_free((gpointer)bookmark->public.url);
    g_free((gpointer)bookmark->public.title);
    g_free(bookmark);
}

static gboolean
valid_title(const char *title)
{
    if (!title || !*title || strnlen(title, BOOKMARK_MAX_TITLE + 1) > BOOKMARK_MAX_TITLE ||
        !g_utf8_validate(title, -1, NULL))
        return FALSE;
    for (const unsigned char *it = (const unsigned char *)title; *it; it++)
        if (*it < 0x20 || *it == 0x7f)
            return FALSE;
    return TRUE;
}

static char *
default_title(const char *url)
{
    g_autoptr(GUri) uri = g_uri_parse(url, G_URI_FLAGS_NONE, NULL);
    const char *host = uri ? g_uri_get_host(uri) : NULL;
    return g_strdup(host && *host ? host : "Bookmarked page");
}

static OwnedBookmark *
new_bookmark(const char *url, const char *title)
{
    OwnedBookmark *bookmark = g_new0(OwnedBookmark, 1);
    bookmark->public.url = g_strdup(url);
    bookmark->public.title = g_strdup(title);
    return bookmark;
}

static gint
find_url(const NuraBookmarkStore *store, const char *url)
{
    for (guint i = 0; i < store->items->len; i++) {
        const NuraBookmark *entry = &((OwnedBookmark *)g_ptr_array_index(store->items, i))->public;
        if (g_strcmp0(entry->url, url) == 0)
            return (gint)i;
    }
    return -1;
}

/* Do not ever follow a hostile destination during open, and never overwrite
 * a pre-existing non-private file in a user-controlled state directory. */
static gboolean
destination_safe(int dir_fd)
{
    struct stat st;
    if (fstatat(dir_fd, BOOKMARK_FILE, &st, AT_SYMLINK_NOFOLLOW) != 0)
        return errno == ENOENT;
    return S_ISREG(st.st_mode) && st.st_uid == geteuid() &&
           st.st_nlink == 1 && (st.st_mode & 0077) == 0;
}

static gboolean
read_bookmarks(NuraBookmarkStore *store, GError **error)
{
    struct stat st;
    int fd = openat(store->dir_fd, BOOKMARK_FILE,
                    O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
    if (fd < 0) {
        if (errno == ENOENT)
            return TRUE;
        return store_error(error, G_IO_ERROR_PERMISSION_DENIED,
                           "Cannot safely read bookmark state");
    }
    gboolean ok = FALSE;
    g_autofree char *bytes = NULL;
    g_autoptr(JsonParser) parser = NULL;
    if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_uid != geteuid() ||
        st.st_nlink != 1 || (st.st_mode & 0077) != 0 ||
        st.st_size < 2 || st.st_size > BOOKMARK_MAX_FILE)
        goto done;
    bytes = g_malloc((gsize)st.st_size + 1);
    gsize used = 0;
    while (used < (gsize)st.st_size) {
        ssize_t n = read(fd, bytes + used, (gsize)st.st_size - used);
        if (n < 0 && errno == EINTR)
            continue;
        if (n <= 0)
            goto done;
        used += (gsize)n;
    }
    bytes[used] = '\0';
    if (memchr(bytes, '\0', used) || !g_utf8_validate(bytes, (gssize)used, NULL))
        goto done;
    parser = json_parser_new();
    if (!json_parser_load_from_data(parser, bytes, (gssize)used, NULL) ||
        !JSON_NODE_HOLDS_OBJECT(json_parser_get_root(parser)))
        goto done;
    JsonObject *root = json_node_get_object(json_parser_get_root(parser));
    JsonNode *version = json_object_get_member(root, "version");
    JsonNode *array = json_object_get_member(root, "entries");
    if (!version || !JSON_NODE_HOLDS_VALUE(version) ||
        json_node_get_value_type(version) != G_TYPE_INT64 ||
        json_node_get_int(version) != 1 ||
        !array || !JSON_NODE_HOLDS_ARRAY(array))
        goto done;
    JsonArray *entries = json_node_get_array(array);
    if (!entries || json_array_get_length(entries) > NURABROWSER_MAX_BOOKMARKS)
        goto done;
    for (guint i = 0; i < json_array_get_length(entries); i++) {
        JsonNode *node = json_array_get_element(entries, i);
        if (!JSON_NODE_HOLDS_OBJECT(node))
            goto done;
        JsonObject *object = json_node_get_object(node);
        JsonNode *url_node = json_object_get_member(object, "url");
        JsonNode *title_node = json_object_get_member(object, "title");
        if (!url_node || !title_node ||
            !JSON_NODE_HOLDS_VALUE(url_node) ||
            !JSON_NODE_HOLDS_VALUE(title_node) ||
            json_node_get_value_type(url_node) != G_TYPE_STRING ||
            json_node_get_value_type(title_node) != G_TYPE_STRING)
            goto done;
        const char *url = json_object_get_string_member_with_default(object, "url", NULL);
        const char *title = json_object_get_string_member_with_default(object, "title", NULL);
        g_autofree char *safe = NULL;
        if (!url || !title || strlen(url) > BOOKMARK_MAX_URL ||
            !nurabrowser_nav_normalize_url(url, &safe, NULL) ||
            !valid_title(title) || find_url(store, safe) >= 0)
            goto done;
        g_ptr_array_add(store->items, new_bookmark(safe, title));
    }
    ok = TRUE;
done:
    close(fd);
    if (!ok)
        return store_error(error, G_IO_ERROR_INVALID_DATA,
                           "Bookmark state is invalid or insecure; refusing overwrite");
    return TRUE;
}

static gboolean
write_all(int fd, const char *data, gsize size)
{
    while (size) {
        ssize_t n = write(fd, data, size);
        if (n < 0 && errno == EINTR)
            continue;
        if (n <= 0)
            return FALSE;
        data += n;
        size -= (gsize)n;
    }
    return TRUE;
}

static gboolean
save_bookmarks(NuraBookmarkStore *store, GError **error)
{
    if (!destination_safe(store->dir_fd))
        return store_error(error, G_IO_ERROR_PERMISSION_DENIED,
                           "Bookmark state path has unsafe ownership or links");
    g_autoptr(JsonBuilder) builder = json_builder_new();
    json_builder_begin_object(builder);
    json_builder_set_member_name(builder, "version");
    json_builder_add_int_value(builder, 1);
    json_builder_set_member_name(builder, "entries");
    json_builder_begin_array(builder);
    for (guint i = 0; i < store->items->len; i++) {
        const NuraBookmark *entry = nurabrowser_bookmarks_get(store, i);
        json_builder_begin_object(builder);
        json_builder_set_member_name(builder, "url");
        json_builder_add_string_value(builder, entry->url);
        json_builder_set_member_name(builder, "title");
        json_builder_add_string_value(builder, entry->title);
        json_builder_end_object(builder);
    }
    json_builder_end_array(builder);
    json_builder_end_object(builder);
    g_autoptr(JsonNode) root = json_builder_get_root(builder);
    g_autoptr(JsonGenerator) generator = json_generator_new();
    json_generator_set_root(generator, root);
    gsize size = 0;
    g_autofree char *data = json_generator_to_data(generator, &size);
    if (!data || size > BOOKMARK_MAX_FILE)
        return store_error(error, G_IO_ERROR_NO_SPACE, "Bookmark state exceeds size limit");

    char temp[80] = { 0 };
    int fd = -1;
    for (guint i = 0; i < 16; i++) {
        g_snprintf(temp, sizeof(temp), ".bookmarks-%08x.tmp", g_random_int());
        fd = openat(store->dir_fd, temp, O_WRONLY | O_CREAT | O_EXCL |
                    O_CLOEXEC | O_NOFOLLOW, 0600);
        if (fd >= 0)
            break;
        if (errno != EEXIST)
            break;
    }
    if (fd < 0)
        return store_error(error, G_IO_ERROR_FAILED, "Cannot create private bookmark transaction");
    gboolean ok = write_all(fd, data, size) && fchmod(fd, 0600) == 0 &&
                  fsync(fd) == 0;
    if (close(fd) != 0)
        ok = FALSE;
    fd = -1;
    if (ok)
        ok = destination_safe(store->dir_fd) &&
             renameat(store->dir_fd, temp, store->dir_fd, BOOKMARK_FILE) == 0 &&
             fsync(store->dir_fd) == 0;
    if (!ok) {
        unlinkat(store->dir_fd, temp, 0);
        return store_error(error, G_IO_ERROR_FAILED, "Bookmark transaction could not be committed");
    }
    return TRUE;
}

NuraBookmarkStore *
nurabrowser_bookmarks_open(const char *state_dir, GError **error)
{
    int dir = nurabrowser_nav_state_open(state_dir);
    if (dir < 0) {
        store_error(error, G_IO_ERROR_PERMISSION_DENIED,
                    "Bookmark directory must be owned, private and symlink-free");
        return NULL;
    }
    NuraBookmarkStore *store = g_new0(NuraBookmarkStore, 1);
    store->dir_fd = dir;
    store->items = g_ptr_array_new_with_free_func(bookmark_free);
    if (!read_bookmarks(store, error)) {
        nurabrowser_bookmarks_close(store);
        return NULL;
    }
    return store;
}

void
nurabrowser_bookmarks_close(NuraBookmarkStore *store)
{
    if (!store)
        return;
    g_ptr_array_unref(store->items);
    close(store->dir_fd);
    g_free(store);
}

guint
nurabrowser_bookmarks_count(const NuraBookmarkStore *store)
{
    return store ? store->items->len : 0;
}

const NuraBookmark *
nurabrowser_bookmarks_get(const NuraBookmarkStore *store, guint index)
{
    return store && index < store->items->len
        ? &((OwnedBookmark *)g_ptr_array_index(store->items, index))->public : NULL;
}

gboolean
nurabrowser_bookmarks_contains(const NuraBookmarkStore *store, const char *url)
{
    g_autofree char *safe = NULL;
    return store && nurabrowser_nav_normalize_url(url, &safe, NULL) &&
           find_url(store, safe) >= 0;
}

gboolean
nurabrowser_bookmarks_add(NuraBookmarkStore *store, const char *url,
                           const char *title, GError **error)
{
    g_autofree char *safe = NULL;
    if (!store || !nurabrowser_nav_normalize_url(url, &safe, NULL) ||
        strlen(safe) > BOOKMARK_MAX_URL)
        return store_error(error, G_IO_ERROR_INVALID_ARGUMENT, "Invalid bookmark URL");
    if (title && *title && !valid_title(title))
        return store_error(error, G_IO_ERROR_INVALID_ARGUMENT, "Invalid bookmark title");
    g_autofree char *fallback = title && *title ? NULL : default_title(safe);
    const char *label = fallback ? fallback : title;
    if (!valid_title(label))
        return store_error(error, G_IO_ERROR_INVALID_ARGUMENT, "Invalid bookmark title");
    gint index = find_url(store, safe);
    if (index < 0 && store->items->len >= NURABROWSER_MAX_BOOKMARKS)
        return store_error(error, G_IO_ERROR_NO_SPACE, "Bookmark capacity reached");
    if (index >= 0) {
        OwnedBookmark *entry = g_ptr_array_index(store->items, (guint)index);
        char *previous = (char *)entry->public.title;
        entry->public.title = g_strdup(label);
        if (!save_bookmarks(store, error)) {
            g_free((gpointer)entry->public.title);
            entry->public.title = previous;
            return FALSE;
        }
        g_free(previous);
    } else {
        g_ptr_array_add(store->items, new_bookmark(safe, label));
        if (!save_bookmarks(store, error)) {
            g_ptr_array_remove_index(store->items, store->items->len - 1);
            return FALSE;
        }
    }
    return TRUE;
}

gboolean
nurabrowser_bookmarks_remove(NuraBookmarkStore *store, const char *url,
                              GError **error)
{
    g_autofree char *safe = NULL;
    if (!store || !nurabrowser_nav_normalize_url(url, &safe, NULL))
        return store_error(error, G_IO_ERROR_INVALID_ARGUMENT, "Invalid bookmark URL");
    gint index = find_url(store, safe);
    if (index < 0)
        return store_error(error, G_IO_ERROR_NOT_FOUND, "Bookmark does not exist");
    OwnedBookmark *item = g_ptr_array_steal_index(store->items, (guint)index);
    if (!save_bookmarks(store, error)) {
        g_ptr_array_insert(store->items, (guint)index, item);
        return FALSE;
    }
    bookmark_free(item);
    return TRUE;
}
